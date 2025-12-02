use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::MaybeTlsStream;

/// Minimal market data event used by the latency logic.
pub struct MarketDataEvent {
    pub recv_ts_mono: Instant,
    pub channel: String,
}

/// RPC response including timestamps when the message was received.
pub struct RpcResponse {
    pub result: Option<Value>,
    pub error: Option<Value>,
    pub raw: Value,
    pub recv_ts_mono: Instant,
    pub recv_ts_wall: DateTime<Utc>,
}

pub struct DeribitClient {
    pub(crate) ws_tx: futures::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<MaybeTlsStream<TcpStream>>,
        Message,
    >,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<RpcResponse>>>>,
    next_id: Arc<Mutex<i64>>,
}

impl DeribitClient {
    pub async fn connect(
        testnet: bool,
        client_id: &str,
        client_secret: &str,
        md_tx: mpsc::UnboundedSender<MarketDataEvent>,
    ) -> Result<Self> {
        let url = if testnet {
            "wss://test.deribit.com/ws/api/v2"
        } else {
            "wss://www.deribit.com/ws/api/v2"
        };

        let request = url.into_client_request()?;
        let (ws_stream, _response) = connect_async(request).await?;
        let (ws_tx, ws_rx) = ws_stream.split();

        let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<RpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_reader = pending.clone();
        let next_id = Arc::new(Mutex::new(1_i64));

        // Spawn reader task
        tokio::spawn(async move {
            let mut ws_rx = ws_rx;
            while let Some(msg) = ws_rx.next().await {
                match msg {
                    Ok(Message::Text(txt)) => {
                        let recv_ts_mono = Instant::now();
                        let recv_ts_wall = Utc::now();
                        if let Ok(raw) = serde_json::from_str::<Value>(&txt) {
                            let id_opt = raw.get("id").and_then(|v| v.as_i64());
                            if let Some(id) = id_opt {
                                // RPC response
                                let result = raw.get("result").cloned();
                                let error = raw.get("error").cloned();
                                let resp = RpcResponse {
                                    result,
                                    error,
                                    raw,
                                    recv_ts_mono,
                                    recv_ts_wall,
                                };
                                let mut guard = pending_reader.lock().await;
                                if let Some(tx) = guard.remove(&id) {
                                    let _ = tx.send(resp);
                                }
                            } else if raw.get("method").and_then(|m| m.as_str())
                                == Some("subscription")
                            {
                                // Subscription event
                                if let Some(params) = raw.get("params") {
                                    if let Some(channel) =
                                        params.get("channel").and_then(|c| c.as_str())
                                    {
                                        let evt = MarketDataEvent {
                                            recv_ts_mono,
                                            channel: channel.to_string(),
                                        };
                                        let _ = md_tx.send(evt);
                                    }
                                }
                            } else {
                                // Other messages (heartbeat, etc.) can be ignored for now
                            }
                        }
                    }
                    Ok(Message::Ping(_)) => {
                        // tungstenite will usually handle Pong automatically.
                    }
                    Ok(Message::Pong(_)) => {
                        // Ignore.
                    }
                    Ok(Message::Binary(_)) => {
                        // We don't use binary frames for Deribit JSON-RPC.
                    }
                    Ok(Message::Close(_)) => {
                        // Remote closed the connection.
                        break;
                    }
                    Ok(Message::Frame(_)) => {
                        // Internal frame variant – can be ignored.
                    }
                    Err(_e) => {
                        // Error on the WebSocket stream – stop the loop.
                        break;
                    }
                }
            }
        });

        let mut client = Self {
            ws_tx,
            pending,
            next_id,
        };

        // Authenticate immediately
        client
            .authenticate(client_id, client_secret)
            .await
            .map_err(|e| anyhow!("authentication failed: {e}"))?;

        Ok(client)
    }

    async fn authenticate(&mut self, client_id: &str, client_secret: &str) -> Result<()> {
        if client_id.is_empty() || client_secret.is_empty() {
            return Err(anyhow!(
                "CLIENT_ID / CLIENT_SECRET are empty, cannot authenticate"
            ));
        }

        let params = json!({
            "grant_type": "client_credentials",
            "client_id": client_id,
            "client_secret": client_secret,
        });

        let resp = self.send_rpc("public/auth", params).await?;
        if resp.error.is_some() {
            return Err(anyhow!("auth error: {:?}", resp.error));
        }
        Ok(())
    }

    pub async fn send_rpc(&mut self, method: &str, params: Value) -> Result<RpcResponse> {
        let mut id_guard = self.next_id.lock().await;
        let id = *id_guard;
        *id_guard += 1;
        drop(id_guard);

        let (tx, rx) = oneshot::channel::<RpcResponse>();

        {
            let mut pending_guard = self.pending.lock().await;
            pending_guard.insert(id, tx);
        }

        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let txt = serde_json::to_string(&req)?;
        self.ws_tx.send(Message::Text(txt)).await?;

        let resp = rx.await?;
        Ok(resp)
    }
}

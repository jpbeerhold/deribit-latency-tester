mod config;
mod deribit_client;
mod latency;
mod summary;

use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use chrono::Utc;
use serde_json::json;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::time::sleep;

use crate::config::{Config, OrderSide};
use crate::deribit_client::{DeribitClient, MarketDataEvent, RpcResponse};
use crate::latency::{LatencyLogger, SampleContext};

#[tokio::main]
async fn main() -> Result<()> {
    // Load configuration from config.toml in the current working directory
    let cfg = Config::load_from_file("config.toml")?;

    let program_start = Instant::now();

    println!(
        "[{}] Starting Deribit latency tester (instrument={}, testnet={})",
        Utc::now().to_rfc3339(),
        cfg.instrument_name,
        cfg.testnet
    );
    println!(
        "[{}] Latency samples will be written to {}",
        Utc::now().to_rfc3339(),
        cfg.output_latency_csv
    );

    // Channel for market data events (book.<instrument>.raw)
    let (md_tx, mut md_rx) = mpsc::unbounded_channel::<MarketDataEvent>();

    // Shared monotonic timestamp of latest market data tick (ns since program_start)
    let last_tick_ns = Arc::new(RwLock::new(None::<i64>));

    // Connect Deribit WebSocket client (this also authenticates)
    let mut client =
        DeribitClient::connect(cfg.testnet, &cfg.client_id, &cfg.client_secret, md_tx).await?;

    println!("[{}] Connected and authenticated.", Utc::now().to_rfc3339());

    // Spawn a task to keep track of latest MD tick timestamps
    {
        let last_tick_ns_clone = Arc::clone(&last_tick_ns);
        let program_start_clone = program_start;
        tokio::spawn(async move {
            while let Some(evt) = md_rx.recv().await {
                // Only consider book.<instrument>.raw events
                if !evt.channel.starts_with("book.") {
                    continue;
                }
                let mono_ns = evt
                    .recv_ts_mono
                    .duration_since(program_start_clone)
                    .as_nanos() as i64;
                let mut guard = last_tick_ns_clone.write().await;
                *guard = Some(mono_ns);
            }
        });
    }

    // Subscribe to raw order book for real MD timestamps
    if cfg.subscribe_raw_book {
        let channel = format!("book.{}.raw", cfg.instrument_name);
        println!(
            "[{}] Subscribing to {} ...",
            Utc::now().to_rfc3339(),
            channel
        );
        let params = json!({
            "channels": [channel]
        });
        let resp = client.send_rpc("public/subscribe", params).await?;
        if resp.error.is_some() {
            eprintln!("Subscribe error: {:?}", resp.error);
        } else {
            println!("[{}] Subscription successful.", Utc::now().to_rfc3339());
        }
    }

    // Optional: get instrument info (e.g. tick_size)
    let tick_size = fetch_tick_size(&mut client, &cfg.instrument_name).await?;
    println!(
        "[{}] Instrument {} tick_size={}",
        Utc::now().to_rfc3339(),
        cfg.instrument_name,
        tick_size
    );

    // Optional: get a reference price (ticker)
    let base_price = match fetch_ticker_price(&mut client, &cfg.instrument_name).await {
        Ok(p) => p,
        Err(_) => cfg.base_price,
    };
    println!(
        "[{}] Using base price ~{} for order placement",
        Utc::now().to_rfc3339(),
        base_price
    );

    // Prepare latency logger
    let mut logger = LatencyLogger::new(&cfg.output_latency_csv, program_start)?;
    println!(
        "[{}] Writing latency samples to {}",
        Utc::now().to_rfc3339(),
        cfg.output_latency_csv
    );

    // Shared state for order id
    let order_id_state = Arc::new(Mutex::new(None::<String>));

    run_roundtrip_test(
        &mut client,
        &cfg,
        tick_size,
        base_price,
        &last_tick_ns,
        &mut logger,
        &order_id_state,
    )
    .await?;

    if cfg.print_summary {
        if let Err(e) = summary::print_summary_from_csv(&cfg.output_latency_csv) {
            eprintln!("Failed to print summary: {e}");
        }
    }

    println!("[{}] Done.", Utc::now().to_rfc3339());
    Ok(())
}

async fn fetch_tick_size(client: &mut DeribitClient, instrument: &str) -> Result<f64> {
    let params = serde_json::json!({ "instrument_name": instrument });
    let resp = client.send_rpc("public/get_instrument", params).await?;
    if let Some(err) = resp.error {
        return Err(anyhow!("get_instrument error: {:?}", err));
    }
    let tick_size = resp
        .result
        .as_ref()
        .and_then(|r| r.get("tick_size"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.5);
    Ok(tick_size)
}

async fn fetch_ticker_price(client: &mut DeribitClient, instrument: &str) -> Result<f64> {
    let params = serde_json::json!({ "instrument_name": instrument });
    let resp = client.send_rpc("public/ticker", params).await?;
    if let Some(err) = resp.error {
        return Err(anyhow!("ticker error: {:?}", err));
    }
    let price = resp
        .result
        .as_ref()
        .and_then(|r| r.get("mark_price").or_else(|| r.get("last_price")))
        .and_then(|v| v.as_f64())
        .ok_or_else(|| anyhow!("no mark_price / last_price in ticker"))?;
    Ok(price)
}

fn quantize_price(price: f64, tick_size: f64) -> f64 {
    if tick_size <= 0.0 {
        return price;
    }
    let steps = (price / tick_size).round();
    steps * tick_size
}

/// Run a sequence of (side + edit + cancel) iterations and log all latencies.
async fn run_roundtrip_test(
    client: &mut DeribitClient,
    cfg: &Config,
    tick_size: f64,
    base_price: f64,
    last_tick_ns: &Arc<RwLock<Option<i64>>>,
    logger: &mut LatencyLogger,
    order_id_state: &Arc<Mutex<Option<String>>>,
) -> Result<()> {
    for i in 0..cfg.num_iterations {
        let iteration_start = Utc::now().to_rfc3339();
        println!(
            "[{}] Iteration {}/{}",
            iteration_start,
            i + 1,
            cfg.num_iterations
        );

        // --- NEW ORDER ---
        let open_price_raw = base_price * (1.0 + cfg.price_offset_percent / 100.0); // Offset price relative to the base price
        let open_price = quantize_price(open_price_raw, tick_size);

        // Decide side and RPC method based on configuration
        let (open_op_type, open_method) = match cfg.side {
            OrderSide::Buy => ("buy", "private/buy"),
            OrderSide::Sell => ("sell", "private/sell"),
        };

        let open_params = json!({
            "instrument_name": cfg.instrument_name,
            "amount": cfg.order_amount,
            "type": "limit",
            "price": open_price,
            "post_only": true,
        });

        let open_resp = timed_rpc(
            client,
            open_op_type,
            open_method,
            &cfg.instrument_name,
            None,
            last_tick_ns,
            logger,
            open_params,
        )
        .await?;

        // Extract order_id if present
        if let Some(result) = open_resp.result.as_ref() {
            if let Some(order) = result.get("order") {
                if let Some(oid) = order.get("order_id").and_then(|v| v.as_str()) {
                    let mut guard = order_id_state.lock().await;
                    *guard = Some(oid.to_string());
                }
            }
        }

        sleep(cfg.sleep_between_requests).await;

        // --- EDIT ORDER (private/edit) ---
        let maybe_order_id = { order_id_state.lock().await.clone() };

        if let Some(ref order_id) = maybe_order_id {
            // Move the quote further away from the market on each edit
            // For buys: more negative offset (further below the market)
            // For sells: more positive offset (further above the market)
            let edit_offset_percent = match cfg.side {
                OrderSide::Buy => cfg.price_offset_percent - cfg.edit_offset_step_percent,
                OrderSide::Sell => cfg.price_offset_percent + cfg.edit_offset_step_percent,
            };

            let new_price_raw = base_price * (1.0 + edit_offset_percent / 100.0); // Small tweak
            let new_price = quantize_price(new_price_raw, tick_size);

            let edit_params = json!({
                "order_id": order_id,
                "amount": cfg.order_amount,
                "price": new_price,
            });

            let _edit_resp = timed_rpc(
                client,
                "edit",
                "private/edit",
                &cfg.instrument_name,
                Some(order_id.as_str()),
                last_tick_ns,
                logger,
                edit_params,
            )
            .await?;

            sleep(cfg.sleep_between_requests).await;

            // --- CANCEL ORDER (private/cancel) ---
            let cancel_params = json!({
                "order_id": order_id,
            });

            let _cancel_resp = timed_rpc(
                client,
                "cancel",
                "private/cancel",
                &cfg.instrument_name,
                Some(order_id.as_str()),
                last_tick_ns,
                logger,
                cancel_params,
            )
            .await?;

            // Clear order id state after cancel
            {
                let mut guard = order_id_state.lock().await;
                *guard = None;
            }
        } else {
            println!(
                "[{}] No active order_id to edit/cancel.",
                Utc::now().to_rfc3339()
            );
        }

        sleep(cfg.sleep_between_requests).await;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn timed_rpc(
    client: &mut DeribitClient,
    op_type: &str,
    rpc_method: &str,
    instrument_name: &str,
    order_id: Option<&str>,
    last_tick_ns: &Arc<RwLock<Option<i64>>>,
    logger: &mut LatencyLogger,
    params: serde_json::Value,
) -> Result<RpcResponse> {
    let tick_ts_mono_ns_opt = {
        let guard = last_tick_ns.read().await;
        *guard
    };

    let send_ts_wall = Utc::now();
    let send_ts_mono = Instant::now();

    let resp = client.send_rpc(rpc_method, params).await?;

    let sample_ctx = SampleContext {
        op_type,
        rpc_method,
        instrument_name,
        order_id,
        tick_ts_mono_ns: tick_ts_mono_ns_opt,
        send_ts_mono,
        send_ts_wall,
        resp: &resp,
    };

    logger.log_sample(sample_ctx)?;

    Ok(resp)
}

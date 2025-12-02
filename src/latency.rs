use std::fs::{create_dir_all, File};
use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use chrono::{DateTime, Utc};
use csv::Writer;
use serde::Serialize;

use crate::deribit_client::RpcResponse;

/// One latency sample for a single RPC request/response.
#[derive(Debug, Serialize)]
pub struct RoundtripSample {
    pub op_type: String,
    pub rpc_method: String,
    pub instrument_name: String,
    pub order_id: Option<String>,

    pub tick_ts_mono_ns: Option<i64>,
    pub send_ts_mono_ns: i64,
    pub recv_ts_mono_ns: i64,

    pub send_ts_wall_iso: String,
    pub recv_ts_wall_iso: String,

    pub rtt_mono_us: i64,
    pub rtt_wall_us: i64,

    pub tick_to_send_us: Option<i64>,
    pub tick_to_ack_us: Option<i64>,

    pub engine_us_in: Option<i64>,
    pub engine_us_out: Option<i64>,
    pub engine_us_diff: Option<i64>,

    pub error_code: Option<i64>,
    pub error_msg: Option<String>,

    /// Time between this Ack and the previous Ack (monotonic), in microseconds.
    pub ack_delta_prev_us: Option<i64>,
}

/// Helper to write latency samples to CSV and track previous Ack timestamp.
pub struct LatencyLogger {
    writer: Writer<File>,
    program_start: Instant,
    last_ack_recv_ns: Option<i64>,
}

/// Context for logging a single latency sample.
pub struct SampleContext<'a> {
    pub op_type: &'a str,
    pub rpc_method: &'a str,
    pub instrument_name: &'a str,
    pub order_id: Option<&'a str>,
    pub tick_ts_mono_ns: Option<i64>,
    pub send_ts_mono: Instant,
    pub send_ts_wall: DateTime<Utc>,
    pub resp: &'a RpcResponse,
}

impl LatencyLogger {
    pub fn new(csv_path: &str, program_start: Instant) -> Result<Self> {
        if let Some(parent) = Path::new(csv_path).parent() {
            if !parent.as_os_str().is_empty() {
                create_dir_all(parent)?;
            }
        }

        let file = File::create(csv_path)?;
        let writer = Writer::from_writer(file);
        Ok(Self {
            writer,
            program_start,
            last_ack_recv_ns: None,
        })
    }

    fn instant_to_ns_since_start(&self, t: Instant) -> i64 {
        let dur = t.duration_since(self.program_start);
        dur.as_nanos() as i64
    }

    fn wall_to_iso(&self, t: DateTime<Utc>) -> String {
        t.to_rfc3339()
    }

    fn duration_us(from: Instant, to: Instant) -> i64 {
        let dur = to.duration_since(from);
        dur.as_micros() as i64
    }

    fn duration_wall_us(from: DateTime<Utc>, to: DateTime<Utc>) -> i64 {
        let dur = to.signed_duration_since(from);
        dur.num_microseconds().unwrap_or(0)
    }

    fn extract_engine_timestamps(resp: &RpcResponse) -> (Option<i64>, Option<i64>, Option<i64>) {
        let raw = &resp.raw;
        let us_in = raw
            .get("usIn")
            .and_then(|v| v.as_i64())
            .or_else(|| raw.get("usIn").and_then(|v| v.as_f64()).map(|f| f as i64));
        let us_out = raw
            .get("usOut")
            .and_then(|v| v.as_i64())
            .or_else(|| raw.get("usOut").and_then(|v| v.as_f64()).map(|f| f as i64));
        let us_diff = raw
            .get("usDiff")
            .and_then(|v| v.as_i64())
            .or_else(|| raw.get("usDiff").and_then(|v| v.as_f64()).map(|f| f as i64));
        (us_in, us_out, us_diff)
    }

    fn extract_error(resp: &RpcResponse) -> (Option<i64>, Option<String>) {
        if let Some(err) = resp.error.as_ref() {
            let code = err
                .get("code")
                .and_then(|v| v.as_i64())
                .or_else(|| err.get("code").and_then(|v| v.as_f64()).map(|f| f as i64));
            let msg = err
                .get("message")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            (code, msg)
        } else {
            (None, None)
        }
    }

    /// Log a single RPC roundtrip as one CSV row.
    pub fn log_sample(&mut self, ctx: SampleContext<'_>) -> Result<()> {
        let SampleContext {
            op_type,
            rpc_method,
            instrument_name,
            order_id,
            tick_ts_mono_ns,
            send_ts_mono,
            send_ts_wall,
            resp,
        } = ctx;

        let recv_ts_mono = resp.recv_ts_mono;
        let recv_ts_wall = resp.recv_ts_wall;

        let send_mono_ns = self.instant_to_ns_since_start(send_ts_mono);
        let recv_mono_ns = self.instant_to_ns_since_start(recv_ts_mono);

        let rtt_mono_us = Self::duration_us(send_ts_mono, recv_ts_mono);
        let rtt_wall_us = Self::duration_wall_us(send_ts_wall, recv_ts_wall);

        let tick_to_send_us = tick_ts_mono_ns.map(|tick_ns| {
            let send_ns = send_mono_ns;
            ((send_ns - tick_ns) as f64 / 1000.0).round() as i64
        });

        let tick_to_ack_us = tick_ts_mono_ns.map(|tick_ns| {
            let ack_ns = recv_mono_ns;
            ((ack_ns - tick_ns) as f64 / 1000.0).round() as i64
        });

        let (engine_us_in, engine_us_out, engine_us_diff) = Self::extract_engine_timestamps(resp);
        let (error_code, error_msg) = Self::extract_error(resp);

        let ack_delta_prev_us = if let Some(last_ns) = self.last_ack_recv_ns {
            let delta_ns = recv_mono_ns - last_ns;
            Some(((delta_ns as f64) / 1000.0).round() as i64)
        } else {
            None
        };
        self.last_ack_recv_ns = Some(recv_mono_ns);

        let sample = RoundtripSample {
            op_type: op_type.to_string(),
            rpc_method: rpc_method.to_string(),
            instrument_name: instrument_name.to_string(),
            order_id: order_id.map(|s| s.to_string()),
            tick_ts_mono_ns,
            send_ts_mono_ns: send_mono_ns,
            recv_ts_mono_ns: recv_mono_ns,
            send_ts_wall_iso: self.wall_to_iso(send_ts_wall),
            recv_ts_wall_iso: self.wall_to_iso(recv_ts_wall),
            rtt_mono_us,
            rtt_wall_us,
            tick_to_send_us,
            tick_to_ack_us,
            engine_us_in,
            engine_us_out,
            engine_us_diff,
            error_code,
            error_msg,
            ack_delta_prev_us,
        };

        self.writer.serialize(sample)?;
        self.writer.flush()?;
        Ok(())
    }
}

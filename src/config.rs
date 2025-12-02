use std::fs;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Side of the order to place.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderSide {
    Buy,
    Sell,
}

/// Configuration as defined in `config.toml` (without secrets).
#[derive(Debug, Deserialize)]
pub struct FileConfig {
    pub testnet: bool,
    pub side: OrderSide,
    pub instrument_name: String,
    pub order_amount: f64,
    pub base_price: f64,
    pub price_offset_percent: f64,
    pub edit_offset_step_percent: f64,
    pub num_iterations: usize,
    pub sleep_between_requests_secs: f64,
    pub output_latency_csv: String,
    pub subscribe_raw_book: bool,
    pub print_summary: bool,
}

/// Fully resolved configuration used by the latency tester.
/// Combines values from `config.toml` with credentials from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub testnet: bool,
    pub client_id: String,
    pub client_secret: String,

    pub side: OrderSide,
    pub instrument_name: String,
    pub order_amount: f64,
    pub base_price: f64,
    pub price_offset_percent: f64,
    pub edit_offset_step_percent: f64,

    pub num_iterations: usize,
    pub sleep_between_requests: Duration,

    pub output_latency_csv: String,
    pub subscribe_raw_book: bool,
    pub print_summary: bool,
}

impl Config {
    /// Load configuration from a TOML file and environment variables.
    ///
    /// * Non-secret values are read from `path`.
    /// * `DERIBIT_CLIENT_ID` and `DERIBIT_CLIENT_SECRET` are read from the process environment.
    pub fn load_from_file(path: &str) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file at '{}'", path))?;

        let file_cfg: FileConfig = toml::from_str(&raw)
            .with_context(|| format!("failed to parse config file at '{}'", path))?;

        let client_id = std::env::var("DERIBIT_CLIENT_ID")
            .with_context(|| "DERIBIT_CLIENT_ID environment variable is not set")?;
        let client_secret = std::env::var("DERIBIT_CLIENT_SECRET")
            .with_context(|| "DERIBIT_CLIENT_SECRET environment variable is not set")?;

        if client_id.is_empty() || client_secret.is_empty() {
            anyhow::bail!("Deribit credentials must not be empty");
        }

        Ok(Self {
            testnet: file_cfg.testnet,
            client_id,
            client_secret,
            side: file_cfg.side,
            instrument_name: file_cfg.instrument_name,
            order_amount: file_cfg.order_amount,
            base_price: file_cfg.base_price,
            price_offset_percent: file_cfg.price_offset_percent,
            edit_offset_step_percent: file_cfg.edit_offset_step_percent,
            num_iterations: file_cfg.num_iterations,
            sleep_between_requests: Duration::from_secs_f64(file_cfg.sleep_between_requests_secs),
            output_latency_csv: file_cfg.output_latency_csv,
            subscribe_raw_book: file_cfg.subscribe_raw_book,
            print_summary: file_cfg.print_summary,
        })
    }
}

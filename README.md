# Deribit Latency Tester

[![CI](https://github.com/jpbeerhold/deribit-latency-tester/actions/workflows/ci.yaml/badge.svg)](https://github.com/jpbeerhold/deribit-latency-tester/actions/workflows/ci.yaml)
[![Rust](https://img.shields.io/badge/Rust-stable-blue?logo=rust)]()
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)]()

A lightweight and production‑grade tool for measuring **Deribit WebSocket RPC round‑trip latency**, including:

- Buy / Sell order latency  
- Raw‑book tick‑aligned timestamps  
- Engine processing timestamps (`usIn`, `usOut`, `usDiff`)  
- CSV logging  
- Summary statistics (p50, p90, p99, max)  
- No CLI — everything configured through `config.toml`  
- Credentials provided strictly via environment variables

---

# ✨ Features

- Single configuration file (`config.toml`)
- Zero CLI arguments  
- Async Rust (`tokio`, `tungstenite`)
- Detailed latency samples & summaries
- Supports buy/sell side selection
- Supports edit‑offset stepping (move quotes closer or further away)
- Tick‑aligned latency via raw‑book subscription
- Full Docker + Devcontainer setup
- CI workflow included

---

# 📁 Project Structure

```
.
├── Cargo.toml              # Project manifest: dependencies, metadata, build settings
├── config.toml             # User-defined configuration loaded at runtime
├── README.md               # Project documentation and usage instructions
├── LICENSE                 # License information for the project
├── rustfmt.toml            # Code formatting configuration for Rust
├── .gitignore              # Specifies files Git should ignore
├── docker-compose.yaml     # Docker Compose setup for running multi-service environments
├── .devcontainer/
│   ├── devcontainer.json   # VS Code Dev Container configuration
│   └── dev.Dockerfile      # Dockerfile for the development environment
├── .github/
│   └── workflows/
│       └── ci.yaml         # GitHub Actions workflow for CI
└── src/
    ├── config.rs           # Logic for loading and handling configuration values
    ├── main.rs             # Application entry point
    ├── deribit_client.rs   # Client implementation for interacting with Deribit API
    ├── latency.rs          # Utilities for measuring and processing latency data
    └── summary.rs          # Functions to generate and print application summaries
```

---

# ⚡ Running the Tool

Create `.env`:

```env
DERIBIT_CLIENT_ID=your_client_id
DERIBIT_CLIENT_SECRET=your_client_secret
```

Load it:

```bash
set -a
source .env
set +a
```

Development:

```bash
cargo run
```

Release:

```bash
cargo run --release
```

---

# 📊 Output

Latency samples are written to `output_latency_csv`, for example:

```
output/local_latency.csv
```

Includes:

- RTT (mono + wallclock)
- Tick timestamps
- Engine (`usIn`, `usOut`, `usDiff`)
- Ack deltas
- Error codes & messages

The sample CSV contains only synthetic data. Real trading data and order identifiers are never committed to this repository.

---

# 📈 Summary Statistics

Example:

```
==================== LATENCY SUMMARY ====================
RTT (Send → Ack):
    count:   100   min:  340 µs   median: 620 µs   p90: 900 µs   p99: 1400 µs   max: 1600 µs
...
=========================================================
```

---

# ⚙️ Configuration (`config.toml`)

All runtime behavior is controlled through this file.  
Below is a complete explanation of all parameters.

### `testnet`
**true = Deribit Testnet**, **false = Mainnet**

### `side`
`"buy"` or `"sell"`

### `instrument_name`
Example: `"BTC_USDC-PERPETUAL"`

### `order_amount`
Order quantity.

### `base_price`
Fallback price if Deribit ticker fails.

### `price_offset_percent`
Initial offset relative to the market/base price.

### `edit_offset_step_percent`
How much the edit step changes the offset:

- BUY → `offset = offset - step`  
- SELL → `offset = offset + step`  
- Negative step → edits move quote **closer** to market.

### `num_iterations`
Number of `open → edit → cancel` cycles.

### `sleep_between_requests_secs`
Delay between RPCs to avoid rate limits.

### `output_latency_csv`
Output file path.

### `subscribe_raw_book`
Enable raw‑book subscription for tick‑aligned latency metrics.

### `print_summary`
If true, prints summary at the end.

---

# 🧠 Raw‑Book Subscription & Tick‑Aligned Latency

When `subscribe_raw_book = true`, the tool subscribes to:

```
book.<instrument>.raw
```

This subscription **does not determine when orders are sent**.  
It is used purely for **advanced latency analytics**:

- Every raw‑book tick is timestamped upon arrival.
- These timestamps are stored and associated with each RPC.
- This enables **latency relative to the most recent market‑data tick**, not only RTT.

This produces a more realistic latency profile for trading‑engine proximity testing.

---

# 🧭 Timestamp Precision

The tool uses multiple timestamp sources with different granularities:

### **Monotonic timestamps (nanoseconds)**

Used for all latency measurements:

* RPC send/receive time
* Round-trip latency (RTT)
* Tick-aligned latencies
* Ack-to-ack deltas

These are derived from Rust’s `Instant::now()` and recorded at **nanosecond precision**.

### **Wall-clock timestamps (microseconds)**

Used for human-readable logging:

* RFC3339 timestamps (`UTC`)
* Resolution: **microseconds**

### **Deribit engine timestamps (microseconds)**

Raw values from Deribit:

* `usIn`
* `usOut`
* `usDiff`

All provided at **microsecond resolution**.

---

# ⚠️ Error Handling & Failure Behavior

### WebSocket disconnects  
If the WebSocket connection fails, `send_rpc()` returns an error → the program stops.  
There is **no reconnect logic** (current design).

### RPC errors  
If Deribit returns an error inside the RPC response body, the tool logs it and **continues to the next step**.

### Missing `order_id`  
If an open order does not return `order_id`, edit/cancel is skipped.

### No timeouts  
There is **no explicit timeout** — `send_rpc().await` waits until Deribit responds or the socket errors.

---

# 📄 License

MIT License — see [LICENSE](./LICENSE).
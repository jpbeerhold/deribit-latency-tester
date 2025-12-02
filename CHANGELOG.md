# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.1.0] - 2025-12-01
### Added
- Full async Rust implementation of Deribit latency tester.
- Configuration-driven design using `config.toml` (no CLI).
- Buy/Sell quoting via `side` parameter.
- Price offset system (`price_offset_percent`) and edit-step offset (`edit_offset_step_percent`).
- Complete order lifecycle measurement: open → edit → cancel.
- Tick-aligned latency tracking through raw book subscription.
- Engine timestamps (`usIn`, `usOut`, `usDiff`) included in CSV logs.
- Computed summary statistics (min, median, p90, p99, max).
- Strict environment-only authentication variables and Devcontainer setup.
- Project-ready structure for central usage and benchmarking.
- Detailed README with configuration reference and usage examples.

---

## [Unreleased]
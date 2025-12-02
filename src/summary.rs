use std::fs::File;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Minimal view of the CSV rows for summary calculation.
#[derive(Debug, Deserialize)]
struct SampleRow {
    rtt_mono_us: i64,
    tick_to_send_us: Option<i64>,
    tick_to_ack_us: Option<i64>,
    ack_delta_prev_us: Option<i64>,
}

pub fn print_summary_from_csv(path: &str) -> Result<()> {
    let file = File::open(path).with_context(|| format!("failed to open CSV at '{}'", path))?;
    let mut rdr = csv::Reader::from_reader(file);

    let mut rtts = Vec::new();
    let mut tick_send = Vec::new();
    let mut tick_ack = Vec::new();
    let mut ack_delta = Vec::new();

    for record in rdr.deserialize::<SampleRow>() {
        let row = record?;
        rtts.push(row.rtt_mono_us);
        if let Some(v) = row.tick_to_send_us {
            tick_send.push(v);
        }
        if let Some(v) = row.tick_to_ack_us {
            tick_ack.push(v);
        }
        if let Some(v) = row.ack_delta_prev_us {
            ack_delta.push(v);
        }
    }

    println!();
    println!("==================== LATENCY SUMMARY ====================");

    print_stats("RTT (Send → Ack)", &mut rtts);
    print_stats("Tick → Send", &mut tick_send);
    print_stats("Tick → Ack", &mut tick_ack);
    print_stats("Ack interval (prev Ack → this Ack)", &mut ack_delta);

    println!();
    println!("=========================================================");
    println!();

    Ok(())
}

fn print_stats(label: &str, data: &mut [i64]) {
    println!();
    println!("{label}:");

    if data.is_empty() {
        println!("    no data");
        return;
    }

    data.sort_unstable();
    let n = data.len();

    let min = data[0];
    let max = data[n - 1];
    let median = percentile(data, 50.0);
    let p90 = percentile(data, 90.0);
    let p99 = percentile(data, 99.0);

    println!(
        "    count: {:>6}   min: {:>8} µs   median: {:>8} µs   p90: {:>8} µs   p99: {:>8} µs   max: {:>8} µs",
        n, min, median, p90, p99, max
    );
}

/// Simple percentile helper: p in [0, 100].
fn percentile(sorted: &[i64], p: f64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let n = sorted.len();
    let rank = (p / 100.0) * ((n - 1) as f64);
    let idx = rank.floor() as usize;
    sorted[idx]
}

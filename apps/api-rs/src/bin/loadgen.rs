
//! Compiled closed-loop load generator — identical for the Python and Rust services, so the
//! reported before/after numbers are not coloured by a GIL-bound client.
//!
//!   loadgen <base_url> [--seconds 60] [--concurrency 32] [--paths P ...]

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::stream::{FuturesUnordered, StreamExt};

#[derive(Default)]
struct Stats {
    lat: Vec<f64>,
    transport_err: u64,
    s2xx: u64,
    s3xx: u64,
    s4xx: u64,
    s5xx: u64,
}

async fn worker(
    client: reqwest::Client,
    paths: Arc<Vec<String>>,
    deadline: Instant,
    mut stats: Stats,
    start_idx: usize,
    base: String,
) -> Stats {
    let mut i = start_idx;
    while Instant::now() < deadline {
        let path = &paths[i % paths.len()];
        i += 1;
        let url = format!("{base}{path}");
        let t0 = Instant::now();
        match client.get(&url).send().await {
            Ok(r) => match r.status().as_u16() {
                c if c < 300 => stats.s2xx += 1,
                c if c < 400 => stats.s3xx += 1,
                c if c < 500 => stats.s4xx += 1,
                _ => stats.s5xx += 1,
            },
            Err(_) => stats.transport_err += 1,
        }
        stats.lat.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    stats
}

fn arg(args: &[String], name: &str, default: &str) -> String {
    args.iter()
        .position(|a| a == name)
        .and_then(|p| args.get(p + 1))
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let base = args.get(1).cloned().unwrap_or_else(|| "http://127.0.0.1:8000".into());
    let seconds: f64 = arg(&args, "--seconds", "60").parse()?;
    let concurrency: usize = arg(&args, "--concurrency", "32").parse()?;
    let paths: Vec<String> = {
        match args.iter().position(|a| a == "--paths") {
            Some(p) if args.len() > p + 1 => args[p + 1..].to_vec(),
            _ => vec!["/api/health".to_string()],
        }
    };
    let paths = Arc::new(paths);

    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(concurrency + 4)
        .timeout(Duration::from_secs(30))
        .build()?;

    // Warmup.
    for p in paths.iter() {
        let _ = client.get(format!("{base}{p}")).send().await;
    }

    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    let t0 = Instant::now();
    let mut futs = FuturesUnordered::new();
    for i in 0..concurrency {
        futs.push(worker(client.clone(), paths.clone(), deadline, Stats::default(), i, base.clone()));
    }
    let mut all = Stats::default();
    while let Some(s) = futs.next().await {
        all.lat.extend(s.lat);
        all.transport_err += s.transport_err;
        all.s2xx += s.s2xx;
        all.s3xx += s.s3xx;
        all.s4xx += s.s4xx;
        all.s5xx += s.s5xx;
    }
    let wall = t0.elapsed().as_secs_f64();
    all.lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = all.lat.len();
    let pct = |p: f64| -> f64 {
        if n == 0 { 0.0 } else { all.lat[((p * n as f64) as usize).min(n - 1)] }
    };
    println!("{{");
    println!("  \"base_url\": \"{base}\",");
    println!("  \"seconds\": {wall:.2},");
    println!("  \"concurrency\": {concurrency},");
    println!("  \"requests\": {n},");
    println!("  \"transport_err\": {},", all.transport_err);
    println!("  \"s2xx\": {}, \"s3xx\": {}, \"s4xx\": {}, \"s5xx\": {},", all.s2xx, all.s3xx, all.s4xx, all.s5xx);
    println!("  \"rps\": {:.1},", n as f64 / wall);
    println!("  \"p50_ms\": {:.3},", pct(0.50));
    println!("  \"p95_ms\": {:.3},", pct(0.95));
    println!("  \"p99_ms\": {:.3}", pct(0.99));
    println!("}}");
    Ok(())
}

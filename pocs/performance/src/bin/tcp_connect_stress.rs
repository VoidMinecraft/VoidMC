use std::{
    env,
    net::SocketAddr,
    process,
    time::{Duration, Instant},
};

use tokio::{net::TcpStream, time::timeout};

#[derive(Debug, Clone)]
struct Args {
    addr: SocketAddr,
    clients: usize,
    timeout_ms: u64,
}

#[derive(Debug)]
enum Outcome {
    Connected(Duration),
    Failed,
    TimedOut,
}

#[tokio::main]
async fn main() {
    let args = parse_args().unwrap_or_else(|err| {
        eprintln!("{err}");
        eprintln!(
            "usage: tcp_connect_stress --addr 127.0.0.1:25565 --clients 64 --timeout-ms 1000"
        );
        process::exit(2);
    });

    let total_start = Instant::now();
    let mut tasks = Vec::with_capacity(args.clients);

    for _ in 0..args.clients {
        let addr = args.addr;
        let timeout_ms = args.timeout_ms;
        tasks.push(tokio::spawn(async move {
            let started = Instant::now();
            match timeout(Duration::from_millis(timeout_ms), TcpStream::connect(addr)).await {
                Ok(Ok(stream)) => {
                    drop(stream);
                    Outcome::Connected(started.elapsed())
                }
                Ok(Err(_)) => Outcome::Failed,
                Err(_) => Outcome::TimedOut,
            }
        }));
    }

    let mut latencies = Vec::new();
    let mut failures = 0usize;
    let mut timeouts = 0usize;

    for task in tasks {
        match task.await.expect("stress task should not panic") {
            Outcome::Connected(latency) => latencies.push(latency),
            Outcome::Failed => failures += 1,
            Outcome::TimedOut => timeouts += 1,
        }
    }

    latencies.sort_unstable();
    let successes = latencies.len();
    let elapsed = total_start.elapsed();
    let avg_ms = if successes == 0 {
        0.0
    } else {
        latencies.iter().map(duration_ms).sum::<f64>() / successes as f64
    };

    println!("# TCP connect stress POC");
    println!();
    println!(
        "addr={}, clients={}, timeout_ms={}",
        args.addr, args.clients, args.timeout_ms
    );
    println!();
    println!("| Metric | Value |");
    println!("| --- | ---: |");
    println!("| Connection attempts | {} |", args.clients);
    println!("| Successful connections | {successes} |");
    println!("| Failed connections | {failures} |");
    println!("| Timed out connections | {timeouts} |");
    println!("| Total elapsed | {:.3} ms |", duration_ms(&elapsed));
    println!("| Average latency | {avg_ms:.3} ms |");
    println!(
        "| Minimum latency | {:.3} ms |",
        latencies.first().map(duration_ms).unwrap_or(0.0)
    );
    println!(
        "| Maximum latency | {:.3} ms |",
        latencies.last().map(duration_ms).unwrap_or(0.0)
    );
}

fn duration_ms(duration: &Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn parse_args() -> Result<Args, String> {
    let mut addr = "127.0.0.1:25565".parse().expect("default address is valid");
    let mut clients = 64usize;
    let mut timeout_ms = 1_000u64;
    let mut args = env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--addr" => {
                let value = args.next().ok_or("--addr requires a value")?;
                addr = value
                    .parse()
                    .map_err(|_| format!("invalid socket address: {value}"))?;
            }
            "--clients" => {
                let value = args.next().ok_or("--clients requires a value")?;
                clients = value
                    .parse()
                    .map_err(|_| format!("invalid client count: {value}"))?;
            }
            "--timeout-ms" => {
                let value = args.next().ok_or("--timeout-ms requires a value")?;
                timeout_ms = value
                    .parse()
                    .map_err(|_| format!("invalid timeout: {value}"))?;
            }
            "--help" | "-h" => {
                return Err(String::new());
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    if clients == 0 {
        return Err("--clients must be greater than zero".to_string());
    }

    Ok(Args {
        addr,
        clients,
        timeout_ms,
    })
}

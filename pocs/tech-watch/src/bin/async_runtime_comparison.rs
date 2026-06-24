use std::time::{Duration, Instant};

const CLIENTS: usize = 128;
const PACKETS_PER_CLIENT: usize = 16;
const PACKET_DELAY: Duration = Duration::from_micros(50);

fn main() {
    println!("Async runtime POC");
    println!("clients={CLIENTS}, packets_per_client={PACKETS_PER_CLIENT}");
    println!();

    let tokio_elapsed = run_tokio();
    let async_std_elapsed = run_async_std();
    let smol_elapsed = run_smol();

    println!("| Runtime | Elapsed | Notes |");
    println!("|---|---:|---|");
    println!(
        "| Tokio | {:?} | Selected: mature TCP ecosystem, runtime tooling, docs |",
        tokio_elapsed
    );
    println!(
        "| async-std | {:?} | Plausible alternative, smaller ecosystem for our stack |",
        async_std_elapsed
    );
    println!(
        "| smol | {:?} | Lightweight and elegant, but less aligned with existing server/runtime examples |",
        smol_elapsed
    );
}

fn run_tokio() -> Duration {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .expect("tokio runtime");

    runtime.block_on(async {
        let start = Instant::now();
        let mut tasks = Vec::with_capacity(CLIENTS);

        for client_id in 0..CLIENTS {
            tasks.push(tokio::spawn(async move {
                simulated_client(client_id, tokio_sleep).await
            }));
        }

        for task in tasks {
            task.await.expect("tokio task");
        }

        start.elapsed()
    })
}

fn run_async_std() -> Duration {
    async_std::task::block_on(async {
        let start = Instant::now();
        let mut tasks = Vec::with_capacity(CLIENTS);

        for client_id in 0..CLIENTS {
            tasks.push(async_std::task::spawn(async move {
                simulated_client(client_id, async_std_sleep).await
            }));
        }

        for task in tasks {
            task.await;
        }

        start.elapsed()
    })
}

fn run_smol() -> Duration {
    smol::block_on(async {
        let start = Instant::now();
        let mut tasks = Vec::with_capacity(CLIENTS);

        for client_id in 0..CLIENTS {
            tasks.push(smol::spawn(async move {
                simulated_client(client_id, smol_sleep).await
            }));
        }

        for task in tasks {
            task.await;
        }

        start.elapsed()
    })
}

async fn simulated_client<F, Fut>(client_id: usize, sleep: F) -> usize
where
    F: Fn(Duration) -> Fut,
    Fut: Future<Output = ()>,
{
    let mut checksum = client_id;
    for packet_id in 0..PACKETS_PER_CLIENT {
        sleep(PACKET_DELAY).await;
        checksum ^= packet_id;
    }
    checksum
}

async fn tokio_sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
}

async fn async_std_sleep(duration: Duration) {
    async_std::task::sleep(duration).await;
}

async fn smol_sleep(duration: Duration) {
    smol::Timer::after(duration).await;
}

//! Runs a named workload's smoke checks and load clients against an
//! in-process server or an operator-supplied endpoint, prints the summary,
//! optionally writes the JSON report, and exits 0 on PASS, 1 on FAIL and 2
//! on a usage error. See `void-docs/docs/performance/load-testing.md`.

use std::process::ExitCode;

use voidmc_e2e::load::{Target, cli, run};

fn main() -> ExitCode {
    // Named so that the in-process server's resource accounting, which goes
    // by thread name, does not count the load clients.
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("load-client")
        .build()
        .expect("failed to start the Tokio runtime")
        .block_on(run_cli())
}

async fn run_cli() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match cli::parse(&args, std::env::var(cli::ENDPOINT_VAR).ok()) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    if matches!(cli.target, Target::Endpoint(_)) {
        // An in-process run installs the harness's log capture instead,
        // filtered by VOID_E2E_LOG too.
        let filter = tracing_subscriber::EnvFilter::try_from_env("VOID_E2E_LOG")
            .unwrap_or_else(|_| "info".into());
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }

    let report = match run(&cli.workload, cli.target).await {
        Ok(report) => report,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    if let Some(path) = &cli.report
        && let Err(error) = report.write(path)
    {
        eprintln!("could not write the report to {}: {error}", path.display());
        return ExitCode::from(2);
    }
    println!("{}", report.summary());
    if report.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

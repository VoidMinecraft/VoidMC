use std::net::SocketAddr;
use std::process::ExitCode;
use std::time::Duration;

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
use voidmc_keeper::proxy::{Events, run};
use voidmc_keeper::session::Settings;

const USAGE: &str = "\
voidmc-keeper: keeps a client in game while the server restarts or crashes

  voidmc-keeper --backend ADDRESS [--listen ADDRESS] [--return-timeout SECONDS] [--settle SECONDS]

  --backend          the server, as [::1]:25671
  --listen           where clients connect (default 127.0.0.1:25565)
  --return-timeout   how long a client waits for a lost server (default 120)
  --settle           how long a new server has before old chunks and packs go (default 3)

Events go to stdout, one JSON object per line; logs go to stderr.";

#[derive(Debug)]
struct Options {
    listen: SocketAddr,
    backend: SocketAddr,
    settings: Settings,
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut listen = SocketAddr::from(([127, 0, 0, 1], 25565));
    let mut backend = None;
    let mut settings = Settings::default();
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or(format!("{flag} needs a value"));
        let seconds = |text: String| {
            text.parse::<u64>()
                .map(Duration::from_secs)
                .map_err(|_| format!("{text} is not a number of seconds"))
        };
        let address = |text: String| {
            text.parse::<SocketAddr>()
                .map_err(|_| format!("{text} is not an address"))
        };
        match flag.as_str() {
            "--listen" => listen = address(value()?)?,
            "--backend" => backend = Some(address(value()?)?),
            "--return-timeout" => settings.return_timeout = seconds(value()?)?,
            "--settle" => settings.settle = seconds(value()?)?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(Options {
        listen,
        backend: backend.ok_or("--backend is required")?,
        settings,
    })
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let options = match parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let listener = match TcpListener::bind(options.listen).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("cannot listen on {}: {error}", options.listen);
            return ExitCode::FAILURE;
        }
    };
    tokio::select! {
        result = run(listener, options.backend, options.settings, Events::stdout()) => {
            if let Err(error) = result {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        _ = tokio::signal::ctrl_c() => ExitCode::SUCCESS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> impl Iterator<Item = String> {
        line.split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn options_are_read_with_their_defaults() {
        let options = parse(args("--backend [::1]:25671 --return-timeout 9")).unwrap();
        assert_eq!(options.backend, "[::1]:25671".parse().unwrap());
        assert_eq!(options.settings.return_timeout, Duration::from_secs(9));
        assert_eq!(options.listen.port(), 25565);
    }

    #[test]
    fn a_backend_is_required_and_bad_values_are_named() {
        assert!(parse(args("")).unwrap_err().contains("--backend"));
        assert!(
            parse(args("--backend nowhere"))
                .unwrap_err()
                .contains("nowhere")
        );
        assert!(parse(args("--listen")).unwrap_err().contains("--listen"));
    }
}

use std::path::PathBuf;

use super::runner::Target;
use super::workload::Workload;

/// The environment variable that supplies the operator endpoint when
/// `--endpoint` is not given.
pub const ENDPOINT_VAR: &str = "VOIDMC_LOAD_ENDPOINT";

pub const USAGE: &str = "\
Usage: voidmc-load [--workload smoke|standard] [--target in-process|endpoint]
                   [--endpoint HOST:PORT] [--report PATH]

Runs the functional smoke checks, then the load clients, and judges the run
against the workload's thresholds. Exits 0 on PASS, 1 on FAIL, 2 on a usage
error.

  --workload   the named workload to run (default: smoke)
  --target     in-process: start a server in this process on 127.0.0.1 with an
               ephemeral port (default)
               endpoint: connect to the server at --endpoint, or at
               $VOIDMC_LOAD_ENDPOINT; there is no default address
  --endpoint   HOST:PORT of the operator's server; only with --target endpoint
  --report     where to write the JSON report; the Markdown summary is written
               next to it with the .md extension. The summary is always
               printed to standard output.";

#[derive(Debug)]
pub struct Cli {
    pub workload: Workload,
    pub target: Target,
    pub report: Option<PathBuf>,
}

/// Parses the arguments (without the program name). `endpoint_var` is the
/// value of [`ENDPOINT_VAR`], if set.
pub fn parse(args: &[String], endpoint_var: Option<String>) -> Result<Cli, String> {
    let mut workload = Workload::smoke();
    let mut target_kind = None;
    let mut endpoint = None;
    let mut report = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| {
            args.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "--workload" => {
                let name = value("--workload")?;
                workload = Workload::by_name(&name).ok_or_else(|| {
                    let known: Vec<_> = Workload::all().iter().map(|w| w.name).collect();
                    format!("unknown workload {name:?}; known: {}", known.join(", "))
                })?;
            }
            "--target" => target_kind = Some(value("--target")?),
            "--endpoint" => endpoint = Some(value("--endpoint")?),
            "--report" => report = Some(PathBuf::from(value("--report")?)),
            "-h" | "--help" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument {other:?}")),
        }
    }

    let target = match target_kind.as_deref().unwrap_or("in-process") {
        "in-process" => {
            if endpoint.is_some() {
                return Err("--endpoint needs --target endpoint".to_string());
            }
            Target::InProcess
        }
        "endpoint" => {
            let endpoint = endpoint
                .or(endpoint_var)
                .map(|endpoint| endpoint.trim().to_string())
                .filter(|endpoint| !endpoint.is_empty())
                .ok_or_else(|| {
                    format!(
                        "--target endpoint needs the server address: pass --endpoint HOST:PORT \
                         or set {ENDPOINT_VAR}; there is no default"
                    )
                })?;
            Target::Endpoint(endpoint)
        }
        other => {
            return Err(format!(
                "unknown target {other:?}; use in-process or endpoint"
            ));
        }
    };
    Ok(Cli {
        workload,
        target,
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn defaults_to_the_smoke_workload_in_process() {
        let cli = parse(&[], Some("staging.invalid:25565".into())).unwrap();
        assert_eq!(cli.workload.id(), "smoke@1");
        assert_eq!(cli.target, Target::InProcess);
        assert_eq!(cli.report, None);
    }

    #[test]
    fn endpoint_target_refuses_to_run_without_an_address() {
        let error = parse(&args(&["--target", "endpoint"]), None).unwrap_err();
        assert!(error.contains("there is no default"), "{error}");
        let error = parse(&args(&["--target", "endpoint"]), Some("  ".into())).unwrap_err();
        assert!(error.contains("there is no default"), "{error}");
    }

    #[test]
    fn endpoint_comes_from_the_flag_then_the_environment() {
        let cli = parse(
            &args(&[
                "--target",
                "endpoint",
                "--endpoint",
                "staging.invalid:25570",
            ]),
            Some("other:1".into()),
        )
        .unwrap();
        assert_eq!(cli.target, Target::Endpoint("staging.invalid:25570".into()));
        let cli = parse(
            &args(&["--target", "endpoint", "--workload", "standard"]),
            Some("staging.invalid:25565".into()),
        )
        .unwrap();
        assert_eq!(cli.target, Target::Endpoint("staging.invalid:25565".into()));
        assert_eq!(cli.workload.id(), "standard@1");
    }

    #[test]
    fn an_endpoint_without_the_endpoint_target_is_rejected() {
        let error = parse(&args(&["--endpoint", "staging.invalid:25565"]), None).unwrap_err();
        assert!(error.contains("--target endpoint"), "{error}");
    }

    #[test]
    fn unknown_workloads_and_targets_are_rejected() {
        assert!(parse(&args(&["--workload", "huge"]), None).is_err());
        assert!(parse(&args(&["--target", "public"]), None).is_err());
        assert!(parse(&args(&["--report"]), None).is_err());
    }
}

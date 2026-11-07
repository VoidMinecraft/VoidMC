use serde_json::Value;

const RUST: [(&str, &str); 4] = [
    ("bot.rs", include_str!("bot.rs")),
    ("condition.rs", include_str!("condition.rs")),
    ("frame.rs", include_str!("frame.rs")),
    ("launch.rs", include_str!("launch.rs")),
];

fn without_tests(source: &str) -> &str {
    source
        .split_once("\n#[cfg(test)]\nmod tests")
        .map_or(source, |(code, _)| code)
}

pub fn current() -> String {
    format!("{:016x}", of_probe(&crate::launch::PROBE_SOURCES, &RUST))
}

pub fn probe_build() -> String {
    format!("{:016x}", of_build(&crate::launch::PROBE_SOURCES))
}

fn of_probe(probe: &[(&str, &str)], rust: &[(&str, &str)]) -> u64 {
    let code: Vec<&str> = probe
        .iter()
        .map(|(_, source)| *source)
        .chain(rust.iter().map(|(_, source)| without_tests(source)))
        .collect();
    of(&code)
}

fn of_build(probe: &[(&str, &str)]) -> u64 {
    let code: Vec<&str> = probe.iter().map(|(_, source)| *source).collect();
    of(&code)
}

fn of(sources: &[&str]) -> u64 {
    sources
        .iter()
        .flat_map(|source| source.bytes().chain([0]))
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
}

pub fn check(name: &str, state: &Value) -> Result<(), String> {
    let running = state["version"].as_str().unwrap_or("none");
    if running == current() {
        return Ok(());
    }
    Err(format!(
        "{name} runs probe version {running} and this voidc has {}: restart it with voidc down {name}, then voidc up",
        current()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_client_of_another_version_is_refused_with_how_to_restart_it() {
        assert!(check("hot-reload", &json!({"version": current()})).is_ok());
        let old = check("hot-reload", &json!({"version": "0000000000000000"})).unwrap_err();
        assert!(old.contains("voidc down hot-reload"), "{old}");
        assert!(check("hot-reload", &json!({"state": "connected"})).is_err());
    }

    #[test]
    fn a_change_to_tests_alone_keeps_the_version() {
        let code = "fn a() {}\n";
        let tested = format!("{code}\n#[cfg(test)]\nmod tests {{ fn t() {{}} }}");
        let retested = format!("{code}\n#[cfg(test)]\nmod tests {{ fn u() {{}} }}");
        assert_eq!(without_tests(&tested), without_tests(&retested));
        assert!(
            RUST.iter()
                .all(|(_, source)| source.contains("\n#[cfg(test)]\nmod tests"))
        );
    }

    #[test]
    fn any_change_to_a_source_is_a_new_version() {
        assert_ne!(of(&["a", "b"]), of(&["a", "c"]));
        assert_ne!(of(&["ab", ""]), of(&["a", "b"]));
        let probe = crate::launch::PROBE_SOURCES;
        assert_eq!(current(), format!("{:016x}", of_probe(&probe, &RUST)));
        assert_eq!(probe_build(), format!("{:016x}", of_build(&probe)));
        let sources: Vec<&str> = probe.iter().map(|(_, source)| *source).collect();
        assert_eq!(of_build(&probe), of(&sources));
        for changed in 0..probe.len() {
            let mut edited = probe;
            edited[changed].1 = "";
            let name = probe[changed].0;
            assert_ne!(of_probe(&edited, &RUST), of_probe(&probe, &RUST), "{name}");
            assert_ne!(of_build(&edited), of_build(&probe), "{name}");
        }
        let names: Vec<&str> = RUST.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, ["bot.rs", "condition.rs", "frame.rs", "launch.rs"]);
        for changed in 0..RUST.len() {
            let mut edited = RUST;
            edited[changed].1 = "";
            assert_ne!(
                of_probe(&probe, &edited),
                of_probe(&probe, &RUST),
                "{changed}"
            );
            let retested = format!(
                "{}\n#[cfg(test)]\nmod tests {{ fn other() {{}} }}",
                without_tests(RUST[changed].1)
            );
            let mut tests_only = RUST;
            tests_only[changed].1 = &retested;
            assert_eq!(
                of_probe(&probe, &tests_only),
                of_probe(&probe, &RUST),
                "{changed}"
            );
        }
    }
}

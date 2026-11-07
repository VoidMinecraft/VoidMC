use serde_json::Value;

const SOURCES: [&str; 5] = [
    include_str!("../probe/Probe.java"),
    include_str!("../probe/HideAgent.java"),
    include_str!("../probe/Condition.java"),
    include_str!("bot.rs"),
    include_str!("condition.rs"),
];

/// Taken from the sources of the probe and of the bot themselves, so it cannot be forgotten:
/// any change to them is a new version.
pub fn current() -> String {
    format!("{:016x}", of(&SOURCES))
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
    fn any_change_to_a_source_is_a_new_version() {
        assert_ne!(of(&["a", "b"]), of(&["a", "c"]));
        assert_ne!(of(&["ab", ""]), of(&["a", "b"]));
    }
}

use std::collections::{BTreeSet, HashMap};
use std::str::FromStr;

/// The word after `name`, wherever it stands.
pub fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|at| args.get(at + 1))
        .cloned()
}

pub fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

/// Every word after `name`, in order, for an option that may come back.
pub fn all(args: &[String], name: &str) -> Vec<String> {
    args.windows(2)
        .filter(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
        .collect()
}

pub fn value<T: FromStr>(args: &[String], name: &str, default: T) -> Result<T, String> {
    match flag(args, name) {
        Some(text) => text.parse().map_err(|_| format!("bad {name}")),
        None => Ok(default),
    }
}

/// A command line refused as a whole if any of it is not understood.
#[derive(Debug, Default)]
pub struct Arguments {
    pub files: Vec<String>,
    values: HashMap<String, String>,
    switches: BTreeSet<String>,
}

impl Arguments {
    pub fn parse(
        args: &[String],
        takes_a_value: &[&str],
        switches: &[&str],
        usage: &str,
    ) -> Result<Self, String> {
        let mut parsed = Self::default();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            if takes_a_value.contains(&arg.as_str()) {
                let value = args
                    .next()
                    .filter(|value| !value.starts_with("--"))
                    .ok_or_else(|| format!("{arg} takes a value"))?;
                parsed.values.insert(arg.clone(), value.clone());
            } else if switches.contains(&arg.as_str()) {
                parsed.switches.insert(arg.clone());
            } else if arg.starts_with("--") {
                return Err(format!("unknown option {arg}\n{usage}"));
            } else {
                parsed.files.push(arg.clone());
            }
        }
        Ok(parsed)
    }

    pub fn value(&self, name: &str) -> Option<String> {
        self.values.get(name).cloned()
    }

    pub fn number<T: FromStr>(&self, name: &str) -> Result<Option<T>, String> {
        self.value(name)
            .map(|value| value.parse().map_err(|_| format!("{name} takes a number")))
            .transpose()
    }

    pub fn has(&self, name: &str) -> bool {
        self.switches.contains(name)
    }
}

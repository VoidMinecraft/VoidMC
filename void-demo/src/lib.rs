pub mod arena;
pub mod audio;
pub mod chat;
pub mod displays;
pub mod items;
pub mod kart;
pub mod race;
pub mod sidebar;
pub mod terrain;
pub mod track;
pub mod travel;
pub mod vehicle;

pub const SEED_VAR: &str = "VOID_DEMO_SEED";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seed {
    Configured(u64),
    Random(u64),
}

impl Seed {
    pub fn value(self) -> u64 {
        match self {
            Seed::Configured(seed) | Seed::Random(seed) => seed,
        }
    }
}

pub fn seed(configured: Option<&str>) -> Result<Seed, std::num::ParseIntError> {
    match configured {
        Some(value) => value.trim().parse().map(Seed::Configured),
        None => {
            let mut seed = 0;
            while seed == 0 {
                seed = rand::random::<u64>();
            }
            Ok(Seed::Random(seed))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configured_seed_is_used_verbatim() {
        assert_eq!(seed(Some("2026")), Ok(Seed::Configured(2026)));
        assert_eq!(seed(Some(" 42 ")), Ok(Seed::Configured(42)));
        assert_eq!(
            seed(Some("18446744073709551615")),
            Ok(Seed::Configured(u64::MAX))
        );
        assert!(seed(Some("abc")).is_err());
        assert!(seed(Some("-1")).is_err());
    }

    #[test]
    fn an_unset_seed_is_random_and_never_zero() {
        let first = seed(None).unwrap();
        let second = seed(None).unwrap();
        assert!(matches!(first, Seed::Random(value) if value != 0));
        assert!(matches!(second, Seed::Random(value) if value != 0));
        assert_ne!(first, second);
    }
}

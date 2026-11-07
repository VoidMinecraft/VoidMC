//! Where the UUIDs the server makes up come from: random, unless a test run asks otherwise.
//!
//! With the `seeded-uuids` feature, `VOID_UUID_SEED=<u64>` makes the n-th UUID of a run a
//! function of the seed and n. Two runs get the same UUIDs as long as they ask in the same
//! order, which systems running in parallel do not promise.

use uuid::Uuid;

#[cfg(not(feature = "seeded-uuids"))]
pub fn random_uuid() -> Uuid {
    Uuid::new_v4()
}

#[cfg(feature = "seeded-uuids")]
pub fn random_uuid() -> Uuid {
    use std::sync::LazyLock;

    static SOURCE: LazyLock<Option<SeededUuids>> = LazyLock::new(|| {
        let seed = std::env::var("VOID_UUID_SEED").ok()?;
        match seed.parse() {
            Ok(seed) => Some(SeededUuids::new(seed)),
            Err(_) => panic!("VOID_UUID_SEED is not a number: {seed}"),
        }
    });
    SOURCE.as_ref().map_or_else(Uuid::new_v4, SeededUuids::next)
}

#[cfg(any(test, feature = "seeded-uuids"))]
struct SeededUuids {
    seed: u64,
    drawn: std::sync::atomic::AtomicU64,
}

#[cfg(any(test, feature = "seeded-uuids"))]
impl SeededUuids {
    fn new(seed: u64) -> Self {
        Self {
            seed,
            drawn: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn next(&self) -> Uuid {
        let drawn = self
            .drawn
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let high = mix(self.seed ^ mix(drawn));
        let low = mix(high);
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(&high.to_le_bytes());
        bytes[8..].copy_from_slice(&low.to_le_bytes());
        uuid::Builder::from_random_bytes(bytes).into_uuid()
    }
}

/// The output function of SplitMix64.
#[cfg(any(test, feature = "seeded-uuids"))]
fn mix(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn a_seed_gives_the_same_uuids_in_the_same_order() {
        let (a, b) = (SeededUuids::new(7), SeededUuids::new(7));
        for _ in 0..100 {
            assert_eq!(a.next(), b.next());
        }
    }

    #[test]
    fn seeds_do_not_share_uuids_and_none_repeats() {
        let (a, b) = (SeededUuids::new(7), SeededUuids::new(8));
        let all: HashSet<Uuid> = (0..1000).flat_map(|_| [a.next(), b.next()]).collect();
        assert_eq!(all.len(), 2000);
    }

    #[test]
    fn a_seeded_uuid_still_reads_as_a_version_4() {
        assert_eq!(SeededUuids::new(1).next().get_version_num(), 4);
    }

    #[test]
    fn without_a_seed_uuids_stay_random() {
        assert_ne!(random_uuid(), random_uuid());
    }
}

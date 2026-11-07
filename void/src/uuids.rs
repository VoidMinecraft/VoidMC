//! Where the UUIDs the server makes up come from: random, unless the server is stepped.
//!
//! A stepped server makes the n-th UUID of the process a function of a fixed seed and n, so two
//! stepped runs get the same UUIDs as long as they ask in the same order.

use std::sync::OnceLock;

use uuid::Uuid;

static STEPPED: OnceLock<SeededUuids> = OnceLock::new();

/// Makes every UUID of the process drawn after the call follow `seed`. Each build of
/// [`crate::pace::SteppedPacePlugin`] calls it; the first seed stays and later ones are ignored.
pub(crate) fn seed_for_steps(seed: u64) {
    let _ = STEPPED.set(SeededUuids::new(seed));
}

/// A fresh UUID, or the next of the seeded stream on a stepped server.
pub fn next() -> Uuid {
    STEPPED.get().map_or_else(fresh, SeededUuids::next)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the one source of fresh UUIDs, which a stepped run replaces"
)]
fn fresh() -> Uuid {
    Uuid::new_v4()
}

struct SeededUuids {
    seed: u64,
    drawn: std::sync::atomic::AtomicU64,
}

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
    fn a_stepped_server_draws_its_uuids_from_the_stepped_seed() {
        crate::pace::make_stepped(&mut bevy_ecs::world::World::new());
        let drawn = next();
        let stream = SeededUuids::new(crate::pace::STEPPED_SEED);
        assert!((0..100_000).any(|_| stream.next() == drawn));
    }

    #[test]
    fn fresh_uuids_are_random() {
        assert_ne!(fresh(), fresh());
    }
}

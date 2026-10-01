use std::fmt;

use crate::block::{BlockError, BlockState};
use crate::math::BlockPos;

pub const MAX_WEIGHT: f64 = 1_000_000.0;

/// What to place: one block state or a weighted random mix
/// (`50%stone,30%dirt,gravel`; weights are relative, a bare entry weighs 1).
/// Random picks hash the position with the seed, so a pattern needs no
/// mutable state and reproduces the same layout for the same seed.
#[derive(Clone, Debug, PartialEq)]
pub enum Pattern {
    Block(BlockState),
    Random(WeightedBlocks),
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeightedBlocks {
    states: Vec<BlockState>,
    cumulative: Vec<u64>,
    total: u64,
    seed: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PatternError {
    Empty,
    Weight(String),
    Block(BlockError),
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatternError::Empty => f.write_str("empty pattern"),
            PatternError::Weight(weight) => write!(f, "invalid weight '{weight}'"),
            PatternError::Block(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for PatternError {}

impl From<BlockError> for PatternError {
    fn from(error: BlockError) -> Self {
        PatternError::Block(error)
    }
}

impl From<BlockState> for Pattern {
    fn from(state: BlockState) -> Self {
        Pattern::Block(state)
    }
}

impl Pattern {
    pub fn parse(input: &str) -> Result<Self, PatternError> {
        let mut entries = Vec::new();
        for entry in split_top_level(input) {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            let (weight, block) = match entry.split_once('%') {
                Some((weight, block)) if !weight.contains('[') => {
                    let weight: f64 = weight
                        .parse()
                        .ok()
                        .filter(|w: &f64| w.is_finite() && *w > 0.0 && *w <= MAX_WEIGHT)
                        .ok_or_else(|| PatternError::Weight(weight.to_string()))?;
                    (weight, block)
                }
                _ => (1.0, entry),
            };
            entries.push((BlockState::parse(block)?, weight));
        }
        match entries.as_slice() {
            [] => Err(PatternError::Empty),
            [(state, _)] => Ok(Pattern::Block(*state)),
            _ => Ok(Pattern::Random(WeightedBlocks::new(entries))),
        }
    }

    /// Weights are clamped to `(0, MAX_WEIGHT]`.
    pub fn random(entries: impl IntoIterator<Item = (BlockState, f64)>) -> Self {
        let entries = entries
            .into_iter()
            .map(|(state, weight)| (state, weight.clamp(f64::MIN_POSITIVE, MAX_WEIGHT)))
            .collect();
        Pattern::Random(WeightedBlocks::new(entries))
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        if let Pattern::Random(weighted) = &mut self {
            weighted.seed = seed;
        }
        self
    }

    pub fn single(&self) -> Option<BlockState> {
        match self {
            Pattern::Block(state) => Some(*state),
            Pattern::Random(_) => None,
        }
    }

    #[inline]
    pub fn at(&self, pos: BlockPos) -> BlockState {
        match self {
            Pattern::Block(state) => *state,
            Pattern::Random(weighted) => weighted.at(pos),
        }
    }
}

impl WeightedBlocks {
    fn new(entries: Vec<(BlockState, f64)>) -> Self {
        let mut states = Vec::with_capacity(entries.len());
        let mut cumulative = Vec::with_capacity(entries.len());
        let mut total = 0u64;
        for (state, weight) in entries {
            total += (weight * 1000.0).round().max(1.0) as u64;
            states.push(state);
            cumulative.push(total);
        }
        Self {
            states,
            cumulative,
            total,
            seed: 0,
        }
    }

    #[inline]
    fn at(&self, pos: BlockPos) -> BlockState {
        let roll = mix(pos, self.seed) % self.total;
        let index = self.cumulative.partition_point(|&c| c <= roll);
        self.states[index]
    }
}

#[inline]
fn mix(pos: BlockPos, seed: u64) -> u64 {
    let mut h = seed
        ^ (pos.x as u32 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (pos.y as u32 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (pos.z as u32 as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^ (h >> 33)
}

/// Splits on commas outside `[...]`, so `oak_stairs[facing=east,half=top]`
/// stays one entry.
pub(crate) fn split_top_level(input: &str) -> impl Iterator<Item = &str> {
    let mut depth = 0i32;
    let mut start = 0;
    let mut parts = Vec::new();
    for (index, c) in input.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&input[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(&input[start..]);
    parts.into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(input: &str) -> BlockState {
        BlockState::parse(input).unwrap()
    }

    #[test]
    fn single_block_patterns() {
        assert_eq!(
            Pattern::parse("stone").unwrap(),
            Pattern::Block(state("stone"))
        );
        assert_eq!(
            Pattern::parse("oak_stairs[facing=east,half=top]").unwrap(),
            Pattern::Block(state("oak_stairs[facing=east,half=top]"))
        );
        assert_eq!(Pattern::parse(" , "), Err(PatternError::Empty));
        assert!(matches!(
            Pattern::parse("0%stone,dirt"),
            Err(PatternError::Weight(_))
        ));
        assert!(matches!(
            Pattern::parse("1e17%stone,1e17%dirt"),
            Err(PatternError::Weight(_))
        ));
        assert!(matches!(
            Pattern::parse("stone,nope"),
            Err(PatternError::Block(_))
        ));
    }

    #[test]
    fn weighted_patterns_follow_their_weights() {
        let pattern = Pattern::parse("75%stone,25%oak_stairs[facing=east,half=top]")
            .unwrap()
            .with_seed(7);
        let mut stone = 0;
        let samples = 40_000;
        for i in 0..samples {
            let picked = pattern.at(BlockPos::new(i % 200, i / 200, -i));
            if picked == state("stone") {
                stone += 1;
            } else {
                assert_eq!(picked, state("oak_stairs[facing=east,half=top]"));
            }
        }
        let share = f64::from(stone) / f64::from(samples);
        assert!((0.72..0.78).contains(&share), "stone share {share}");
    }

    #[test]
    fn random_patterns_are_deterministic_per_seed() {
        let a = Pattern::parse("stone,dirt").unwrap().with_seed(1);
        let b = Pattern::parse("stone,dirt").unwrap().with_seed(1);
        let c = Pattern::parse("stone,dirt").unwrap().with_seed(2);
        let positions: Vec<_> = (0..64).map(|i| BlockPos::new(i, 0, 0)).collect();
        let pick = |p: &Pattern| positions.iter().map(|pos| p.at(*pos)).collect::<Vec<_>>();
        assert_eq!(pick(&a), pick(&b));
        assert_ne!(pick(&a), pick(&c));
    }
}

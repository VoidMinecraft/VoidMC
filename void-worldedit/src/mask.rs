use std::fmt;
use std::sync::Arc;

use crate::block::{BlockError, BlockState, VERSION};
use crate::pattern::split_top_level;

/// Which existing blocks an edit may touch. Block lists compile to a bitset
/// over every block state, so testing a block is one bit lookup.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum Mask {
    #[default]
    Any,
    Existing,
    Blocks(Arc<BlockSet>),
    Not(Box<Mask>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockSet {
    bits: Vec<u64>,
}

impl BlockSet {
    pub fn new() -> Self {
        let states = voidmc_data::block_state_count(VERSION) as usize;
        Self {
            bits: vec![0; states.div_ceil(64)],
        }
    }

    pub fn insert(&mut self, state: BlockState) {
        let index = state.0 as usize;
        if let Some(word) = self.bits.get_mut(index / 64) {
            *word |= 1 << (index % 64);
        }
    }

    pub fn insert_block(&mut self, state: BlockState) {
        match state.block_type() {
            Some(block) => {
                for id in block.min_state_id..=block.max_state_id() {
                    self.insert(BlockState(id as u32));
                }
            }
            None => self.insert(state),
        }
    }

    #[inline]
    pub fn contains(&self, state: BlockState) -> bool {
        let index = state.0 as usize;
        self.bits
            .get(index / 64)
            .is_some_and(|word| word & (1 << (index % 64)) != 0)
    }
}

impl Default for BlockSet {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MaskError(pub BlockError);

impl fmt::Display for MaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for MaskError {}

impl Mask {
    /// `stone,dirt` matches any state of those blocks, `oak_stairs[facing=east]`
    /// only that state, `#existing` any non-air block, `*` everything, and a
    /// leading `!` negates the whole mask.
    pub fn parse(input: &str) -> Result<Self, MaskError> {
        let input = input.trim();
        if let Some(rest) = input.strip_prefix('!') {
            return Ok(Mask::Not(Box::new(Mask::parse(rest)?)));
        }
        match input {
            "*" | "#any" | "" => return Ok(Mask::Any),
            "#existing" => return Ok(Mask::Existing),
            _ => {}
        }
        let mut set = BlockSet::new();
        for entry in split_top_level(input) {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }
            let state = BlockState::parse(entry).map_err(MaskError)?;
            if entry.contains('[') {
                set.insert(state);
            } else {
                set.insert_block(state);
            }
        }
        Ok(Mask::Blocks(Arc::new(set)))
    }

    pub fn blocks(states: impl IntoIterator<Item = BlockState>) -> Self {
        let mut set = BlockSet::new();
        for state in states {
            set.insert_block(state);
        }
        Mask::Blocks(Arc::new(set))
    }

    pub fn is_any(&self) -> bool {
        matches!(self, Mask::Any)
    }

    #[inline]
    pub fn test(&self, state: BlockState) -> bool {
        match self {
            Mask::Any => true,
            Mask::Existing => !state.is_air(),
            Mask::Blocks(set) => set.contains(state),
            Mask::Not(inner) => !inner.test(state),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(input: &str) -> BlockState {
        BlockState::parse(input).unwrap()
    }

    #[test]
    fn block_names_match_every_state() {
        let mask = Mask::parse("oak_stairs,stone").unwrap();
        assert!(mask.test(state("stone")));
        assert!(mask.test(state("oak_stairs[facing=east]")));
        assert!(mask.test(state("oak_stairs[facing=west,half=top]")));
        assert!(!mask.test(state("dirt")));
    }

    #[test]
    fn explicit_states_match_only_themselves() {
        let mask = Mask::parse("oak_stairs[facing=east]").unwrap();
        assert!(mask.test(state("oak_stairs[facing=east]")));
        assert!(!mask.test(state("oak_stairs[facing=west]")));
    }

    #[test]
    fn keywords_and_negation() {
        assert_eq!(Mask::parse("*").unwrap(), Mask::Any);
        let existing = Mask::parse("#existing").unwrap();
        assert!(existing.test(state("dirt")));
        assert!(!existing.test(state("cave_air")));
        let not_stone = Mask::parse("!stone").unwrap();
        assert!(!not_stone.test(state("stone")));
        assert!(not_stone.test(state("air")));
        assert!(Mask::parse("stone,bogus").is_err());
    }
}

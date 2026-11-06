use voidmc_codec::{Decode, Encode, VarI64};

/// Many blocks of one section, in one packet. The client changes them in place, which
/// does not take the chunk away and bring it back.
#[derive(Debug, Clone, Encode, Decode)]
pub struct SectionBlocksUpdate {
    /// See [`SectionBlocksUpdate::section`].
    pub section: i64,
    /// See [`SectionBlocksUpdate::block`].
    pub blocks: Vec<VarI64>,
}

impl SectionBlocksUpdate {
    /// The packed position of a section, in sections.
    pub fn section(x: i32, y: i32, z: i32) -> i64 {
        (i64::from(x) & 0x3F_FFFF) << 42
            | (i64::from(z) & 0x3F_FFFF) << 20
            | i64::from(y) & 0xF_FFFF
    }

    /// A block state at a place of its section, from 0 to 15 on each axis.
    pub fn block(x: u8, y: u8, z: u8, state: i32) -> VarI64 {
        let place = i64::from(x) << 8 | i64::from(z) << 4 | i64::from(y);
        VarI64(i64::from(state) << 12 | place)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_section_packs_as_the_client_reads_it() {
        let packed = SectionBlocksUpdate::section(-1, -4, 60_000);
        assert_eq!(packed >> 42, -1);
        assert_eq!(packed << 22 >> 42, 60_000);
        assert_eq!(packed << 44 >> 44, -4);
    }

    #[test]
    fn a_block_keeps_its_state_over_its_place() {
        let VarI64(entry) = SectionBlocksUpdate::block(1, 2, 3, 4000);
        assert_eq!(entry >> 12, 4000);
        assert_eq!(entry & 0xFFF, 0x132);
    }
}

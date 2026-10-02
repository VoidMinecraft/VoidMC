use crate::Version;

/// A block and its state range; state ids are laid out with the last
/// property varying fastest, as in Mojang's `blocks.json` report.
#[derive(Debug, PartialEq, Eq)]
pub struct BlockType {
    pub name: &'static str,
    pub min_state_id: i32,
    pub default_state_id: i32,
    pub properties: &'static [BlockProperty],
}

#[derive(Debug, PartialEq, Eq)]
pub struct BlockProperty {
    pub name: &'static str,
    pub values: &'static [&'static str],
}

impl BlockType {
    pub fn state_count(&self) -> i32 {
        self.properties
            .iter()
            .map(|property| property.values.len() as i32)
            .product()
    }

    pub fn max_state_id(&self) -> i32 {
        self.min_state_id + self.state_count() - 1
    }

    pub fn contains(&self, state_id: i32) -> bool {
        (self.min_state_id..=self.max_state_id()).contains(&state_id)
    }

    fn stride(&self, index: usize) -> i32 {
        self.properties[index + 1..]
            .iter()
            .map(|property| property.values.len() as i32)
            .product()
    }

    fn value_index(&self, state_id: i32, index: usize) -> usize {
        let len = self.properties[index].values.len() as i32;
        ((state_id - self.min_state_id) / self.stride(index) % len) as usize
    }

    /// The value of `property` in `state_id`; `None` for a state of another block.
    pub fn property(&self, state_id: i32, property: &str) -> Option<&'static str> {
        if !self.contains(state_id) {
            return None;
        }
        let index = self.properties.iter().position(|p| p.name == property)?;
        Some(self.properties[index].values[self.value_index(state_id, index)])
    }

    /// Every `(property, value)` pair of `state_id`, in declaration order.
    pub fn property_values(
        &self,
        state_id: i32,
    ) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        self.properties
            .iter()
            .enumerate()
            .map(move |(index, p)| (p.name, p.values[self.value_index(state_id, index)]))
    }

    /// `state_id` with `property` set to `value`, or `None` when this block
    /// has no such property or value.
    pub fn with_property(&self, state_id: i32, property: &str, value: &str) -> Option<i32> {
        if !self.contains(state_id) {
            return None;
        }
        let index = self.properties.iter().position(|p| p.name == property)?;
        let target = self.properties[index]
            .values
            .iter()
            .position(|v| *v == value)? as i32;
        let current = self.value_index(state_id, index) as i32;
        Some(state_id + (target - current) * self.stride(index))
    }
}

pub fn block_types(version: Version) -> &'static [BlockType] {
    match version {
        Version::V26_1_2 => crate::v26_1_2::block_types::BLOCK_TYPES,
    }
}

/// Total number of block states, which sizes a direct chunk palette.
pub fn block_state_count(version: Version) -> i32 {
    block_types(version)
        .last()
        .map_or(0, |block| block.max_state_id() + 1)
}

/// The block owning `state_id`.
pub fn block_type_of_state(version: Version, state_id: i32) -> Option<&'static BlockType> {
    let types = block_types(version);
    let index = types
        .partition_point(|block| block.min_state_id <= state_id)
        .checked_sub(1)?;
    let block = &types[index];
    block.contains(state_id).then_some(block)
}

/// The block named `name`, e.g. `"minecraft:oak_stairs"`.
pub fn block_type(version: Version, name: &str) -> Option<&'static BlockType> {
    block_type_of_state(version, crate::block_default_state(version, name)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const V: Version = Version::V26_1_2;

    #[test]
    fn every_state_id_resolves_to_exactly_one_block() {
        let types = block_types(V);
        assert_eq!(types[0].min_state_id, 0);
        for pair in types.windows(2) {
            assert_eq!(pair[0].max_state_id() + 1, pair[1].min_state_id);
        }
        assert_eq!(block_state_count(V), 29_873);
        assert_eq!(block_type_of_state(V, 0).unwrap().name, "minecraft:air");
        assert_eq!(block_type_of_state(V, 29_873), None);
        assert_eq!(block_type_of_state(V, -1), None);
    }

    #[test]
    fn decodes_and_rewrites_properties_like_the_report() {
        let stairs = block_type(V, "minecraft:oak_stairs").unwrap();
        assert_eq!(stairs.min_state_id, 3907);
        assert_eq!(stairs.state_count(), 4 * 2 * 5 * 2);
        let values: Vec<_> = stairs.property_values(3908).collect();
        assert_eq!(
            values,
            vec![
                ("facing", "north"),
                ("half", "top"),
                ("shape", "straight"),
                ("waterlogged", "false"),
            ]
        );
        let east = stairs.with_property(3908, "facing", "east").unwrap();
        assert_eq!(stairs.property(east, "facing"), Some("east"));
        assert_eq!(stairs.property(east, "waterlogged"), Some("false"));
        assert_eq!(stairs.with_property(3908, "facing", "up"), None);
        assert_eq!(stairs.with_property(3908, "axis", "x"), None);
        assert_eq!(stairs.property(0, "facing"), None);
        assert_eq!(stairs.with_property(0, "facing", "east"), None);
    }

    #[test]
    fn integer_properties_keep_their_real_values() {
        let repeater = block_type(V, "minecraft:repeater").unwrap();
        let delay = repeater
            .properties
            .iter()
            .find(|p| p.name == "delay")
            .unwrap();
        assert_eq!(delay.values, &["1", "2", "3", "4"]);
        assert_eq!(
            repeater.property(repeater.default_state_id, "delay"),
            Some("1")
        );
    }
}

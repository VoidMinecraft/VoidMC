use std::fmt;

use voidmc_data::{BlockType, Version, v26_1_2::blocks};

use crate::math::Axis;

pub const VERSION: Version = Version::V26_1_2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(transparent)]
pub struct BlockState(pub u32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockError {
    UnknownBlock(String),
    UnknownProperty { block: String, property: String },
    InvalidValue { property: String, value: String },
    Malformed(String),
}

impl fmt::Display for BlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockError::UnknownBlock(name) => write!(f, "unknown block '{name}'"),
            BlockError::UnknownProperty { block, property } => {
                write!(f, "{block} has no property '{property}'")
            }
            BlockError::InvalidValue { property, value } => {
                write!(f, "'{value}' is not a valid value for '{property}'")
            }
            BlockError::Malformed(input) => write!(f, "malformed block state '{input}'"),
        }
    }
}

impl std::error::Error for BlockError {}

impl BlockState {
    pub const AIR: Self = Self(blocks::AIR as u32);

    pub fn id(self) -> u32 {
        self.0
    }

    pub fn is_air(self) -> bool {
        matches!(
            self.0 as i32,
            blocks::AIR | blocks::CAVE_AIR | blocks::VOID_AIR
        )
    }

    pub fn block_type(self) -> Option<&'static BlockType> {
        voidmc_data::block_type_of_state(VERSION, self.0 as i32)
    }

    pub fn name(self) -> &'static str {
        self.block_type()
            .map_or("minecraft:air", |block| block.name)
    }

    pub fn property(self, name: &str) -> Option<&'static str> {
        self.block_type()?.property(self.0 as i32, name)
    }

    pub fn with(self, property: &str, value: &str) -> Option<Self> {
        self.block_type()?
            .with_property(self.0 as i32, property, value)
            .map(|id| Self(id as u32))
    }

    pub fn default_of(name: &str) -> Option<Self> {
        voidmc_data::block_default_state(VERSION, &qualify(name)).map(|id| Self(id as u32))
    }

    /// Parses `stone`, `minecraft:oak_stairs[facing=east,half=top]`; unset
    /// properties keep the block's default.
    pub fn parse(input: &str) -> Result<Self, BlockError> {
        let input = input.trim();
        let (name, properties) = match input.split_once('[') {
            Some((name, rest)) => {
                let body = rest
                    .strip_suffix(']')
                    .ok_or_else(|| BlockError::Malformed(input.to_string()))?;
                (name, Some(body))
            }
            None => (input, None),
        };
        let qualified = qualify(name);
        let block = voidmc_data::block_type(VERSION, &qualified)
            .ok_or_else(|| BlockError::UnknownBlock(name.to_string()))?;
        let mut state = block.default_state_id;
        for pair in properties.into_iter().flat_map(|body| body.split(',')) {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }
            let (property, value) = pair
                .split_once('=')
                .ok_or_else(|| BlockError::Malformed(input.to_string()))?;
            let (property, value) = (property.trim(), value.trim());
            if !block.properties.iter().any(|p| p.name == property) {
                return Err(BlockError::UnknownProperty {
                    block: block.name.to_string(),
                    property: property.to_string(),
                });
            }
            state = block.with_property(state, property, value).ok_or_else(|| {
                BlockError::InvalidValue {
                    property: property.to_string(),
                    value: value.to_string(),
                }
            })?;
        }
        Ok(Self(state as u32))
    }

    /// Like [`parse`](Self::parse) but drops unknown properties and values,
    /// for data written by other Minecraft versions.
    pub fn parse_lenient(input: &str) -> Option<Self> {
        let (name, body) = input.split_once('[').unwrap_or((input, ""));
        let block = voidmc_data::block_type(VERSION, &qualify(name))?;
        let mut state = block.default_state_id;
        for (property, value) in body
            .trim_end_matches(']')
            .split(',')
            .filter_map(|pair| pair.split_once('='))
        {
            if let Some(next) = block.with_property(state, property.trim(), value.trim()) {
                state = next;
            }
        }
        Some(Self(state as u32))
    }

    /// The state turned by `quarter_turns` × 90° clockwise seen from above.
    pub fn rotated(self, quarter_turns: u8) -> Self {
        let turns = quarter_turns % 4;
        let Some(block) = self.block_type() else {
            return self;
        };
        if turns == 0 || block.properties.is_empty() {
            return self;
        }
        let id = self.0 as i32;
        let mut state = id;
        for property in block.properties {
            let value = block.property(id, property.name).unwrap_or_default();
            let rotated = match property.name {
                "rotation" => value
                    .parse::<u8>()
                    .ok()
                    .and_then(|r| one(((r + 4 * turns) % 16).to_string())),
                "axis" if turns % 2 == 1 => one(match value {
                    "x" => "z",
                    "z" => "x",
                    other => other,
                }),
                "north" | "east" | "south" | "west" => {
                    let source = rotate_direction_word(property.name, 4 - turns);
                    block.property(id, source).and_then(one)
                }
                name => map_direction_words(name, value, |word| rotate_direction_word(word, turns)),
            };
            state = apply(block, state, property.name, rotated);
        }
        Self(state as u32)
    }

    /// The state mirrored across the plane perpendicular to `axis`.
    pub fn flipped(self, axis: Axis) -> Self {
        let Some(block) = self.block_type() else {
            return self;
        };
        if block.properties.is_empty() {
            return self;
        }
        let id = self.0 as i32;
        let mut state = id;
        for property in block.properties {
            let value = block.property(id, property.name).unwrap_or_default();
            let flipped = match property.name {
                "rotation" if axis != Axis::Y => value.parse::<u8>().ok().and_then(|r| {
                    one(match axis {
                        Axis::X => (16 - r) % 16,
                        _ => (24 - r) % 16,
                    }
                    .to_string())
                }),
                "north" | "east" | "south" | "west" => {
                    let source = flip_direction_word(property.name, axis);
                    block.property(id, source).and_then(one)
                }
                "up" | "down" if axis == Axis::Y => {
                    let source = flip_direction_word(property.name, axis);
                    block.property(id, source).and_then(one)
                }
                "shape" | "hinge" | "type" if axis != Axis::Y && is_handed(value) => {
                    one(swap_hand(value))
                }
                "half" | "type" | "face" | "attachment" if axis == Axis::Y => one(match value {
                    "top" => "bottom",
                    "bottom" => "top",
                    "upper" => "lower",
                    "lower" => "upper",
                    "floor" => "ceiling",
                    "ceiling" => "floor",
                    other => other,
                }),
                "hanging" if axis == Axis::Y => one(if value == "true" { "false" } else { "true" }),
                name => map_direction_words(name, value, |word| flip_direction_word(word, axis)),
            };
            state = apply(block, state, property.name, flipped);
        }
        Self(state as u32)
    }
}

fn apply(block: &BlockType, state: i32, property: &str, candidates: Option<Vec<String>>) -> i32 {
    candidates
        .into_iter()
        .flatten()
        .find_map(|value| block.with_property(state, property, &value))
        .unwrap_or(state)
}

fn qualify(name: &str) -> String {
    let name = name.trim();
    if name.contains(':') {
        name.to_ascii_lowercase()
    } else {
        format!("minecraft:{}", name.to_ascii_lowercase())
    }
}

const HORIZONTAL: [&str; 4] = ["north", "east", "south", "west"];

fn rotate_direction_word(word: &str, turns: u8) -> &str {
    match HORIZONTAL.iter().position(|d| *d == word) {
        Some(index) => HORIZONTAL[(index + turns as usize) % 4],
        None => word,
    }
}

fn flip_direction_word(word: &str, axis: Axis) -> &str {
    match (axis, word) {
        (Axis::X, "east") => "west",
        (Axis::X, "west") => "east",
        (Axis::Z, "north") => "south",
        (Axis::Z, "south") => "north",
        (Axis::Y, "up") => "down",
        (Axis::Y, "down") => "up",
        (_, other) => other,
    }
}

/// Rewrites every direction word of a `_`-joined value (`north_east`,
/// `ascending_west`, `up_south`). Rail `shape`s also try the reversed word
/// order, since only one order of each pair exists. `None` when nothing
/// changed.
fn map_direction_words<'a>(
    property: &str,
    value: &'a str,
    map: impl Fn(&'a str) -> &'a str,
) -> Option<Vec<String>> {
    let words: Vec<&str> = value.split('_').collect();
    let mapped: Vec<&str> = words.iter().map(|word| map(word)).collect();
    if mapped == words {
        return None;
    }
    let mut candidates = vec![mapped.join("_")];
    if property == "shape" && mapped.len() == 2 {
        candidates.push(format!("{}_{}", mapped[1], mapped[0]));
    }
    Some(candidates)
}

fn one(value: impl Into<String>) -> Option<Vec<String>> {
    Some(vec![value.into()])
}

fn is_handed(value: &str) -> bool {
    value.contains("left") || value.contains("right")
}

fn swap_hand(value: &str) -> String {
    if value.contains("left") {
        value.replace("left", "right")
    } else {
        value.replace("right", "left")
    }
}

impl fmt::Display for BlockState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(block) = self.block_type() else {
            return f.write_str("minecraft:air");
        };
        f.write_str(block.name)?;
        if block.properties.is_empty() {
            return Ok(());
        }
        f.write_str("[")?;
        for (index, (property, value)) in block.property_values(self.0 as i32).enumerate() {
            if index > 0 {
                f.write_str(",")?;
            }
            write!(f, "{property}={value}")?;
        }
        f.write_str("]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(input: &str) -> BlockState {
        BlockState::parse(input).unwrap()
    }

    #[test]
    fn parses_names_and_properties() {
        assert_eq!(state("stone"), BlockState(blocks::STONE as u32));
        assert_eq!(state("minecraft:stone"), state("STONE"));
        let stairs = state("oak_stairs[facing=east,half=top]");
        assert_eq!(stairs.property("facing"), Some("east"));
        assert_eq!(stairs.property("half"), Some("top"));
        assert_eq!(stairs.property("shape"), Some("straight"));
        assert!(matches!(
            BlockState::parse("nope"),
            Err(BlockError::UnknownBlock(_))
        ));
        assert!(matches!(
            BlockState::parse("stone[facing=east]"),
            Err(BlockError::UnknownProperty { .. })
        ));
        assert!(matches!(
            BlockState::parse("oak_stairs[facing=up]"),
            Err(BlockError::InvalidValue { .. })
        ));
        assert!(matches!(
            BlockState::parse("oak_stairs[facing=east"),
            Err(BlockError::Malformed(_))
        ));
    }

    #[test]
    fn display_round_trips_through_parse() {
        for input in [
            "minecraft:stone",
            "minecraft:oak_stairs[facing=west,half=bottom,shape=outer_left,waterlogged=true]",
            "minecraft:repeater[delay=3,facing=south,locked=false,powered=false]",
        ] {
            assert_eq!(state(input).to_string(), input);
        }
    }

    #[test]
    fn lenient_parse_ignores_unknown_properties() {
        let parsed = BlockState::parse_lenient("minecraft:oak_log[axis=x,legacy=1]").unwrap();
        assert_eq!(parsed, state("oak_log[axis=x]"));
        assert_eq!(BlockState::parse_lenient("minecraft:unknown_block"), None);
    }

    #[test]
    fn air_variants_are_air() {
        assert!(BlockState::AIR.is_air());
        assert!(state("cave_air").is_air());
        assert!(state("void_air").is_air());
        assert!(!state("glass").is_air());
    }

    #[test]
    fn rotation_turns_directional_properties_clockwise() {
        let stairs = state("oak_stairs[facing=north]");
        assert_eq!(stairs.rotated(1).property("facing"), Some("east"));
        assert_eq!(stairs.rotated(2).property("facing"), Some("south"));
        assert_eq!(stairs.rotated(3).property("facing"), Some("west"));
        assert_eq!(stairs.rotated(4), stairs);

        assert_eq!(
            state("oak_log[axis=x]").rotated(1).property("axis"),
            Some("z")
        );
        assert_eq!(
            state("oak_log[axis=y]").rotated(1).property("axis"),
            Some("y")
        );
        assert_eq!(
            state("oak_sign[rotation=15]")
                .rotated(1)
                .property("rotation"),
            Some("3")
        );

        let fence = state("oak_fence[north=true,east=false,south=false,west=false]");
        let turned = fence.rotated(1);
        assert_eq!(turned.property("east"), Some("true"));
        assert_eq!(turned.property("north"), Some("false"));

        let rail = state("rail[shape=north_east]");
        assert_eq!(rail.rotated(1).property("shape"), Some("south_east"));
        assert_eq!(rail.rotated(2).property("shape"), Some("south_west"));
        assert_eq!(
            state("rail[shape=ascending_north]")
                .rotated(1)
                .property("shape"),
            Some("ascending_east")
        );
        assert_eq!(
            state("rail[shape=north_south]")
                .rotated(1)
                .property("shape"),
            Some("east_west")
        );
    }

    #[test]
    fn every_flip_and_full_turn_is_an_involution() {
        let count = voidmc_data::block_state_count(VERSION) as u32;
        for id in 0..count {
            let state = BlockState(id);
            for axis in [Axis::X, Axis::Y, Axis::Z] {
                assert_eq!(state.flipped(axis).flipped(axis), state, "{state} {axis:?}");
            }
            assert_eq!(state.rotated(1).rotated(3), state, "{state}");
        }
    }

    #[test]
    fn flips_mirror_directions_and_handedness() {
        let stairs = state("oak_stairs[facing=east,shape=inner_left]");
        let flipped = stairs.flipped(Axis::X);
        assert_eq!(flipped.property("facing"), Some("west"));
        assert_eq!(flipped.property("shape"), Some("inner_right"));
        assert_eq!(flipped.flipped(Axis::X), stairs);

        let slab = state("oak_slab[type=top]");
        assert_eq!(slab.flipped(Axis::Y).property("type"), Some("bottom"));
        assert_eq!(slab.flipped(Axis::X), slab);

        let door = state("oak_door[hinge=left,facing=north]");
        let mirrored = door.flipped(Axis::Z);
        assert_eq!(mirrored.property("hinge"), Some("right"));
        assert_eq!(mirrored.property("facing"), Some("south"));

        assert_eq!(
            state("oak_sign[rotation=4]")
                .flipped(Axis::X)
                .property("rotation"),
            Some("12")
        );
        assert_eq!(
            state("oak_sign[rotation=0]")
                .flipped(Axis::Z)
                .property("rotation"),
            Some("8")
        );
    }
}

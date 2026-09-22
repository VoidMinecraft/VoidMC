use voidmc_codec::{Decode, DecodeError, Encode, VarI32};

/// The type of string parsing expected by the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringType {
    /// Reads a single word.
    SingleWord = 0,
    /// Reads a quotable phrase (either a single word or a quoted string).
    QuotablePhrase = 1,
    /// Reads the rest of the input.
    GreedyPhrase = 2,
}

/// Argument parsers the Minecraft client understands.
///
/// Ids follow the `minecraft:command_argument_type` registry order of
/// 26.1.2 (`ArgumentTypeInfos::bootstrap`).
#[derive(Debug, Clone, PartialEq)]
pub enum Parser {
    Bool,
    Float { min: Option<f32>, max: Option<f32> },
    Double { min: Option<f64>, max: Option<f64> },
    Integer { min: Option<i32>, max: Option<i32> },
    Long { min: Option<i64>, max: Option<i64> },
    String(StringType),
    Entity { single: bool, players_only: bool },
    GameProfile,
    BlockPos,
    ColumnPos,
    Vec3,
    Vec2,
    BlockState,
    ItemStack,
    Color,
    HexColor,
    Component,
    Style,
    Message,
    Angle,
    Rotation,
    ResourceLocation,
    Dimension,
    GameMode,
    Time { min: i32 },
    Resource { registry: String },
    ResourceKey { registry: String },
    Uuid,
}

impl Parser {
    pub fn parser_id(&self) -> i32 {
        match self {
            Parser::Bool => 0,
            Parser::Float { .. } => 1,
            Parser::Double { .. } => 2,
            Parser::Integer { .. } => 3,
            Parser::Long { .. } => 4,
            Parser::String(_) => 5,
            Parser::Entity { .. } => 6,
            Parser::GameProfile => 7,
            Parser::BlockPos => 8,
            Parser::ColumnPos => 9,
            Parser::Vec3 => 10,
            Parser::Vec2 => 11,
            Parser::BlockState => 12,
            Parser::ItemStack => 14,
            Parser::Color => 16,
            Parser::HexColor => 17,
            Parser::Component => 18,
            Parser::Style => 19,
            Parser::Message => 20,
            Parser::Angle => 28,
            Parser::Rotation => 29,
            Parser::ResourceLocation => 36,
            Parser::Dimension => 41,
            Parser::GameMode => 42,
            Parser::Time { .. } => 43,
            Parser::Resource { .. } => 46,
            Parser::ResourceKey { .. } => 47,
            Parser::Uuid => 56,
        }
    }

    fn encode_number_bounds<T: Copy>(
        buf: &mut Vec<u8>,
        min: Option<T>,
        max: Option<T>,
        write: impl Fn(&mut Vec<u8>, T),
    ) {
        let flags = (min.is_some() as u8) | ((max.is_some() as u8) << 1);
        buf.push(flags);
        if let Some(v) = min {
            write(buf, v);
        }
        if let Some(v) = max {
            write(buf, v);
        }
    }

    pub fn encode_properties(&self, buf: &mut Vec<u8>) {
        match self {
            Parser::Float { min, max } => {
                Self::encode_number_bounds(buf, *min, *max, |buf, v| {
                    buf.extend_from_slice(&v.to_be_bytes())
                });
            }
            Parser::Double { min, max } => {
                Self::encode_number_bounds(buf, *min, *max, |buf, v| {
                    buf.extend_from_slice(&v.to_be_bytes())
                });
            }
            Parser::Integer { min, max } => {
                Self::encode_number_bounds(buf, *min, *max, |buf, v| {
                    buf.extend_from_slice(&v.to_be_bytes())
                });
            }
            Parser::Long { min, max } => {
                Self::encode_number_bounds(buf, *min, *max, |buf, v| {
                    buf.extend_from_slice(&v.to_be_bytes())
                });
            }
            Parser::String(string_type) => {
                VarI32(*string_type as i32).encode(buf);
            }
            Parser::Entity {
                single,
                players_only,
            } => {
                let flags = (*single as u8) | ((*players_only as u8) << 1);
                buf.push(flags);
            }
            Parser::Time { min } => {
                buf.extend_from_slice(&min.to_be_bytes());
            }
            Parser::Resource { registry } | Parser::ResourceKey { registry } => {
                encode_string(buf, registry);
            }
            Parser::Bool
            | Parser::GameProfile
            | Parser::BlockPos
            | Parser::ColumnPos
            | Parser::Vec3
            | Parser::Vec2
            | Parser::BlockState
            | Parser::ItemStack
            | Parser::Color
            | Parser::HexColor
            | Parser::Component
            | Parser::Style
            | Parser::Message
            | Parser::Angle
            | Parser::Rotation
            | Parser::ResourceLocation
            | Parser::Dimension
            | Parser::GameMode
            | Parser::Uuid => {}
        }
    }
}

/// A node in the command tree sent to the client for tab-completion.
#[derive(Debug, Clone)]
pub struct CommandNode {
    /// 0 = root, 1 = literal, 2 = argument
    pub node_type: u8,
    pub is_executable: bool,
    pub children: Vec<i32>,
    pub redirect_node: Option<i32>,
    pub name: Option<String>,
    pub parser: Option<Parser>,
    pub suggestions_type: Option<String>,
}

/// Clientbound Commands packet — declares the server's command tree for autocompletion.
#[derive(Debug, Clone)]
pub struct Commands {
    pub nodes: Vec<CommandNode>,
    pub root_index: i32,
}

fn encode_string(buf: &mut Vec<u8>, s: &str) {
    VarI32(s.len() as i32).encode(buf);
    buf.extend_from_slice(s.as_bytes());
}

impl Encode for Commands {
    fn encode(&self, buf: &mut Vec<u8>) {
        // Count of nodes
        VarI32(self.nodes.len() as i32).encode(buf);

        for node in &self.nodes {
            // Flags byte: type (2 bits) | is_executable (0x04) | has_redirect (0x08) | has_suggestions (0x10)
            let mut flags = node.node_type & 0x03;
            if node.is_executable {
                flags |= 0x04;
            }
            if node.redirect_node.is_some() {
                flags |= 0x08;
            }
            if node.suggestions_type.is_some() {
                flags |= 0x10;
            }
            buf.push(flags);

            // Children indices
            VarI32(node.children.len() as i32).encode(buf);
            for &child in &node.children {
                VarI32(child).encode(buf);
            }

            // Redirect node (optional)
            if let Some(redirect) = node.redirect_node {
                VarI32(redirect).encode(buf);
            }

            // Name (for literal and argument nodes)
            if (node.node_type == 1 || node.node_type == 2)
                && let Some(ref name) = node.name
            {
                encode_string(buf, name);
            }

            // Parser (for argument nodes only)
            if node.node_type == 2
                && let Some(ref parser) = node.parser
            {
                VarI32(parser.parser_id()).encode(buf);
                parser.encode_properties(buf);
            }

            // Suggestions type (optional)
            if let Some(ref suggestions) = node.suggestions_type {
                encode_string(buf, suggestions);
            }
        }

        // Root index
        VarI32(self.root_index).encode(buf);
    }
}

impl Decode for Commands {
    fn decode_with(_decoder: &mut voidmc_codec::Decoder<'_>) -> Result<Self, DecodeError> {
        // Server-only packet, no need to decode
        Err(DecodeError::InvalidLength)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_ids_follow_the_26_1_2_registry_order() {
        let expected = [
            (Parser::Bool, 0),
            (
                Parser::Float {
                    min: None,
                    max: None,
                },
                1,
            ),
            (
                Parser::Double {
                    min: None,
                    max: None,
                },
                2,
            ),
            (
                Parser::Integer {
                    min: None,
                    max: None,
                },
                3,
            ),
            (
                Parser::Long {
                    min: None,
                    max: None,
                },
                4,
            ),
            (Parser::String(StringType::SingleWord), 5),
            (
                Parser::Entity {
                    single: true,
                    players_only: true,
                },
                6,
            ),
            (Parser::GameProfile, 7),
            (Parser::BlockPos, 8),
            (Parser::ColumnPos, 9),
            (Parser::Vec3, 10),
            (Parser::Vec2, 11),
            (Parser::BlockState, 12),
            (Parser::ItemStack, 14),
            (Parser::Color, 16),
            (Parser::HexColor, 17),
            (Parser::Component, 18),
            (Parser::Style, 19),
            (Parser::Message, 20),
            (Parser::Angle, 28),
            (Parser::Rotation, 29),
            (Parser::ResourceLocation, 36),
            (Parser::Dimension, 41),
            (Parser::GameMode, 42),
            (Parser::Time { min: 0 }, 43),
            (
                Parser::Resource {
                    registry: "minecraft:item".into(),
                },
                46,
            ),
            (
                Parser::ResourceKey {
                    registry: "minecraft:item".into(),
                },
                47,
            ),
            (Parser::Uuid, 56),
        ];
        for (parser, id) in expected {
            assert_eq!(parser.parser_id(), id, "{parser:?}");
        }
    }

    #[test]
    fn message_parser_id_is_not_style() {
        assert_eq!(Parser::Message.parser_id(), 20);
        assert_ne!(Parser::Message.parser_id(), Parser::Style.parser_id());
    }

    #[test]
    fn singleton_parsers_encode_no_properties() {
        for parser in [
            Parser::Bool,
            Parser::GameProfile,
            Parser::BlockPos,
            Parser::ColumnPos,
            Parser::Vec3,
            Parser::Vec2,
            Parser::BlockState,
            Parser::ItemStack,
            Parser::Color,
            Parser::HexColor,
            Parser::Component,
            Parser::Style,
            Parser::Message,
            Parser::Angle,
            Parser::Rotation,
            Parser::ResourceLocation,
            Parser::Dimension,
            Parser::GameMode,
            Parser::Uuid,
        ] {
            let mut buf = Vec::new();
            parser.encode_properties(&mut buf);
            assert!(buf.is_empty(), "{parser:?}");
        }
    }

    #[test]
    fn number_bounds_encode_flags_then_min_then_max() {
        let mut buf = Vec::new();
        Parser::Integer {
            min: Some(0),
            max: Some(3),
        }
        .encode_properties(&mut buf);
        assert_eq!(buf, [0x03, 0, 0, 0, 0, 0, 0, 0, 3]);

        let mut buf = Vec::new();
        Parser::Float {
            min: Some(0.5),
            max: None,
        }
        .encode_properties(&mut buf);
        assert_eq!(buf, [0x01, 0x3f, 0x00, 0x00, 0x00]);

        let mut buf = Vec::new();
        Parser::Double {
            min: None,
            max: Some(1.0),
        }
        .encode_properties(&mut buf);
        assert_eq!(buf, [0x02, 0x3f, 0xf0, 0, 0, 0, 0, 0, 0]);

        let mut buf = Vec::new();
        Parser::Long {
            min: None,
            max: None,
        }
        .encode_properties(&mut buf);
        assert_eq!(buf, [0x00]);
    }

    #[test]
    fn entity_flags_single_is_bit0_players_only_is_bit1() {
        let mut buf = Vec::new();
        Parser::Entity {
            single: true,
            players_only: false,
        }
        .encode_properties(&mut buf);
        assert_eq!(buf, [0x01]);

        let mut buf = Vec::new();
        Parser::Entity {
            single: false,
            players_only: true,
        }
        .encode_properties(&mut buf);
        assert_eq!(buf, [0x02]);
    }

    #[test]
    fn time_encodes_min_as_plain_int() {
        let mut buf = Vec::new();
        Parser::Time { min: -1 }.encode_properties(&mut buf);
        assert_eq!(buf, [0xff, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn resource_parsers_encode_registry_key_as_string() {
        let mut buf = Vec::new();
        Parser::Resource {
            registry: "minecraft:item".into(),
        }
        .encode_properties(&mut buf);
        assert_eq!(buf[0], 14);
        assert_eq!(&buf[1..], b"minecraft:item");
    }

    #[test]
    fn string_type_encodes_as_varint_enum_ordinal() {
        for (string_type, ordinal) in [
            (StringType::SingleWord, 0),
            (StringType::QuotablePhrase, 1),
            (StringType::GreedyPhrase, 2),
        ] {
            let mut buf = Vec::new();
            Parser::String(string_type).encode_properties(&mut buf);
            assert_eq!(buf, [ordinal]);
        }
    }

    #[test]
    fn argument_node_encodes_flags_children_name_parser_and_suggestions() {
        let tree = Commands {
            nodes: vec![
                CommandNode {
                    node_type: 0,
                    is_executable: false,
                    children: vec![1],
                    redirect_node: None,
                    name: None,
                    parser: None,
                    suggestions_type: None,
                },
                CommandNode {
                    node_type: 1,
                    is_executable: false,
                    children: vec![2],
                    redirect_node: None,
                    name: Some("gamemode".into()),
                    parser: None,
                    suggestions_type: None,
                },
                CommandNode {
                    node_type: 2,
                    is_executable: true,
                    children: vec![],
                    redirect_node: None,
                    name: Some("mode".into()),
                    parser: Some(Parser::GameMode),
                    suggestions_type: None,
                },
                CommandNode {
                    node_type: 2,
                    is_executable: true,
                    children: vec![],
                    redirect_node: Some(1),
                    name: Some("team".into()),
                    parser: Some(Parser::String(StringType::SingleWord)),
                    suggestions_type: Some("minecraft:ask_server".into()),
                },
            ],
            root_index: 0,
        };
        let mut buf = Vec::new();
        tree.encode(&mut buf);

        let mut expected = vec![4];
        expected.extend([0x00, 1, 1]);
        expected.extend([0x01, 1, 2, 8]);
        expected.extend(b"gamemode");
        expected.extend([0x02 | 0x04, 0, 4]);
        expected.extend(b"mode");
        expected.push(42);
        expected.extend([0x02 | 0x04 | 0x08 | 0x10, 0, 1, 4]);
        expected.extend(b"team");
        expected.extend([5, 0, 20]);
        expected.extend(b"minecraft:ask_server");
        expected.push(0);
        assert_eq!(buf, expected);
    }
}

use voidmc_codec::{Decode, DecodeError, Encode, VarI32};

/// The type of string parsing expected by the client.
#[derive(Debug, Clone, Copy)]
pub enum StringType {
    /// Reads a single word.
    SingleWord = 0,
    /// Reads a quotable phrase (either a single word or a quoted string).
    QuotablePhrase = 1,
    /// Reads the rest of the input.
    GreedyPhrase = 2,
}

/// Argument parsers the Minecraft client understands.
#[derive(Debug, Clone)]
pub enum Parser {
    Bool,
    Float { min: Option<f32>, max: Option<f32> },
    Double { min: Option<f64>, max: Option<f64> },
    Integer { min: Option<i32>, max: Option<i32> },
    Long { min: Option<i64>, max: Option<i64> },
    String(StringType),
    Entity { single: bool, players_only: bool },
    GameProfile,
    Message,
}

impl Parser {
    fn parser_id(&self) -> i32 {
        match self {
            Parser::Bool => 0,
            Parser::Float { .. } => 1,
            Parser::Double { .. } => 2,
            Parser::Integer { .. } => 3,
            Parser::Long { .. } => 4,
            Parser::String(_) => 5,
            Parser::Entity { .. } => 6,
            Parser::GameProfile => 7,
            Parser::Message => 19,
        }
    }

    fn encode_properties(&self, buf: &mut Vec<u8>) {
        match self {
            Parser::Bool => {}
            Parser::Float { min, max } => {
                let flags = (min.is_some() as u8) | ((max.is_some() as u8) << 1);
                buf.push(flags);
                if let Some(v) = min {
                    buf.extend_from_slice(&v.to_be_bytes());
                }
                if let Some(v) = max {
                    buf.extend_from_slice(&v.to_be_bytes());
                }
            }
            Parser::Double { min, max } => {
                let flags = (min.is_some() as u8) | ((max.is_some() as u8) << 1);
                buf.push(flags);
                if let Some(v) = min {
                    buf.extend_from_slice(&v.to_be_bytes());
                }
                if let Some(v) = max {
                    buf.extend_from_slice(&v.to_be_bytes());
                }
            }
            Parser::Integer { min, max } => {
                let flags = (min.is_some() as u8) | ((max.is_some() as u8) << 1);
                buf.push(flags);
                if let Some(v) = min {
                    buf.extend_from_slice(&v.to_be_bytes());
                }
                if let Some(v) = max {
                    buf.extend_from_slice(&v.to_be_bytes());
                }
            }
            Parser::Long { min, max } => {
                let flags = (min.is_some() as u8) | ((max.is_some() as u8) << 1);
                buf.push(flags);
                if let Some(v) = min {
                    buf.extend_from_slice(&v.to_be_bytes());
                }
                if let Some(v) = max {
                    buf.extend_from_slice(&v.to_be_bytes());
                }
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
            Parser::GameProfile | Parser::Message => {}
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
    fn decode(_buf: &mut &[u8]) -> Result<Self, DecodeError> {
        // Server-only packet, no need to decode
        Err(DecodeError::InvalidLength)
    }
}

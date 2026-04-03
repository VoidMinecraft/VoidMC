pub mod defaults;
pub mod error;
pub mod flags;
pub mod parser;
pub mod plugin;

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use void_protocol::clientbound::commands::{CommandNode, Commands, Parser, StringType};

use crate::components::{ClientId, PlayerName, PlayerReady};
use crate::network::{NetworkChannels, OutgoingPacket};

pub use error::ParseError;
pub use flags::{FlagDefinition, FlagSet};
pub use parser::ArgParser;

// ---------------------------------------------------------------------------
// Argument & flag definitions
// ---------------------------------------------------------------------------

/// Describes one argument of a command — used for both parsing and protocol tree.
pub(crate) struct ArgumentDefinition {
    pub name: String,
    pub parser: Arc<dyn ArgParser>,
    pub required: bool,
    pub variadic: bool,
}

// ---------------------------------------------------------------------------
// Command & registered command
// ---------------------------------------------------------------------------

/// A registered command in the registry.
struct RegisteredCommand {
    name: String,
    description: String,
    aliases: Vec<String>,
    usage: Option<String>,
    arguments: Vec<ArgumentDefinition>,
    flag_definitions: Vec<FlagDefinition>,
    handler: Arc<dyn Fn(&mut CommandContext) + Send + Sync>,
}

/// The result of `CommandBuilder::build()` — a command ready to be registered.
pub struct Command {
    name: String,
    description: String,
    aliases: Vec<String>,
    usage: Option<String>,
    arguments: Vec<ArgumentDefinition>,
    flag_definitions: Vec<FlagDefinition>,
    handler: Arc<dyn Fn(&mut CommandContext) + Send + Sync>,
}

// ---------------------------------------------------------------------------
// CommandBuilder
// ---------------------------------------------------------------------------

/// Fluent API for building commands.
pub struct CommandBuilder {
    name: String,
    description: String,
    aliases: Vec<String>,
    usage: Option<String>,
    arguments: Vec<ArgumentDefinition>,
    flag_definitions: Vec<FlagDefinition>,
    handler: Option<Arc<dyn Fn(&mut CommandContext) + Send + Sync>>,
}

impl CommandBuilder {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            description: String::new(),
            aliases: Vec::new(),
            usage: None,
            arguments: Vec::new(),
            flag_definitions: Vec::new(),
            handler: None,
        }
    }

    pub fn description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    pub fn alias(mut self, alias: &str) -> Self {
        self.aliases.push(alias.to_string());
        self
    }

    /// Set a custom usage string. If not set, one is auto-generated from
    /// the argument and flag definitions.
    pub fn usage(mut self, usage: &str) -> Self {
        self.usage = Some(usage.to_string());
        self
    }

    /// Add a required typed argument.
    pub fn arg(mut self, name: &str, parser: Arc<dyn ArgParser>) -> Self {
        self.arguments.push(ArgumentDefinition {
            name: name.to_string(),
            parser,
            required: true,
            variadic: false,
        });
        self
    }

    /// Add an optional typed argument.
    pub fn arg_optional(mut self, name: &str, parser: Arc<dyn ArgParser>) -> Self {
        self.arguments.push(ArgumentDefinition {
            name: name.to_string(),
            parser,
            required: false,
            variadic: false,
        });
        self
    }

    /// Add a variadic argument (consumes all remaining tokens). Must be last.
    /// The argument is optional (zero or more tokens).
    pub fn arg_variadic(mut self, name: &str, parser: Arc<dyn ArgParser>) -> Self {
        self.arguments.push(ArgumentDefinition {
            name: name.to_string(),
            parser,
            required: false,
            variadic: true,
        });
        self
    }

    /// Add a variadic argument requiring at least one token. Must be last.
    pub fn arg_variadic_required(mut self, name: &str, parser: Arc<dyn ArgParser>) -> Self {
        self.arguments.push(ArgumentDefinition {
            name: name.to_string(),
            parser,
            required: true,
            variadic: true,
        });
        self
    }

    /// Add a boolean flag (e.g., `--verbose` / `-v`).
    pub fn flag(mut self, long: &str, short: Option<char>, description: &str) -> Self {
        self.flag_definitions.push(FlagDefinition {
            long: long.to_string(),
            short,
            description: description.to_string(),
            takes_value: false,
            value_parser: None,
        });
        self
    }

    /// Add a flag that takes a typed value (e.g., `--color red`).
    pub fn flag_value(
        mut self,
        long: &str,
        short: Option<char>,
        description: &str,
        parser: Arc<dyn ArgParser>,
    ) -> Self {
        self.flag_definitions.push(FlagDefinition {
            long: long.to_string(),
            short,
            description: description.to_string(),
            takes_value: true,
            value_parser: Some(parser),
        });
        self
    }

    pub fn handler(mut self, f: impl Fn(&mut CommandContext) + Send + Sync + 'static) -> Self {
        self.handler = Some(Arc::new(f));
        self
    }

    pub fn build(self) -> Command {
        // Validate: variadic must be last
        if let Some(pos) = self.arguments.iter().position(|a| a.variadic) {
            assert!(
                pos == self.arguments.len() - 1,
                "Variadic argument must be the last argument"
            );
        }

        Command {
            name: self.name,
            description: self.description,
            aliases: self.aliases,
            usage: self.usage,
            arguments: self.arguments,
            flag_definitions: self.flag_definitions,
            handler: self.handler.expect("Command must have a handler"),
        }
    }
}

// ---------------------------------------------------------------------------
// CommandContext — with typed access
// ---------------------------------------------------------------------------

/// Context passed to command handlers — provides helpers to interact with the world.
pub struct CommandContext<'a> {
    pub world: &'a mut World,
    pub entity: Entity,
    pub client_id: u32,
    pub args: Vec<String>,
    parsed_args: HashMap<String, Box<dyn Any + Send + Sync>>,
    flags: FlagSet,
}

impl<'a> CommandContext<'a> {
    /// Get a typed argument by name.
    pub fn get<T: 'static>(&self, name: &str) -> Option<&T> {
        self.parsed_args.get(name)?.downcast_ref::<T>()
    }

    /// Check if an optional argument was provided.
    pub fn has_arg(&self, name: &str) -> bool {
        self.parsed_args.contains_key(name)
    }

    /// Check if a boolean flag is set.
    pub fn flag(&self, name: &str) -> bool {
        self.flags.has(name)
    }

    /// Get a typed flag value.
    pub fn flag_value<T: 'static>(&self, name: &str) -> Option<&T> {
        self.flags.get_value::<T>(name)
    }

    /// Send a system message to the command sender.
    pub fn reply(&self, message: &str) {
        send_system_chat(self.world, self.client_id, message, "white");
    }

    /// Send an error message (red) to the command sender.
    pub fn reply_error(&self, message: &str) {
        send_system_chat(self.world, self.client_id, message, "red");
    }

    /// Broadcast a system message to all ready players.
    pub fn broadcast(&mut self, message: &str) {
        let channels = self.world.resource::<NetworkChannels>();
        let sender = channels.outgoing.clone();
        let ready_clients: Vec<u32> = self
            .world
            .query_filtered::<&ClientId, With<PlayerReady>>()
            .iter(self.world)
            .map(|c| c.0)
            .collect();

        let nbt = text_to_nbt(message, "white");
        let packet = void_protocol::clientbound::ClientboundPacket::Play(
            void_protocol::clientbound::PlayPacket::SystemChat(
                void_protocol::clientbound::SystemChat {
                    content: nbt,
                    overlay: false,
                },
            ),
        );

        for cid in ready_clients {
            let _ = sender.send(OutgoingPacket {
                client_id: cid,
                packet: packet.clone(),
            });
        }
    }

    /// Get the sender's player name.
    pub fn player_name(&self) -> Option<String> {
        self.world
            .get::<PlayerName>(self.entity)
            .map(|n| n.0.clone())
    }

    /// Check if the sender has the Operator component.
    pub fn is_operator(&self) -> bool {
        self.world
            .get::<crate::components::Operator>(self.entity)
            .is_some()
    }
}

pub(crate) fn send_system_chat(world: &World, client_id: u32, message: &str, color: &str) {
    let channels = world.resource::<NetworkChannels>();
    let nbt = text_to_nbt(message, color);
    let _ = channels.outgoing.send(OutgoingPacket {
        client_id,
        packet: void_protocol::clientbound::ClientboundPacket::Play(
            void_protocol::clientbound::PlayPacket::SystemChat(
                void_protocol::clientbound::SystemChat {
                    content: nbt,
                    overlay: false,
                },
            ),
        ),
    });
}

pub fn text_to_nbt(text: &str, color: &str) -> ussr_nbt::owned::Nbt {
    use ussr_nbt::owned::{Nbt, Tag};
    Nbt {
        name: "".into(),
        compound: vec![
            ("text".into(), Tag::String(text.into())),
            ("color".into(), Tag::String(color.into())),
        ]
        .into(),
    }
}

// ---------------------------------------------------------------------------
// Command registry
// ---------------------------------------------------------------------------

/// Internal resolve result — handler + definitions needed for the parsing pipeline.
struct ResolveResult {
    handler: Arc<dyn Fn(&mut CommandContext) + Send + Sync>,
    arguments: Vec<(String, Arc<dyn ArgParser>, bool, bool)>, // (name, parser, required, variadic)
    flag_definitions: Vec<FlagDefinition>,
    usage: String,
}

enum Resolved {
    Found(ResolveResult),
    NotFound(String),
}

/// ECS Resource holding all registered commands.
#[derive(Resource)]
pub struct CommandRegistry {
    commands: HashMap<String, RegisteredCommand>,
    aliases: HashMap<String, String>,
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
            aliases: HashMap::new(),
        }
    }

    /// Register a command built with `CommandBuilder`.
    pub fn register(&mut self, command: Command) {
        for alias in &command.aliases {
            self.aliases.insert(alias.clone(), command.name.clone());
        }
        self.commands.insert(
            command.name.clone(),
            RegisteredCommand {
                name: command.name,
                description: command.description,
                aliases: command.aliases,
                usage: command.usage,
                arguments: command.arguments,
                flag_definitions: command.flag_definitions,
                handler: command.handler,
            },
        );
    }

    /// Resolve a command name (or alias) to its canonical name.
    pub fn resolve<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        if self.commands.contains_key(name) {
            Some(name)
        } else {
            self.aliases.get(name).map(|s| s.as_str())
        }
    }

    /// Look up a command handler + definitions by name (or alias).
    /// Clones Arcs so the registry borrow can be dropped before invoking.
    fn resolve_handler(&self, name: &str) -> Resolved {
        let canonical = if self.commands.contains_key(name) {
            name
        } else if let Some(alias) = self.aliases.get(name) {
            alias.as_str()
        } else {
            return Resolved::NotFound(format!("Unknown command: /{}", name));
        };

        match self.commands.get(canonical) {
            Some(cmd) => {
                let args_info: Vec<_> = cmd
                    .arguments
                    .iter()
                    .map(|a| {
                        (
                            a.name.clone(),
                            Arc::clone(&a.parser),
                            a.required,
                            a.variadic,
                        )
                    })
                    .collect();

                // Rebuild flag definitions (clone the Arcs)
                let flag_defs: Vec<FlagDefinition> = cmd
                    .flag_definitions
                    .iter()
                    .map(|f| FlagDefinition {
                        long: f.long.clone(),
                        short: f.short,
                        description: f.description.clone(),
                        takes_value: f.takes_value,
                        value_parser: f.value_parser.as_ref().map(Arc::clone),
                    })
                    .collect();

                let usage = cmd.usage.clone().unwrap_or_else(|| {
                    auto_usage(&cmd.name, &cmd.arguments, &cmd.flag_definitions)
                });

                Resolved::Found(ResolveResult {
                    handler: Arc::clone(&cmd.handler),
                    arguments: args_info,
                    flag_definitions: flag_defs,
                    usage,
                })
            }
            None => Resolved::NotFound(format!("Unknown command: /{}", name)),
        }
    }

    /// Get all registered command names (canonical names only).
    pub fn command_names(&self) -> Vec<&str> {
        self.commands.keys().map(|s| s.as_str()).collect()
    }

    /// Get the description of a command.
    pub fn description(&self, name: &str) -> Option<&str> {
        let canonical = self.resolve(name)?;
        self.commands.get(canonical).map(|c| c.description.as_str())
    }

    /// Get the usage string of a command.
    pub fn usage(&self, name: &str) -> Option<String> {
        let canonical = self.resolve(name)?;
        self.commands.get(canonical).map(|c| {
            c.usage
                .clone()
                .unwrap_or_else(|| auto_usage(&c.name, &c.arguments, &c.flag_definitions))
        })
    }

    /// Build the clientbound Commands packet from the registry.
    pub fn build_command_tree(&self) -> Commands {
        // Node 0 = root
        let mut nodes: Vec<CommandNode> = vec![CommandNode {
            node_type: 0, // root
            is_executable: false,
            children: Vec::new(),
            redirect_node: None,
            name: None,
            parser: None,
            suggestions_type: None,
        }];

        let mut root_children: Vec<i32> = Vec::new();

        for cmd in self.commands.values() {
            let cmd_node_index = nodes.len() as i32;

            // Build argument chain for this command
            let mut arg_indices: Vec<i32> = Vec::new();
            for _arg in &cmd.arguments {
                let arg_index = nodes.len() as i32 + arg_indices.len() as i32 + 1;
                arg_indices.push(arg_index);
            }

            // The literal node for the command name
            let is_executable =
                cmd.arguments.is_empty() || cmd.arguments.iter().all(|a| !a.required);
            let children = if arg_indices.is_empty() {
                Vec::new()
            } else {
                vec![arg_indices[0]]
            };

            nodes.push(CommandNode {
                node_type: 1, // literal
                is_executable,
                children,
                redirect_node: None,
                name: Some(cmd.name.clone()),
                parser: None,
                suggestions_type: None,
            });
            root_children.push(cmd_node_index);

            // Add argument nodes
            for (i, arg) in cmd.arguments.iter().enumerate() {
                let next_children = if i + 1 < cmd.arguments.len() {
                    vec![arg_indices[i + 1]]
                } else {
                    Vec::new()
                };

                let is_exec = i + 1 == cmd.arguments.len()
                    || cmd.arguments[i + 1..].iter().all(|a| !a.required);

                let protocol_parser = arg
                    .parser
                    .protocol_parser()
                    .unwrap_or(Parser::String(StringType::SingleWord));

                let suggestions = arg.parser.suggestions_type().map(|s| s.to_string());

                nodes.push(CommandNode {
                    node_type: 2, // argument
                    is_executable: is_exec,
                    children: next_children,
                    redirect_node: None,
                    name: Some(arg.name.clone()),
                    parser: Some(protocol_parser),
                    suggestions_type: suggestions,
                });
            }

            // Add alias literal nodes pointing to the same argument chain
            for alias in &cmd.aliases {
                let alias_node_index = nodes.len() as i32;
                let children = if arg_indices.is_empty() {
                    Vec::new()
                } else {
                    vec![arg_indices[0]]
                };

                nodes.push(CommandNode {
                    node_type: 1, // literal
                    is_executable,
                    children,
                    redirect_node: None,
                    name: Some(alias.clone()),
                    parser: None,
                    suggestions_type: None,
                });
                root_children.push(alias_node_index);
            }
        }

        // Update root node children
        nodes[0].children = root_children;

        Commands {
            nodes,
            root_index: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Auto-generated usage string
// ---------------------------------------------------------------------------

fn auto_usage(name: &str, arguments: &[ArgumentDefinition], flags: &[FlagDefinition]) -> String {
    let mut parts = vec![format!("/{}", name)];

    for arg in arguments {
        let type_name = arg.parser.type_name();
        if arg.variadic && arg.required {
            parts.push(format!("<{}:{}>...", arg.name, type_name));
        } else if arg.variadic {
            parts.push(format!("[{}:{}]...", arg.name, type_name));
        } else if arg.required {
            parts.push(format!("<{}:{}>", arg.name, type_name));
        } else {
            parts.push(format!("[{}:{}]", arg.name, type_name));
        }
    }

    for flag in flags {
        let short = flag.short.map(|c| format!("-{}/", c)).unwrap_or_default();
        if flag.takes_value {
            parts.push(format!("[{}--{} <value>]", short, flag.long));
        } else {
            parts.push(format!("[{}--{}]", short, flag.long));
        }
    }

    parts.join(" ")
}

// ---------------------------------------------------------------------------
// Parsing pipeline
// ---------------------------------------------------------------------------

/// Parse positional tokens against argument definitions.
fn parse_positional(
    tokens: &[String],
    definitions: &[(String, Arc<dyn ArgParser>, bool, bool)], // (name, parser, required, variadic)
) -> Result<HashMap<String, Box<dyn Any + Send + Sync>>, Vec<ParseError>> {
    let mut parsed = HashMap::new();
    let mut errors = Vec::new();
    let mut token_idx = 0;

    for (name, parser, required, variadic) in definitions {
        if *variadic {
            // Consume all remaining tokens, joined by spaces
            if token_idx < tokens.len() {
                let remaining = tokens[token_idx..].join(" ");
                match parser.parse(&remaining) {
                    Ok(val) => {
                        parsed.insert(name.clone(), val);
                    }
                    Err(detail) => {
                        errors.push(ParseError::InvalidValue {
                            name: name.clone(),
                            value: remaining,
                            expected: parser.type_name().to_string(),
                            detail: Some(detail),
                        });
                    }
                }
                token_idx = tokens.len(); // consumed all
            } else if *required {
                errors.push(ParseError::MissingArgument {
                    name: name.clone(),
                    expected_type: parser.type_name().to_string(),
                });
            }
        } else if token_idx < tokens.len() {
            let token = &tokens[token_idx];
            match parser.parse(token) {
                Ok(val) => {
                    parsed.insert(name.clone(), val);
                }
                Err(detail) => {
                    errors.push(ParseError::InvalidValue {
                        name: name.clone(),
                        value: token.clone(),
                        expected: parser.type_name().to_string(),
                        detail: Some(detail),
                    });
                }
            }
            token_idx += 1;
        } else if *required {
            errors.push(ParseError::MissingArgument {
                name: name.clone(),
                expected_type: parser.type_name().to_string(),
            });
        }
    }

    // Check for excess tokens (only if no variadic arg exists)
    let has_variadic = definitions.iter().any(|(_, _, _, v)| *v);
    if !has_variadic && token_idx < tokens.len() {
        errors.push(ParseError::TooManyArguments {
            expected: definitions.len(),
            got: tokens.len(),
        });
    }

    if errors.is_empty() {
        Ok(parsed)
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Resolve and execute a command with the full parsing pipeline.
pub fn dispatch_command(
    world: &mut World,
    client_id: u32,
    entity: Entity,
    command_name: &str,
    args: Vec<String>,
) {
    // Step 1: borrow registry immutably to clone handler + definitions
    let resolved = world
        .resource::<CommandRegistry>()
        .resolve_handler(command_name);

    // Step 2: registry borrow is dropped — we now have full &mut World
    match resolved {
        Resolved::Found(res) => {
            // Flag extraction pre-pass
            let (positional, flags, flag_errors) =
                flags::extract_flags(&args, &res.flag_definitions);

            if !flag_errors.is_empty() {
                for err in &flag_errors {
                    send_system_chat(world, client_id, &err.to_player_message(), "red");
                }
                send_system_chat(world, client_id, &format!("Usage: {}", res.usage), "gray");
                return;
            }

            // Positional argument parsing
            match parse_positional(&positional, &res.arguments) {
                Ok(parsed_args) => {
                    let mut ctx = CommandContext {
                        world,
                        entity,
                        client_id,
                        args,
                        parsed_args,
                        flags,
                    };
                    (res.handler)(&mut ctx);
                }
                Err(errors) => {
                    for err in &errors {
                        send_system_chat(world, client_id, &err.to_player_message(), "red");
                    }
                    send_system_chat(world, client_id, &format!("Usage: {}", res.usage), "gray");
                }
            }
        }
        Resolved::NotFound(err) => {
            send_system_chat(world, client_id, &err, "red");
        }
    }
}

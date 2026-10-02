pub mod coordinates;
pub mod defaults;
pub mod error;
pub mod flags;
pub mod parser;
pub mod plugin;
pub mod selector;

use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use bevy_ecs::prelude::*;
use voidmc_protocol::clientbound::commands::{CommandNode, Commands, Parser, StringType};

use crate::components::PlayerName;
use crate::messages::{TextColor, WorldMessages};
use crate::players::WorldPlayers;

pub use error::ParseError;
pub use flags::{FlagDefinition, FlagSet, extract_flags_before};
pub use parser::{ArgParser, ParseContext};

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
// Command
// ---------------------------------------------------------------------------

type Handler = Arc<dyn Fn(&mut CommandContext) + Send + Sync>;
type Requirement = Arc<dyn Fn(&World, Entity) -> bool + Send + Sync>;

/// The result of `CommandBuilder::build()` — a command ready to be registered.
pub struct Command {
    name: String,
    description: String,
    aliases: Vec<String>,
    usage: Option<String>,
    arguments: Vec<ArgumentDefinition>,
    flag_definitions: Vec<FlagDefinition>,
    handler: Option<Handler>,
    subcommands: Vec<Command>,
    requirement: Option<Requirement>,
    suggest_entity_types: bool,
}

impl Command {
    fn matches(&self, literal: &str) -> bool {
        self.name == literal || self.aliases.iter().any(|alias| alias == literal)
    }

    fn subcommand(&self, literal: &str) -> Option<&Command> {
        self.subcommands.iter().find(|sub| sub.matches(literal))
    }

    fn argument_tokens(&self) -> usize {
        self.arguments.iter().map(|a| a.parser.token_count()).sum()
    }

    fn subcommand_names(&self) -> String {
        self.subcommands
            .iter()
            .map(|sub| sub.name.as_str())
            .collect::<Vec<_>>()
            .join("|")
    }
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
    handler: Option<Handler>,
    subcommands: Vec<Command>,
    requirement: Option<Requirement>,
    suggest_entity_types: bool,
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
            subcommands: Vec::new(),
            requirement: None,
            suggest_entity_types: false,
        }
    }

    /// Mark this command as accepting entity type names as a target argument.
    /// The suggestion handler will include `minecraft:*` entity type names in tab-completion.
    pub fn suggest_entity_types(mut self) -> Self {
        self.suggest_entity_types = true;
        self
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

    /// Add a literal branch tried after this command's arguments, like
    /// `add` in `/team add <team>`. The subcommand's arguments are parsed
    /// after the parent's, and both are read from the same context.
    pub fn subcommand(mut self, subcommand: CommandBuilder) -> Self {
        self.subcommands.push(subcommand.build());
        self
    }

    /// Who may run this command (and its subcommands): checked before any
    /// argument is parsed or completed, like brigadier's `requires`.
    pub fn requires(
        mut self,
        requirement: impl Fn(&World, Entity) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.requirement = Some(Arc::new(requirement));
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
        assert!(
            self.handler.is_some() || !self.subcommands.is_empty(),
            "Command must have a handler or subcommands"
        );
        assert!(
            self.subcommands.is_empty() || self.arguments.iter().all(|a| a.required && !a.variadic),
            "Arguments before subcommands must be required and not variadic"
        );

        Command {
            name: self.name,
            description: self.description,
            aliases: self.aliases,
            usage: self.usage,
            arguments: self.arguments,
            flag_definitions: self.flag_definitions,
            handler: self.handler,
            subcommands: self.subcommands,
            requirement: self.requirement,
            suggest_entity_types: self.suggest_entity_types,
        }
    }
}

// ---------------------------------------------------------------------------
// CommandContext — with typed access
// ---------------------------------------------------------------------------

/// Context passed to command handlers — provides helpers to interact with the world.
pub struct CommandContext<'a> {
    world: &'a mut World,
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

    /// Read-only world access for advanced command handlers.
    pub fn with_world<R>(&self, f: impl FnOnce(&World) -> R) -> R {
        f(self.world)
    }

    /// Mutable world access for advanced command handlers.
    ///
    /// Prefer using dedicated helper methods when possible.
    pub fn with_world_mut<R>(&mut self, f: impl FnOnce(&mut World) -> R) -> R {
        f(self.world)
    }

    /// Send a system message to the command sender.
    pub fn reply(&self, message: &str) {
        WorldMessages::new(self.world)
            .message(self.entity, message)
            .send();
    }

    /// Send an error message (red) to the command sender.
    pub fn reply_error(&self, message: &str) {
        WorldMessages::new(self.world)
            .message(self.entity, message)
            .color(TextColor::Red)
            .send();
    }

    /// Broadcast a system message to all ready players.
    pub fn broadcast(&mut self, message: &str) {
        WorldMessages::new(self.world).broadcast(message).send();
    }

    pub fn players(&self) -> WorldPlayers<'_> {
        WorldPlayers::new(self.world)
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

pub fn system_chat(message: &str, color: &str) -> voidmc_protocol::clientbound::SystemChat {
    voidmc_protocol::clientbound::SystemChat {
        content: text_to_nbt(message, color),
        overlay: false,
    }
}

pub(crate) fn send_system_chat(world: &World, player: Entity, message: &str, color: TextColor) {
    WorldMessages::new(world)
        .message(player, message)
        .color(color)
        .send();
}

/// Invalid colours fall back to white; prefer [`crate::messages::text_component`].
pub fn text_to_nbt(text: &str, color: &str) -> ussr_nbt::owned::Nbt {
    crate::messages::text_component(text, TextColor::parse_or_white(color))
}

// ---------------------------------------------------------------------------
// Command registry
// ---------------------------------------------------------------------------

/// Internal resolve result — handler + definitions needed for the parsing pipeline.
struct ResolveResult {
    handler: Option<Handler>,
    arguments: Vec<(String, Arc<dyn ArgParser>, bool, bool)>, // (name, parser, required, variadic)
    flag_definitions: Vec<FlagDefinition>,
    usage: String,
    tokens: Vec<String>,
    unknown_subcommand: Option<String>,
    subcommands: String,
    requirements: Vec<Requirement>,
    positional_limit: Option<usize>,
}

enum Resolved {
    Found(ResolveResult),
    NotFound(String),
}

/// The subcommands a token list selects, starting from a registered command.
struct Walk<'a> {
    path: Vec<&'a Command>,
    literals: Vec<usize>,
    unknown: Option<usize>,
}

impl<'a> Walk<'a> {
    fn new<S: AsRef<str>>(root: &'a Command, tokens: &[S]) -> Self {
        let mut walk = Walk {
            path: vec![root],
            literals: Vec::new(),
            unknown: None,
        };
        let mut index = 0;
        let mut flags_allowed = true;
        loop {
            let node = walk.node();
            if node.subcommands.is_empty() {
                break;
            }
            let flags = walk.flags();
            let mut needed = node.argument_tokens();
            while index < tokens.len() {
                if flags_allowed && !flags.is_empty() && tokens[index].as_ref() == "--" {
                    flags_allowed = false;
                    index += 1;
                } else if let Some(next) = flags_allowed
                    .then(|| skip_flag(tokens, index, &flags))
                    .flatten()
                {
                    index = next;
                } else if needed > 0 {
                    needed -= 1;
                    index += 1;
                } else {
                    break;
                }
            }
            let Some(token) = tokens.get(index) else {
                break;
            };
            match node.subcommand(token.as_ref()) {
                Some(subcommand) => {
                    walk.literals.push(index);
                    walk.path.push(subcommand);
                    index += 1;
                }
                None => {
                    walk.unknown = Some(index);
                    break;
                }
            }
        }
        walk
    }

    fn node(&self) -> &'a Command {
        self.path[self.path.len() - 1]
    }

    fn flags(&self) -> Vec<FlagDefinition> {
        self.path
            .iter()
            .flat_map(|command| command.flag_definitions.iter().cloned())
            .collect()
    }

    fn permits(&self, world: &World, executor: Entity) -> bool {
        self.path.iter().all(|command| {
            command
                .requirement
                .as_ref()
                .is_none_or(|requirement| requirement(world, executor))
        })
    }

    fn start(&self) -> usize {
        self.literals.last().map_or(0, |index| index + 1)
    }

    fn usage(&self) -> String {
        self.node()
            .usage
            .clone()
            .unwrap_or_else(|| auto_usage(&self.path))
    }
}

fn skip_flag<S: AsRef<str>>(
    tokens: &[S],
    index: usize,
    definitions: &[FlagDefinition],
) -> Option<usize> {
    let token = tokens[index].as_ref();
    if is_combined_short_flags(token, definitions) {
        return Some(index + 1);
    }
    definitions
        .iter()
        .find(|definition| definition.matches_token(token))
        .map(|definition| index + 1 + definition.takes_value as usize)
}

fn append_flag_branch(
    nodes: &mut Vec<CommandNode>,
    parent_index: i32,
    literal: String,
    definition: &FlagDefinition,
) {
    let literal_index = nodes.len() as i32;
    nodes[parent_index as usize].children.push(literal_index);

    if definition.takes_value {
        let value_index = literal_index + 1;
        nodes.push(CommandNode {
            node_type: 1,
            is_executable: false,
            children: vec![value_index],
            redirect_node: None,
            name: Some(literal),
            parser: None,
            suggestions_type: None,
        });

        let protocol_parser = definition
            .value_parser
            .as_ref()
            .and_then(|parser| parser.protocol_parser())
            .unwrap_or(Parser::String(StringType::SingleWord));
        let suggestions_type = definition
            .value_parser
            .as_ref()
            .and_then(|parser| parser.suggestions_type())
            .map(str::to_string);

        nodes.push(CommandNode {
            node_type: 2,
            is_executable: true,
            children: Vec::new(),
            redirect_node: Some(parent_index),
            name: Some(definition.long.clone()),
            parser: Some(protocol_parser),
            suggestions_type,
        });
    } else {
        nodes.push(CommandNode {
            node_type: 1,
            is_executable: true,
            children: Vec::new(),
            redirect_node: Some(parent_index),
            name: Some(literal),
            parser: None,
            suggestions_type: None,
        });
    }
}

fn append_flag_branches(
    nodes: &mut Vec<CommandNode>,
    parent_index: i32,
    definitions: &[FlagDefinition],
) {
    for definition in definitions {
        append_flag_branch(
            nodes,
            parent_index,
            format!("--{}", definition.long),
            definition,
        );
        if let Some(short) = definition.short {
            append_flag_branch(nodes, parent_index, format!("-{short}"), definition);
        }
    }
}

fn literal_node(name: &str, is_executable: bool, children: Vec<i32>) -> CommandNode {
    CommandNode {
        node_type: 1,
        is_executable,
        children,
        redirect_node: None,
        name: Some(name.to_string()),
        parser: None,
        suggestions_type: None,
    }
}

/// Appends `command`'s literal, argument chain and subcommands; returns the
/// literal node indices (name then aliases) for the parent to adopt.
fn append_command(
    nodes: &mut Vec<CommandNode>,
    command: &Command,
    inherited_flags: &[FlagDefinition],
) -> Vec<i32> {
    let flags: Vec<FlagDefinition> = inherited_flags
        .iter()
        .chain(&command.flag_definitions)
        .cloned()
        .collect();
    let runnable = command.handler.is_some();
    let executable_from =
        |index: usize| runnable && command.arguments[index..].iter().all(|a| !a.required);

    let literal_index = nodes.len() as i32;
    let literal_executable = executable_from(0);
    nodes.push(literal_node(&command.name, literal_executable, Vec::new()));
    let mut flag_parents = Vec::new();
    if literal_executable {
        flag_parents.push(literal_index);
    }

    let mut tail = literal_index;
    for (i, arg) in command.arguments.iter().enumerate() {
        let index = nodes.len() as i32;
        nodes[tail as usize].children.push(index);
        let is_executable = executable_from(i + 1);
        nodes.push(CommandNode {
            node_type: 2,
            is_executable,
            children: Vec::new(),
            redirect_node: None,
            name: Some(arg.name.clone()),
            parser: Some(
                arg.parser
                    .protocol_parser()
                    .unwrap_or(Parser::String(StringType::SingleWord)),
            ),
            suggestions_type: arg.parser.suggestions_type().map(str::to_string),
        });
        if is_executable {
            flag_parents.push(index);
        }
        tail = index;
    }

    for subcommand in &command.subcommands {
        let literals = append_command(nodes, subcommand, &flags);
        nodes[tail as usize].children.extend(literals);
    }

    let mut literals = vec![literal_index];
    for alias in &command.aliases {
        let index = nodes.len() as i32;
        let children = nodes[literal_index as usize].children.clone();
        nodes.push(literal_node(alias, literal_executable, children));
        if literal_executable {
            flag_parents.push(index);
        }
        literals.push(index);
    }

    for parent_index in flag_parents {
        append_flag_branches(nodes, parent_index, &flags);
    }
    literals
}

/// Suggestions for the token starting at byte `start` of the chat line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub start: usize,
    pub length: usize,
    pub matches: Vec<String>,
}

fn flag_completions(definitions: &[FlagDefinition], partial: &str) -> Vec<String> {
    definitions
        .iter()
        .flat_map(|definition| {
            std::iter::once(format!("--{}", definition.long))
                .chain(definition.short.map(|short| format!("-{short}")))
        })
        .filter(|flag| flag.starts_with(partial))
        .collect()
}

fn positional_count(tokens: &[&str], definitions: &[FlagDefinition]) -> usize {
    let mut count = 0;
    let mut index = 0;
    let mut flags_allowed = true;
    while index < tokens.len() {
        let token = tokens[index];
        index += 1;
        if flags_allowed && !definitions.is_empty() && token == "--" {
            flags_allowed = false;
            continue;
        }
        let flag = flags_allowed
            .then(|| {
                definitions
                    .iter()
                    .find(|definition| definition.matches_token(token))
            })
            .flatten();
        match flag {
            Some(definition) => {
                if definition.takes_value {
                    index += 1;
                }
            }
            None if flags_allowed && is_combined_short_flags(token, definitions) => {}
            None => count += 1,
        }
    }
    count
}

fn is_combined_short_flags(token: &str, definitions: &[FlagDefinition]) -> bool {
    let Some(shorts) = token.strip_prefix('-') else {
        return false;
    };
    shorts.len() > 1
        && shorts.chars().all(|c| {
            definitions
                .iter()
                .any(|definition| definition.short == Some(c) && !definition.takes_value)
        })
}

fn argument_at(arguments: &[ArgumentDefinition], positional: usize) -> Option<&ArgumentDefinition> {
    let mut offset = 0;
    for arg in arguments {
        if arg.variadic {
            return Some(arg);
        }
        offset += arg.parser.token_count();
        if positional < offset {
            return Some(arg);
        }
    }
    None
}

/// ECS Resource holding all registered commands.
#[derive(Resource)]
pub struct CommandRegistry {
    commands: HashMap<String, Command>,
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
        self.commands.insert(command.name.clone(), command);
    }

    /// Returns `true` if the canonical command was built with `.suggest_entity_types()`.
    pub fn accepts_entity_type_arg(&self, canonical_name: &str) -> bool {
        self.commands
            .get(canonical_name)
            .is_some_and(|cmd| cmd.suggest_entity_types)
    }

    /// Server-side tab-completion for a `minecraft:ask_server` request
    /// carrying the full chat line (`/tp @`), or `None` when the line does
    /// not name a registered command with at least one argument typed.
    pub fn complete(&self, text: &str, world: &World, executor: Entity) -> Option<Completion> {
        let line = text.strip_prefix('/').unwrap_or(text);
        let (name, rest) = line.split_once(' ')?;
        let cmd = self.commands.get(self.resolve(name)?)?;

        let tokens: Vec<&str> = rest.split_whitespace().collect();
        let (partial, completed) = if rest.ends_with(char::is_whitespace) || tokens.is_empty() {
            ("", tokens.as_slice())
        } else {
            (tokens[tokens.len() - 1], &tokens[..tokens.len() - 1])
        };
        let partial_start = text
            .rfind(partial)
            .filter(|&index| text.is_char_boundary(index))
            .unwrap_or(text.len());

        let walk = Walk::new(cmd, completed);
        let node = walk.node();
        let flags = walk.flags();
        let mut matches = if walk.unknown.is_some() || !walk.permits(world, executor) {
            Vec::new()
        } else if partial.starts_with('-') && !flags.is_empty() {
            flag_completions(&flags, partial)
        } else {
            let positional = positional_count(&completed[walk.start()..], &flags);
            let mut matches =
                if !node.subcommands.is_empty() && positional >= node.argument_tokens() {
                    node.subcommands
                        .iter()
                        .flat_map(|sub| std::iter::once(&sub.name).chain(&sub.aliases))
                        .filter(|literal| parser::starts_with_ignore_case(literal, partial))
                        .cloned()
                        .collect()
                } else {
                    argument_at(&node.arguments, positional)
                        .map(|arg| arg.parser.suggestions(partial, world))
                        .unwrap_or_default()
                };
            if cmd.suggest_entity_types {
                matches.extend(
                    voidmc_data::entity_type_names(voidmc_data::Version::V26_1_2)
                        .into_iter()
                        .filter(|entity_type| parser::starts_with_ignore_case(entity_type, partial))
                        .map(str::to_string),
                );
            }
            matches
        };
        let mut seen = std::collections::HashSet::new();
        matches.retain(|candidate| seen.insert(candidate.clone()));

        Some(Completion {
            start: text[..partial_start].encode_utf16().count(),
            length: partial.encode_utf16().count(),
            matches,
        })
    }

    /// Resolve a command name (or alias) to its canonical name.
    pub fn resolve<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        if self.commands.contains_key(name) {
            Some(name)
        } else {
            self.aliases.get(name).map(|s| s.as_str())
        }
    }

    /// Look up the handler + definitions selected by a command name (or
    /// alias) and its tokens. Clones Arcs so the registry borrow can be
    /// dropped before invoking.
    fn resolve_handler(&self, name: &str, args: &[String]) -> Resolved {
        let Some(cmd) = self.resolve(name).and_then(|name| self.commands.get(name)) else {
            return Resolved::NotFound(format!("Unknown command: /{}", name));
        };
        let walk = Walk::new(cmd, args);
        let node = walk.node();
        let arguments = walk
            .path
            .iter()
            .flat_map(|command| &command.arguments)
            .map(|a| {
                (
                    a.name.clone(),
                    Arc::clone(&a.parser),
                    a.required,
                    a.variadic,
                )
            })
            .collect();
        let tokens = args
            .iter()
            .enumerate()
            .filter(|(index, _)| !walk.literals.contains(index))
            .map(|(_, token)| token.clone())
            .collect();
        let subcommands = node.subcommand_names();
        Resolved::Found(ResolveResult {
            handler: node.handler.clone(),
            arguments,
            flag_definitions: walk.flags(),
            usage: walk.usage(),
            tokens,
            unknown_subcommand: walk.unknown.map(|index| {
                format!(
                    "Unknown subcommand '{}' (expected {})",
                    args[index], subcommands
                )
            }),
            subcommands,
            requirements: walk
                .path
                .iter()
                .filter_map(|command| command.requirement.clone())
                .collect(),
            positional_limit: node.arguments.last().filter(|arg| arg.variadic).map(|_| {
                walk.path
                    .iter()
                    .flat_map(|command| &command.arguments)
                    .filter(|arg| !arg.variadic)
                    .map(|arg| arg.parser.token_count())
                    .sum()
            }),
        })
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
        self.commands
            .get(canonical)
            .map(|c| c.usage.clone().unwrap_or_else(|| auto_usage(&[c])))
    }

    /// Build the clientbound Commands packet from the registry.
    pub fn build_command_tree(&self) -> Commands {
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
            root_children.extend(append_command(&mut nodes, cmd, &[]));
        }
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

fn auto_usage(path: &[&Command]) -> String {
    let mut parts = vec![format!("/{}", path[0].name)];

    for (depth, command) in path.iter().enumerate() {
        if depth > 0 {
            parts.push(command.name.clone());
        }
        for arg in &command.arguments {
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
    }

    let node = path[path.len() - 1];
    if !node.subcommands.is_empty() {
        let names = node.subcommand_names();
        if node.handler.is_some() {
            parts.push(format!("[{names}]"));
        } else {
            parts.push(format!("<{names}>"));
        }
    }

    for flag in path.iter().flat_map(|command| &command.flag_definitions) {
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
    ctx: &ParseContext<'_>,
) -> Result<HashMap<String, Box<dyn Any + Send + Sync>>, Vec<ParseError>> {
    let mut parsed = HashMap::new();
    let mut errors = Vec::new();
    let mut token_idx = 0;

    for (name, parser, required, variadic) in definitions {
        if *variadic {
            // Consume all remaining tokens, joined by spaces
            if token_idx < tokens.len() {
                let remaining = tokens[token_idx..].join(" ");
                match parser.parse_in(&remaining, ctx) {
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
            let end = (token_idx + parser.token_count()).min(tokens.len());
            let input = tokens[token_idx..end].join(" ");
            match parser.parse_in(&input, ctx) {
                Ok(val) => {
                    parsed.insert(name.clone(), val);
                }
                Err(detail) => {
                    errors.push(ParseError::InvalidValue {
                        name: name.clone(),
                        value: input,
                        expected: parser.type_name().to_string(),
                        detail: Some(detail),
                    });
                }
            }
            token_idx = end;
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
            expected: definitions
                .iter()
                .map(|(_, parser, _, _)| parser.token_count())
                .sum(),
            got: tokens.len(),
        });
    }

    if errors.is_empty() {
        Ok(parsed)
    } else {
        Err(errors)
    }
}

#[derive(Debug, Clone)]
pub struct QueuedCommand {
    pub client_id: u32,
    pub entity: Entity,
    pub command: String,
    pub args: Vec<String>,
    pub sequence: u64,
}

#[derive(Resource, Default)]
pub struct CommandQueue(pub VecDeque<QueuedCommand>);

#[derive(Resource, Default)]
pub struct CommandEnqueueSequence(pub u64);

pub fn enqueue_command(
    queue: &mut CommandQueue,
    sequence: &mut CommandEnqueueSequence,
    client_id: u32,
    entity: Entity,
    command: String,
    args: Vec<String>,
) {
    let next_sequence = sequence.0;
    sequence.0 = sequence.0.wrapping_add(1);

    queue.0.push_back(QueuedCommand {
        client_id,
        entity,
        command,
        args,
        sequence: next_sequence,
    });
}

pub fn drain_command_queue(world: &mut World) {
    loop {
        let queued = {
            let mut queue = world.resource_mut::<CommandQueue>();
            queue.0.pop_front()
        };

        let Some(queued) = queued else {
            break;
        };

        if !world.entities().contains(queued.entity) {
            tracing::debug!(
                "Dropping queued command '{}' for despawned entity {:?}",
                queued.command,
                queued.entity
            );
            continue;
        }

        dispatch_command(
            world,
            queued.client_id,
            queued.entity,
            &queued.command,
            queued.args,
        );

        // Preserve command-to-command ordering for deferred side effects.
        world.flush();
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
    tracing::info!(
        "[DISPATCH_COMMAND_CALLED] command='{}', args={:?}",
        command_name,
        args
    );
    // Step 1: borrow registry immutably to clone handler + definitions
    let resolved = world
        .resource::<CommandRegistry>()
        .resolve_handler(command_name, &args);

    // Step 2: registry borrow is dropped — we now have full &mut World
    let res = match resolved {
        Resolved::Found(res) => res,
        Resolved::NotFound(err) => {
            send_system_chat(world, entity, &err, TextColor::Red);
            return;
        }
    };
    let fail = |world: &World, errors: &[String]| {
        for err in errors {
            send_system_chat(world, entity, err, TextColor::Red);
        }
        send_system_chat(
            world,
            entity,
            &format!("Usage: {}", res.usage),
            TextColor::Gray,
        );
    };
    if !res
        .requirements
        .iter()
        .all(|requirement| requirement(world, entity))
    {
        send_system_chat(
            world,
            entity,
            "You do not have permission to use this command",
            TextColor::Red,
        );
        return;
    }
    if let Some(err) = &res.unknown_subcommand {
        fail(world, std::slice::from_ref(err));
        return;
    }

    let ctx = ParseContext {
        world,
        executor: entity,
    };
    let (positional, flags, flag_errors) = flags::extract_flags_before(
        &res.tokens,
        &res.flag_definitions,
        &ctx,
        res.positional_limit,
    );
    if !flag_errors.is_empty() {
        let errors: Vec<String> = flag_errors.iter().map(|e| e.to_player_message()).collect();
        fail(world, &errors);
        return;
    }

    match parse_positional(&positional, &res.arguments, &ctx) {
        Ok(parsed_args) => {
            let Some(handler) = &res.handler else {
                fail(
                    world,
                    &[format!("Missing subcommand (expected {})", res.subcommands)],
                );
                return;
            };
            let mut ctx = CommandContext {
                world,
                entity,
                client_id,
                args,
                parsed_args,
                flags,
            };
            handler(&mut ctx);
        }
        Err(errors) => {
            let errors: Vec<String> = errors.iter().map(|e| e.to_player_message()).collect();
            fail(world, &errors);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::defaults::{gamemode_command, summon_command, tp_command};
    use crate::commands::parser::{EnumArg, GameProfileArg, PlayerArg, StringArg, TimeArg};
    use crate::components::{PlayerName, PlayerReady};
    use voidmc_codec::Encode;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Team {
        Red,
        Blue,
    }

    fn team_command() -> Command {
        CommandBuilder::new("team")
            .alias("t")
            .arg("player", Arc::new(GameProfileArg))
            .arg(
                "team",
                EnumArg::new([("red", Team::Red), ("blue", Team::Blue)]),
            )
            .arg_optional("for", TimeArg::non_negative())
            .flag("silent", Some('s'), "No announcement")
            .flag_value("reason", Some('r'), "Why", StringArg::single_word())
            .handler(|_| {})
            .build()
    }

    fn registry_with(commands: impl IntoIterator<Item = Command>) -> CommandRegistry {
        let mut registry = CommandRegistry::new();
        for command in commands {
            registry.register(command);
        }
        registry
    }

    fn player_world() -> World {
        let mut world = World::new();
        world.spawn((PlayerName("Alice".into()), PlayerReady));
        world.spawn((PlayerName("Bob".into()), PlayerReady));
        world
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle)
    }

    fn encoded_argument_node(tree: &Commands, name: &str) -> Vec<u8> {
        let node = tree
            .nodes
            .iter()
            .find(|node| node.node_type == 2 && node.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing argument node {name}"));
        let single = Commands {
            nodes: vec![node.clone()],
            root_index: 0,
        };
        let mut buf = Vec::new();
        single.encode(&mut buf);
        buf[1..buf.len() - 1].to_vec()
    }

    #[test]
    fn completion_routes_ask_server_to_the_argument_parser() {
        let registry = registry_with([team_command()]);
        let world = player_world();

        let players = registry
            .complete("/team ", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(players.start, 6);
        assert_eq!(players.length, 0);
        assert_eq!(
            players.matches,
            vec!["Alice".to_string(), "Bob".to_string()]
        );

        let partial = registry
            .complete("/t bo", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(
            partial,
            Completion {
                start: 3,
                length: 2,
                matches: vec!["Bob".to_string()],
            }
        );

        let teams = registry
            .complete("/team Bob b", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(teams.start, 10);
        assert_eq!(teams.matches, vec!["blue".to_string()]);

        let time = registry
            .complete("/team Bob red 1", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert!(time.matches.is_empty());
        assert_eq!(time.start, 14);
    }

    #[test]
    fn completion_skips_flags_and_their_values_when_counting_positionals() {
        let registry = registry_with([team_command()]);
        let world = player_world();

        let teams = registry
            .complete("/team --silent -r why Bob r", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(teams.matches, vec!["red".to_string()]);

        let flags = registry
            .complete("/team Bob red --", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(
            flags.matches,
            vec!["--silent".to_string(), "--reason".to_string()]
        );
        let short = registry
            .complete("/team Bob red -s", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(short.matches, vec!["-s".to_string()]);

        let after_stop = registry
            .complete("/team -- Bob r", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(after_stop.matches, vec!["red".to_string()]);
    }

    #[test]
    fn completion_range_is_in_utf16_code_units() {
        let registry = registry_with([team_command()]);
        let world = player_world();

        let after_accent = registry
            .complete("/team Émile ", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(after_accent.start, 12);
        assert_eq!(after_accent.length, 0);

        let partial = registry
            .complete("/team Émile bl", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(partial.start, 12);
        assert_eq!(partial.length, 2);
        assert_eq!(partial.matches, vec!["blue".to_string()]);

        let astral = registry
            .complete("/team 😀 r", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(astral.start, 9);
        assert_eq!(astral.length, 1);

        let tab = registry
            .complete("/team É\t", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(tab.start, 8);
        assert_eq!(tab.length, 0);

        let nbsp = registry
            .complete("/team É\u{a0}", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(nbsp.start, 8);
        assert_eq!(nbsp.length, 0);

        let after_tab = registry
            .complete("/team É\tbl", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(after_tab.start, 8);
        assert_eq!(after_tab.length, 2);
        assert_eq!(after_tab.matches, vec!["blue".to_string()]);
    }

    #[test]
    fn completion_never_panics_on_random_unicode() {
        let registry = registry_with([team_command()]);
        let world = player_world();
        let alphabet: Vec<char> = "/ \t\u{a0}\u{2003}\n-@bltÉé😀漢\u{301}\u{200b}\u{feff}"
            .chars()
            .collect();
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };

        for i in 0..500 {
            let len = (next() % 12) as usize;
            let body: String = (0..len)
                .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
                .collect();
            let text = if i % 2 == 0 {
                format!("/team {body}")
            } else {
                body
            };
            let utf16_len = text.encode_utf16().count();
            if let Some(completion) = registry.complete(&text, &world, Entity::PLACEHOLDER) {
                assert!(
                    completion.start + completion.length <= utf16_len,
                    "range out of bounds for {text:?}"
                );
            }
        }
    }

    #[test]
    fn completion_treats_combined_bool_short_flags_as_flags() {
        let command = CommandBuilder::new("summon")
            .arg(
                "team",
                EnumArg::new([("red", Team::Red), ("blue", Team::Blue)]),
            )
            .flag("wet", Some('w'), "")
            .flag("glowing", Some('g'), "")
            .flag_value("reason", Some('r'), "", StringArg::single_word())
            .handler(|_| {})
            .build();
        let registry = registry_with([command]);
        let world = World::new();

        let combined = registry
            .complete("/summon -wg r", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(combined.matches, vec!["red".to_string()]);

        let with_value_flag = registry
            .complete("/summon -wr r", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert!(with_value_flag.matches.is_empty());

        let unknown = registry
            .complete("/summon -wx r", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert!(unknown.matches.is_empty());

        let after_stop = registry
            .complete("/summon -- -wg r", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert!(after_stop.matches.is_empty());
    }

    #[test]
    fn flag_values_parse_with_executor_context() {
        let mut world = player_world();
        let executor = world.spawn((PlayerName("Carol".into()), PlayerReady)).id();
        let bob = world
            .query::<(Entity, &PlayerName)>()
            .iter(&world)
            .find(|(_, name)| name.0 == "Bob")
            .map(|(entity, _)| entity)
            .unwrap();
        let command = CommandBuilder::new("warp")
            .flag_value("to", Some('t'), "", Arc::new(PlayerArg))
            .flag_value("from", None, "", Arc::new(PlayerArg))
            .handler(|_| {})
            .build();
        let ctx = ParseContext {
            world: &world,
            executor,
        };

        let args: Vec<String> = ["-t", "bob", "--from", "@s"]
            .into_iter()
            .map(String::from)
            .collect();
        let (positional, flags, errors) =
            flags::extract_flags(&args, &command.flag_definitions, &ctx);
        assert!(
            errors.is_empty(),
            "{}",
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        );
        assert!(positional.is_empty());
        assert_eq!(flags.get_value::<Entity>("to"), Some(&bob));
        assert_eq!(flags.get_value::<Entity>("from"), Some(&executor));

        let args: Vec<String> = ["--to", "nobody"].into_iter().map(String::from).collect();
        let (_, _, errors) = flags::extract_flags(&args, &command.flag_definitions, &ctx);
        assert!(matches!(
            errors.as_slice(),
            [ParseError::InvalidValue { name, .. }] if name == "to"
        ));
    }

    #[test]
    fn too_many_arguments_counts_tokens_on_both_sides() {
        let world = World::new();
        let ctx = ParseContext {
            world: &world,
            executor: Entity::PLACEHOLDER,
        };
        let definitions: Vec<_> = tp_command()
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
        let tokens: Vec<String> = ["1", "2", "3", "4"].into_iter().map(String::from).collect();

        let errors = parse_positional(&tokens, &definitions, &ctx).unwrap_err();
        assert!(matches!(
            errors.as_slice(),
            [ParseError::TooManyArguments {
                expected: 3,
                got: 4
            }]
        ));
    }

    #[test]
    fn completion_ignores_unknown_commands_and_bare_names() {
        let registry = registry_with([team_command()]);
        let world = player_world();
        assert!(
            registry
                .complete("/nope ", &world, Entity::PLACEHOLDER)
                .is_none()
        );
        assert!(
            registry
                .complete("/tea", &world, Entity::PLACEHOLDER)
                .is_none()
        );
        assert!(
            registry
                .complete("/team", &world, Entity::PLACEHOLDER)
                .is_none()
        );
    }

    #[test]
    fn completion_keeps_legacy_entity_type_suggestions() {
        let command = CommandBuilder::new("ride")
            .suggest_entity_types()
            .arg("what", StringArg::single_word())
            .handler(|_| {})
            .build();
        let registry = registry_with([command]);
        let world = World::new();
        let completion = registry
            .complete("/ride minecraft:pi", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert!(completion.matches.contains(&"minecraft:pig".to_string()));
        assert!(
            completion
                .matches
                .iter()
                .all(|m| m.starts_with("minecraft:pi"))
        );
    }

    #[test]
    fn command_tree_nodes_encode_vanilla_parser_ids_and_properties() {
        let registry = registry_with([tp_command(), gamemode_command(), team_command()]);
        let tree = registry.build_command_tree();

        let mut position = vec![0x02 | 0x04, 0, 8];
        position.extend(b"position");
        position.push(10);
        assert_eq!(encoded_argument_node(&tree, "position"), position);

        let mut mode = vec![0x02 | 0x04, 0, 4];
        mode.extend(b"mode");
        mode.push(42);
        assert_eq!(encoded_argument_node(&tree, "mode"), mode);

        let player = encoded_argument_node(&tree, "player");
        assert_eq!(player[0], 0x02 | 0x10);
        assert!(contains(&player, b"playerminecraft:ask_server"));

        let team = encoded_argument_node(&tree, "team");
        assert!(contains(&team, b"team minecraft:ask_server"));

        let time = encoded_argument_node(&tree, "for");
        assert!(contains(&time, b"for+    "));
        assert_eq!(time[0] & 0x10, 0);

        let mut buf = Vec::new();
        tree.encode(&mut buf);
        assert_eq!(buf[0] as usize, tree.nodes.len());
    }

    fn child_named(tree: &Commands, parent_index: i32, name: &str) -> i32 {
        tree.nodes[parent_index as usize]
            .children
            .iter()
            .copied()
            .find(|index| tree.nodes[*index as usize].name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing child node {name}"))
    }

    #[test]
    fn summon_tree_uses_vec3_and_exposes_flags_after_entity() {
        let mut registry = CommandRegistry::new();
        registry.register(summon_command());

        let tree = registry.build_command_tree();
        let summon = child_named(&tree, tree.root_index, "summon");
        let entity = child_named(&tree, summon, "entity");
        let position = child_named(&tree, entity, "position");

        assert!(matches!(
            tree.nodes[position as usize].parser,
            Some(Parser::Vec3)
        ));
        assert!(tree.nodes[entity as usize].is_executable);
        assert!(tree.nodes[position as usize].is_executable);

        for flag in ["--wander", "--gravity", "--block-checks", "-w", "-g", "-b"] {
            let flag_node = child_named(&tree, entity, flag);
            assert!(tree.nodes[flag_node as usize].is_executable);
            assert_eq!(tree.nodes[flag_node as usize].redirect_node, Some(entity));

            let position_flag_node = child_named(&tree, position, flag);
            assert_eq!(
                tree.nodes[position_flag_node as usize].redirect_node,
                Some(position)
            );
        }
    }
    fn recording_world() -> (
        World,
        Entity,
        Arc<std::sync::Mutex<Vec<String>>>,
        flume::Receiver<crate::network::OutgoingPacket>,
    ) {
        use crate::components::ClientId;
        use crate::network::{IncomingPacket, NetworkChannels, OutgoingPacket};

        let (_incoming_tx, incoming_rx) = flume::unbounded::<IncomingPacket>();
        let (outgoing_tx, outgoing_rx) = flume::unbounded::<OutgoingPacket>();
        let (_disconnect_tx, disconnect_rx) = flume::unbounded::<u32>();
        let (kick_tx, _kick_rx) = flume::unbounded::<u32>();
        let mut world = World::new();
        world.insert_resource(NetworkChannels {
            incoming: incoming_rx,
            outgoing: outgoing_tx,
            disconnect: disconnect_rx,
            kick: kick_tx,
        });
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        world.insert_resource(registry_with([club_command(Arc::clone(&calls))]));
        let player = world
            .spawn((ClientId(1), PlayerName("Alice".into()), PlayerReady))
            .id();
        (world, player, calls, outgoing_rx)
    }

    fn club_command(calls: Arc<std::sync::Mutex<Vec<String>>>) -> Command {
        let record = |calls: &Arc<std::sync::Mutex<Vec<String>>>, label: &'static str| {
            let calls = Arc::clone(calls);
            move |ctx: &mut CommandContext| {
                let mut entry = label.to_string();
                for name in ["club", "member", "color", "text"] {
                    if let Some(value) = ctx.get::<String>(name) {
                        entry.push_str(&format!(" {name}={value}"));
                    }
                }
                if ctx.flag("quiet") {
                    entry.push_str(" quiet");
                }
                calls.lock().unwrap().push(entry);
            }
        };
        CommandBuilder::new("club")
            .subcommand(
                CommandBuilder::new("add")
                    .alias("create")
                    .arg("club", StringArg::single_word())
                    .arg_variadic("text", StringArg::greedy())
                    .handler(record(&calls, "add")),
            )
            .subcommand(
                CommandBuilder::new("list")
                    .arg_optional("club", StringArg::single_word())
                    .handler(record(&calls, "list")),
            )
            .subcommand(
                CommandBuilder::new("modify")
                    .arg("club", StringArg::single_word())
                    .subcommand(
                        CommandBuilder::new("color")
                            .arg("color", Arc::new(parser::ColorArg))
                            .flag("quiet", Some('q'), "")
                            .handler(record(&calls, "color")),
                    )
                    .subcommand(
                        CommandBuilder::new("member")
                            .arg("member", Arc::new(GameProfileArg))
                            .handler(record(&calls, "member")),
                    ),
            )
            .build()
    }

    fn chats(rx: &flume::Receiver<crate::network::OutgoingPacket>) -> usize {
        rx.try_iter().count()
    }

    fn run(world: &mut World, player: Entity, line: &str) {
        let args = line.split_whitespace().map(String::from).collect();
        dispatch_command(world, 1, player, "club", args);
    }

    #[test]
    fn subcommands_dispatch_with_the_whole_path_parsed() {
        let (mut world, player, calls, rx) = recording_world();

        run(&mut world, player, "add reds The Red Club");
        run(&mut world, player, "create blues");
        run(&mut world, player, "list");
        run(&mut world, player, "list reds");
        run(&mut world, player, "modify reds color gold -q");
        run(&mut world, player, "modify reds member Alice");
        run(&mut world, player, "modify reds color -- gold");

        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                "add club=reds text=The Red Club",
                "add club=blues",
                "list",
                "list club=reds",
                "color club=reds color=gold quiet",
                "member club=reds member=Alice",
                "color club=reds color=gold",
            ]
        );
        assert_eq!(chats(&rx), 0);
    }

    #[test]
    fn subcommand_errors_reply_without_running_a_handler() {
        let (mut world, player, calls, rx) = recording_world();

        for line in [
            "",
            "nope",
            "modify",
            "modify reds",
            "modify reds paint red",
            "modify reds color purple_rain",
            "list a b",
        ] {
            run(&mut world, player, line);
            assert_eq!(
                chats(&rx),
                2,
                "{line:?} should send an error and a usage line"
            );
        }
        assert!(calls.lock().unwrap().is_empty());
    }

    #[test]
    fn subcommand_usage_follows_the_resolved_path() {
        let registry = registry_with([club_command(Default::default())]);
        assert_eq!(registry.usage("club").unwrap(), "/club <add|list|modify>");

        let usage = |line: &str| {
            let args: Vec<String> = line.split_whitespace().map(String::from).collect();
            match registry.resolve_handler("club", &args) {
                Resolved::Found(res) => res.usage,
                Resolved::NotFound(err) => err,
            }
        };
        assert_eq!(
            usage("modify x"),
            "/club modify <club:string> <color|member>"
        );
        assert_eq!(
            usage("modify x color"),
            "/club modify <club:string> color <color:color> [-q/--quiet]"
        );
        assert_eq!(usage("create"), "/club add <club:string> [text:string]...");
    }

    #[test]
    fn completion_walks_subcommands_to_the_right_argument() {
        let registry = registry_with([club_command(Default::default())]);
        let world = player_world();

        let literals = registry
            .complete("/club ", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(literals.matches, vec!["add", "create", "list", "modify"]);
        let partial = registry
            .complete("/club mo", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(partial.matches, vec!["modify"]);
        assert_eq!((partial.start, partial.length), (6, 2));

        let options = registry
            .complete("/club modify reds ", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(options.matches, vec!["color", "member"]);

        let members = registry
            .complete("/club modify reds member b", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert_eq!(members.matches, vec!["Bob"]);

        let flags = registry
            .complete(
                "/club modify reds color gold -",
                &world,
                Entity::PLACEHOLDER,
            )
            .unwrap();
        assert_eq!(flags.matches, vec!["--quiet", "-q"]);

        let unknown = registry
            .complete("/club paint ", &world, Entity::PLACEHOLDER)
            .unwrap();
        assert!(unknown.matches.is_empty());
    }

    #[test]
    fn subcommand_tree_branches_literals_after_arguments() {
        let registry = registry_with([club_command(Default::default())]);
        let tree = registry.build_command_tree();

        let club = child_named(&tree, tree.root_index, "club");
        assert!(!tree.nodes[club as usize].is_executable);

        let add = child_named(&tree, club, "add");
        let create = child_named(&tree, club, "create");
        assert_eq!(
            tree.nodes[add as usize].children,
            tree.nodes[create as usize].children
        );
        let add_club = child_named(&tree, add, "club");
        assert!(tree.nodes[add_club as usize].is_executable);

        let list = child_named(&tree, club, "list");
        assert!(tree.nodes[list as usize].is_executable);

        let modify = child_named(&tree, club, "modify");
        assert!(!tree.nodes[modify as usize].is_executable);
        let modify_club = child_named(&tree, modify, "club");
        assert!(!tree.nodes[modify_club as usize].is_executable);
        let color = child_named(&tree, modify_club, "color");
        assert_eq!(tree.nodes[color as usize].node_type, 1);
        let color_value = child_named(&tree, color, "color");
        assert!(matches!(
            tree.nodes[color_value as usize].parser,
            Some(Parser::Color)
        ));
        assert!(tree.nodes[color_value as usize].is_executable);
        let quiet = child_named(&tree, color_value, "--quiet");
        assert_eq!(tree.nodes[quiet as usize].redirect_node, Some(color_value));

        let mut buf = Vec::new();
        tree.encode(&mut buf);
        assert_eq!(buf[0] as usize, tree.nodes.len());
    }

    #[test]
    #[should_panic(expected = "handler or subcommands")]
    fn commands_need_a_handler_or_subcommands() {
        CommandBuilder::new("empty").build();
    }

    #[test]
    #[should_panic(expected = "before subcommands must be required")]
    fn optional_arguments_cannot_precede_subcommands() {
        CommandBuilder::new("bad")
            .arg_optional("x", StringArg::single_word())
            .subcommand(CommandBuilder::new("y").handler(|_| {}))
            .build();
    }

    fn echo_world(command: Command) -> (World, Entity) {
        let (mut world, player, _, _) = recording_world();
        world.insert_resource(registry_with([command]));
        (world, player)
    }

    #[test]
    fn dash_tokens_stay_positional_without_flags_or_after_a_greedy_start() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = |seen: &Arc<std::sync::Mutex<Vec<String>>>| {
            let seen = Arc::clone(seen);
            move |ctx: &mut CommandContext| {
                let mut entry = ctx.get::<String>("word").cloned().unwrap_or_default();
                if let Some(rest) = ctx.get::<String>("rest") {
                    entry.push('|');
                    entry.push_str(rest);
                }
                if ctx.flag("loud") {
                    entry.push_str("|loud");
                }
                seen.lock().unwrap().push(entry);
            }
        };
        let plain = CommandBuilder::new("plain")
            .arg("word", StringArg::single_word())
            .arg_variadic("rest", StringArg::greedy())
            .handler(record(&seen))
            .build();
        let flagged = CommandBuilder::new("flagged")
            .arg("word", StringArg::single_word())
            .arg_variadic("rest", StringArg::greedy())
            .flag("loud", Some('l'), "")
            .handler(record(&seen))
            .build();
        let (mut world, player) = echo_world(plain);
        world.resource_mut::<CommandRegistry>().register(flagged);

        for (name, line) in [
            ("plain", "-blue"),
            ("plain", "red \"Red -big team\""),
            ("plain", "-abc --"),
            ("flagged", "-l red a --b -c"),
            ("flagged", "red -- -x"),
        ] {
            let args = line.split_whitespace().map(String::from).collect();
            dispatch_command(&mut world, 1, player, name, args);
        }
        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                "-blue",
                "red|\"Red -big team\"",
                "-abc|--",
                "red|a --b -c|loud",
                "red|-- -x",
            ]
        );
    }

    #[test]
    fn requirements_gate_dispatch_and_completion_before_parsing() {
        use crate::components::Operator;

        let ran = Arc::new(std::sync::Mutex::new(0));
        let counter = Arc::clone(&ran);
        let command = CommandBuilder::new("secret")
            .requires(|world, executor| world.get::<Operator>(executor).is_some())
            .subcommand(
                CommandBuilder::new("open")
                    .arg("who", Arc::new(PlayerArg))
                    .handler(move |_| *counter.lock().unwrap() += 1),
            )
            .build();
        let (mut world, player) = echo_world(command);

        dispatch_command(
            &mut world,
            1,
            player,
            "secret",
            vec!["open".into(), "nobody".into()],
        );
        assert_eq!(*ran.lock().unwrap(), 0);
        let registry = world.resource::<CommandRegistry>();
        assert!(
            registry
                .complete("/secret ", &world, player)
                .unwrap()
                .matches
                .is_empty()
        );

        world.entity_mut(player).insert(Operator);
        dispatch_command(
            &mut world,
            1,
            player,
            "secret",
            vec!["open".into(), "@s".into()],
        );
        assert_eq!(*ran.lock().unwrap(), 1);
        let registry = world.resource::<CommandRegistry>();
        assert_eq!(
            registry
                .complete("/secret ", &world, player)
                .unwrap()
                .matches,
            vec!["open"]
        );
    }
}

//! What an NPC says, as data: nodes of lines, and what the player can answer.

use std::collections::BTreeMap;

use crate::font::{text_width, wrap};
use crate::rig::Gesture;
use crate::stage::Shot;

/// How wide a line of speech gets, and how many lines fit under the NPC's name.
pub const SPEECH_WIDTH: i32 = 240;
pub const SPEECH_LINES: usize = 3;
/// What fits on a choice, next to its mark.
pub const CHOICE_TEXT_WIDTH: i32 = 122;

/// What a player's choice leads to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Then {
    Goto(String),
    /// Opens an interface of the NPC, by name.
    Open(String),
    Leave,
}

/// Something the NPC says. What moves with it plays when it starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub shot: Option<Shot>,
    pub gesture: Option<Gesture>,
}

impl Line {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            shot: None,
            gesture: None,
        }
    }

    pub fn shot(mut self, shot: Shot) -> Self {
        self.shot = Some(shot);
        self
    }

    pub fn gesture(mut self, gesture: Gesture) -> Self {
        self.gesture = Some(gesture);
        self
    }

    pub(crate) fn wrapped(&self) -> Vec<String> {
        wrap(&self.text, SPEECH_WIDTH)
    }
}

/// An answer. It can depend on what the player did before, through [`crate::Memory`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    pub then: Then,
    pub needs: Vec<String>,
    pub unless: Vec<String>,
    pub sets: Vec<String>,
    pub emits: Option<String>,
}

impl Choice {
    pub fn new(label: impl Into<String>, then: Then) -> Self {
        Self {
            label: label.into(),
            then,
            needs: Vec::new(),
            unless: Vec::new(),
            sets: Vec::new(),
            emits: None,
        }
    }

    /// Only offered once `flag` is set.
    pub fn needs(mut self, flag: impl Into<String>) -> Self {
        self.needs.push(flag.into());
        self
    }

    /// No longer offered once `flag` is set.
    pub fn unless(mut self, flag: impl Into<String>) -> Self {
        self.unless.push(flag.into());
        self
    }

    /// Sets `flag` when chosen.
    pub fn sets(mut self, flag: impl Into<String>) -> Self {
        self.sets.push(flag.into());
        self
    }

    /// Triggers [`crate::NpcEvent`] with `name` when chosen.
    pub fn emits(mut self, name: impl Into<String>) -> Self {
        self.emits = Some(name.into());
        self
    }
}

/// Lines, then either choices or the next node. A node with neither ends the conversation.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: String,
    pub lines: Vec<Line>,
    pub choices: Vec<Choice>,
    pub next: Option<String>,
    pub sets: Vec<String>,
}

impl Node {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            lines: Vec::new(),
            choices: Vec::new(),
            next: None,
            sets: Vec::new(),
        }
    }

    pub fn say(self, text: impl Into<String>) -> Self {
        self.line(Line::new(text))
    }

    pub fn line(mut self, line: Line) -> Self {
        self.lines.push(line);
        self
    }

    pub fn choice(self, label: impl Into<String>, node: impl Into<String>) -> Self {
        self.answer(Choice::new(label, Then::Goto(node.into())))
    }

    pub fn open(self, label: impl Into<String>, interface: impl Into<String>) -> Self {
        self.answer(Choice::new(label, Then::Open(interface.into())))
    }

    pub fn leave(self, label: impl Into<String>) -> Self {
        self.answer(Choice::new(label, Then::Leave))
    }

    pub fn answer(mut self, choice: Choice) -> Self {
        self.choices.push(choice);
        self
    }

    /// Goes on with another node once its lines are said.
    pub fn then(mut self, node: impl Into<String>) -> Self {
        self.next = Some(node.into());
        self
    }

    /// Sets `flag` when the player gets here.
    pub fn sets(mut self, flag: impl Into<String>) -> Self {
        self.sets.push(flag.into());
        self
    }
}

/// All an NPC can say. It starts from the first opening whose flag is set, or from `start`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Conversation {
    start: String,
    openings: Vec<(String, String)>,
    nodes: BTreeMap<String, Node>,
}

impl Conversation {
    pub fn new(start: impl Into<String>) -> Self {
        Self {
            start: start.into(),
            ..Self::default()
        }
    }

    /// Starts from `node` once `flag` is set. The first one given goes first.
    pub fn opening(mut self, flag: impl Into<String>, node: impl Into<String>) -> Self {
        self.openings.push((flag.into(), node.into()));
        self
    }

    pub fn node(mut self, node: Node) -> Self {
        self.nodes.insert(node.id.clone(), node);
        self
    }

    pub fn get(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub(crate) fn start(&self, is_set: impl Fn(&str) -> bool) -> &str {
        self.openings
            .iter()
            .find(|(flag, _)| is_set(flag))
            .map_or(self.start.as_str(), |(_, node)| node.as_str())
    }

    /// What is wrong with it, for an NPC that is about to be used.
    pub(crate) fn problems(&self, is_interface: impl Fn(&str) -> bool) -> Vec<String> {
        let mut problems = Vec::new();
        let starts = self.openings.iter().map(|(_, node)| node);
        for node in starts.chain(std::iter::once(&self.start)) {
            if !self.nodes.contains_key(node) {
                problems.push(format!(
                    "the conversation starts from unknown node `{node}`"
                ));
            }
        }
        for node in self.nodes.values() {
            if node.lines.is_empty() {
                problems.push(format!("node `{}` says nothing", node.id));
            }
            if !node.choices.is_empty() && node.next.is_some() {
                problems.push(format!("node `{}` has both choices and a next", node.id));
            }
            if let Some(next) = &node.next
                && !self.nodes.contains_key(next)
            {
                problems.push(format!("node `{}` goes to unknown `{next}`", node.id));
            }
            for line in &node.lines {
                if line.wrapped().len() > SPEECH_LINES {
                    problems.push(format!("line too long in `{}`: {}", node.id, line.text));
                }
            }
            for choice in &node.choices {
                if text_width(&choice.label) > CHOICE_TEXT_WIDTH {
                    problems.push(format!("choice too long: {}", choice.label));
                }
                match &choice.then {
                    Then::Goto(to) if !self.nodes.contains_key(to) => {
                        problems.push(format!("`{}` leads to unknown node `{to}`", choice.label));
                    }
                    Then::Open(name) if !is_interface(name) => {
                        problems.push(format!("`{}` opens unknown `{name}`", choice.label));
                    }
                    _ => {}
                }
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn talk() -> Conversation {
        Conversation::new("a")
            .opening("met", "b")
            .node(
                Node::new("a")
                    .say("x")
                    .choice("to b", "b")
                    .open("shop", "shop"),
            )
            .node(Node::new("b").say("y").leave("bye"))
    }

    #[test]
    fn a_sound_conversation_has_no_problems() {
        assert!(talk().problems(|name| name == "shop").is_empty());
    }

    #[test]
    fn it_starts_from_the_first_opening_that_applies() {
        assert_eq!(talk().start(|_| false), "a");
        assert_eq!(talk().start(|flag| flag == "met"), "b");
    }

    #[test]
    fn a_choice_to_nowhere_is_a_problem() {
        let talk = Conversation::new("a").node(Node::new("a").say("x").choice("go", "nope"));
        assert_eq!(talk.problems(|_| true).len(), 1);
    }

    #[test]
    fn an_unknown_interface_is_a_problem() {
        assert_eq!(talk().problems(|_| false).len(), 1);
    }

    #[test]
    fn a_start_that_does_not_exist_is_a_problem() {
        assert_eq!(Conversation::new("a").problems(|_| true).len(), 1);
    }

    #[test]
    fn a_silent_node_is_a_problem() {
        let talk = Conversation::new("a").node(Node::new("a").leave("bye"));
        assert_eq!(talk.problems(|_| true).len(), 1);
    }

    #[test]
    fn a_node_cannot_both_ask_and_move_on() {
        let talk = Conversation::new("a").node(Node::new("a").say("x").leave("bye").then("a"));
        assert_eq!(talk.problems(|_| true).len(), 1);
    }

    #[test]
    fn a_label_that_does_not_fit_is_a_problem() {
        let long = "x".repeat(40);
        let talk = Conversation::new("a").node(Node::new("a").say("x").leave(long));
        assert_eq!(talk.problems(|_| true).len(), 1);
    }

    #[test]
    fn a_speech_that_does_not_fit_is_a_problem() {
        let long = "mot ".repeat(80);
        let talk = Conversation::new("a").node(Node::new("a").say(long));
        assert_eq!(talk.problems(|_| true).len(), 1);
    }
}

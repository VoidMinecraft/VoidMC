use ussr_nbt::owned::{Compound, List, Nbt, Tag};
use voidmc_protocol::clientbound::{ClearDialog, ShowDialog};

use crate::text::{Text, compound, string};

const DEFAULT_BUTTON_WIDTH: i32 = 150;

/// A dialog button. Without [`Button::on_click`] it does nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Button {
    label: Text,
    width: i32,
    click: Option<String>,
}

impl Button {
    pub fn new(label: impl Into<String>) -> Self {
        Self::with_label(Text::new(label))
    }

    pub fn with_label(label: Text) -> Self {
        Self {
            label,
            width: DEFAULT_BUTTON_WIDTH,
            click: None,
        }
    }

    pub fn width(mut self, width: i32) -> Self {
        self.width = width;
        self
    }

    pub fn on_click(mut self, id: impl Into<String>) -> Self {
        self.click = Some(id.into());
        self
    }

    fn to_compound(&self) -> Compound {
        let mut tags = vec![
            ("label", Tag::Compound(self.label.to_compound())),
            ("width", Tag::Int(self.width)),
        ];
        if let Some(id) = &self.click {
            let action = compound(vec![
                ("type", string("minecraft:custom")),
                ("id", string(id)),
            ]);
            tags.push(("action", Tag::Compound(action)));
        }
        compound(tags)
    }
}

/// A dialog that stays open across clicks, so the server can answer one by
/// sending the next dialog, which replaces it. The game keeps running behind it.
///
/// The client needs at least one button, hence the argument of [`Dialog::new`].
#[derive(Debug, Clone, PartialEq)]
pub struct Dialog {
    title: Text,
    body: Vec<(Text, i32)>,
    buttons: Vec<Button>,
    exit: Option<Button>,
    columns: i32,
}

impl Dialog {
    pub fn new(title: Text, button: Button) -> Self {
        Self {
            title,
            body: Vec::new(),
            buttons: vec![button],
            exit: None,
            columns: 1,
        }
    }

    /// Adds a block of text, wrapped at `width` GUI pixels.
    pub fn body(mut self, text: Text, width: i32) -> Self {
        self.body.push((text, width));
        self
    }

    pub fn button(mut self, button: Button) -> Self {
        self.buttons.push(button);
        self
    }

    /// Adds a footer button whose click also fires when the player presses
    /// Escape. It is the only way to learn that the dialog closed, and the button
    /// itself closes nothing: answer with [`Dialog::close`].
    pub fn exit(mut self, button: Button) -> Self {
        self.exit = Some(button);
        self
    }

    pub fn close() -> ClearDialog {
        ClearDialog {}
    }

    pub fn columns(mut self, columns: i32) -> Self {
        self.columns = columns;
        self
    }

    pub fn to_nbt(&self) -> Nbt {
        let body = self
            .body
            .iter()
            .map(|(text, width)| {
                compound(vec![
                    ("type", string("minecraft:plain_message")),
                    ("contents", Tag::Compound(text.to_compound())),
                    ("width", Tag::Int(*width)),
                ])
            })
            .collect();
        let buttons = self.buttons.iter().map(Button::to_compound).collect();
        let mut tags = vec![
            ("type", string("minecraft:multi_action")),
            ("title", Tag::Compound(self.title.to_compound())),
            ("body", Tag::List(List::Compound(body))),
            // The client refuses `after_action: none` on a pausing dialog.
            ("pause", Tag::Byte(0)),
            ("after_action", string("none")),
            ("columns", Tag::Int(self.columns)),
            ("actions", Tag::List(List::Compound(buttons))),
        ];
        if let Some(exit) = &self.exit {
            tags.push(("exit_action", Tag::Compound(exit.to_compound())));
        }
        Nbt {
            name: "".into(),
            compound: compound(tags),
        }
    }

    pub fn packet(&self) -> ShowDialog {
        ShowDialog::inline(self.to_nbt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tests::{tag, text_of};

    fn dialog() -> Dialog {
        Dialog::new(Text::new("t"), Button::new("ok").on_click("a:ok"))
    }

    #[test]
    fn a_dialog_stays_open_without_pausing() {
        let nbt = dialog().to_nbt();
        assert!(matches!(tag(&nbt.compound, "pause"), Some(Tag::Byte(0))));
        assert_eq!(text_of(tag(&nbt.compound, "after_action")), "none");
    }

    #[test]
    fn a_button_action_is_a_namespaced_custom_type() {
        let nbt = dialog().to_nbt();
        let Some(Tag::List(List::Compound(buttons))) = tag(&nbt.compound, "actions") else {
            panic!("no buttons");
        };
        let Some(Tag::Compound(action)) = tag(&buttons[0], "action") else {
            panic!("no action");
        };
        assert_eq!(text_of(tag(action, "type")), "minecraft:custom");
        assert_eq!(text_of(tag(action, "id")), "a:ok");
    }

    #[test]
    fn a_button_without_a_click_has_no_action() {
        let button = Button::new("x").to_compound();
        assert!(tag(&button, "action").is_none());
    }

    #[test]
    fn the_exit_button_is_only_there_when_asked_for() {
        assert!(tag(&dialog().to_nbt().compound, "exit_action").is_none());
        let nbt = dialog().exit(Button::new("x").on_click("a:x")).to_nbt();
        assert!(tag(&nbt.compound, "exit_action").is_some());
    }

    #[test]
    fn body_blocks_keep_their_width() {
        let nbt = dialog().body(Text::new("b"), 123).to_nbt();
        let Some(Tag::List(List::Compound(body))) = tag(&nbt.compound, "body") else {
            panic!("no body");
        };
        assert!(matches!(tag(&body[0], "width"), Some(Tag::Int(123))));
    }
}

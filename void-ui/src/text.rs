use ussr_nbt::owned::{Compound, List, Nbt, Tag};
use voidmc::TextColor;

pub(crate) fn string(value: &str) -> Tag {
    Tag::String(value.into())
}

pub(crate) fn compound(tags: Vec<(&str, Tag)>) -> Compound {
    tags.into_iter()
        .map(|(name, tag)| (name.into(), tag))
        .collect::<Vec<_>>()
        .into()
}

/// A text component. A click reports its id through [`crate::UiClick`]; a hover
/// shows another text at the mouse. Children inherit the parent's style, but a
/// hover text inherits nothing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Text {
    content: String,
    keybind: Option<String>,
    color: Option<TextColor>,
    font: Option<String>,
    shadow: bool,
    click: Option<String>,
    hover: Option<Box<Text>>,
    children: Vec<Text>,
}

impl Text {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            shadow: true,
            ..Self::default()
        }
    }

    /// The key a player has bound to `action`, such as `key.jump`, in their own
    /// layout and language: no text written on the server can know it.
    pub fn keybind(action: impl Into<String>) -> Self {
        Self {
            keybind: Some(action.into()),
            shadow: true,
            ..Self::default()
        }
    }

    pub fn color(mut self, color: TextColor) -> Self {
        self.color = Some(color);
        self
    }

    pub fn font(mut self, font: impl Into<String>) -> Self {
        self.font = Some(font.into());
        self
    }

    /// Glyphs that draw pictures need this: the shadow is a copy under each one.
    pub fn no_shadow(mut self) -> Self {
        self.shadow = false;
        self
    }

    pub fn on_click(mut self, id: impl Into<String>) -> Self {
        self.click = Some(id.into());
        self
    }

    pub fn on_hover(mut self, text: Text) -> Self {
        self.hover = Some(Box::new(text));
        self
    }

    pub fn append(mut self, child: Text) -> Self {
        self.children.push(child);
        self
    }

    pub fn extend(mut self, children: impl IntoIterator<Item = Text>) -> Self {
        self.children.extend(children);
        self
    }

    pub fn to_compound(&self) -> Compound {
        let mut tags = match &self.keybind {
            Some(action) => vec![("keybind", string(action))],
            None => vec![("text", string(&self.content))],
        };
        if let Some(color) = self.color {
            tags.push(("color", string(&color.to_string())));
        }
        if let Some(font) = &self.font {
            tags.push(("font", string(font)));
        }
        if !self.shadow {
            tags.push(("shadow_color", Tag::Int(0)));
        }
        if let Some(id) = &self.click {
            // A plain enum name here, unlike the registry id of a dialog action.
            let click = compound(vec![("action", string("custom")), ("id", string(id))]);
            tags.push(("click_event", Tag::Compound(click)));
        }
        if let Some(text) = &self.hover {
            let hover = compound(vec![
                ("action", string("show_text")),
                ("value", Tag::Compound(text.to_compound())),
            ]);
            tags.push(("hover_event", Tag::Compound(hover)));
        }
        if !self.children.is_empty() {
            let children = self.children.iter().map(Text::to_compound).collect();
            tags.push(("extra", Tag::List(List::Compound(children))));
        }
        compound(tags)
    }

    pub fn to_nbt(&self) -> Nbt {
        Nbt {
            name: "".into(),
            compound: self.to_compound(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn a_keybind_names_an_action_and_no_text() {
        let compound = Text::keybind("key.jump").to_compound();
        assert!(
            matches!(tag(&compound, "keybind"), Some(Tag::String(key)) if key.to_string() == "key.jump")
        );
        assert!(tag(&compound, "text").is_none());
    }

    pub(crate) fn tag<'a>(compound: &'a Compound, name: &str) -> Option<&'a Tag> {
        compound
            .tags
            .iter()
            .find(|(key, _)| key.to_string() == name)
            .map(|(_, tag)| tag)
    }

    pub(crate) fn text_of(tag: Option<&Tag>) -> String {
        match tag {
            Some(Tag::String(value)) => value.to_string(),
            other => panic!("not a string: {other:?}"),
        }
    }

    #[test]
    fn plain_text_only_has_its_content() {
        let compound = Text::new("hi").to_compound();
        assert_eq!(compound.tags.len(), 1);
        assert_eq!(text_of(tag(&compound, "text")), "hi");
    }

    #[test]
    fn a_click_is_a_custom_event_without_a_namespace() {
        let compound = Text::new("x").on_click("a:b").to_compound();
        let Some(Tag::Compound(click)) = tag(&compound, "click_event") else {
            panic!("no click event");
        };
        assert_eq!(text_of(tag(click, "action")), "custom");
        assert_eq!(text_of(tag(click, "id")), "a:b");
    }

    #[test]
    fn a_hover_carries_its_own_text() {
        let compound = Text::new("x").on_hover(Text::new("tip")).to_compound();
        let Some(Tag::Compound(hover)) = tag(&compound, "hover_event") else {
            panic!("no hover event");
        };
        assert_eq!(text_of(tag(hover, "action")), "show_text");
        let Some(Tag::Compound(value)) = tag(hover, "value") else {
            panic!("no value");
        };
        assert_eq!(text_of(tag(value, "text")), "tip");
    }

    #[test]
    fn no_shadow_sets_a_transparent_shadow() {
        let compound = Text::new("x").no_shadow().to_compound();
        assert!(matches!(tag(&compound, "shadow_color"), Some(Tag::Int(0))));
    }

    #[test]
    fn children_are_listed_in_order() {
        let compound = Text::new("")
            .append(Text::new("a"))
            .append(Text::new("b"))
            .to_compound();
        let Some(Tag::List(List::Compound(children))) = tag(&compound, "extra") else {
            panic!("no children");
        };
        assert_eq!(text_of(tag(&children[1], "text")), "b");
    }
}

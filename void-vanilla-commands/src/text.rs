use voidmc::TextColor;

/// The part of a text component the engine can display: its plain text and
/// the colour of its root.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StyledText {
    pub text: String,
    pub color: Option<TextColor>,
}

#[derive(Debug, PartialEq)]
enum Value {
    Text(String),
    Compound(Vec<(String, Value)>),
    List(Vec<Value>),
}

impl Value {
    fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Compound(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn string(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    fn plain_text(&self, out: &mut String) {
        match self {
            Value::Text(text) => out.push_str(text),
            Value::List(values) => values.iter().for_each(|value| value.plain_text(out)),
            Value::Compound(_) => {
                if let Some(text) = ["text", "translate", "keybind"]
                    .into_iter()
                    .find_map(|key| self.get(key).and_then(Value::string))
                {
                    out.push_str(text);
                }
                if let Some(extra) = self.get("extra") {
                    extra.plain_text(out);
                }
            }
        }
    }

    fn color(&self) -> Result<Option<TextColor>, String> {
        let value = match self {
            Value::List(values) => return values.first().map_or(Ok(None), Value::color),
            Value::Text(_) => return Ok(None),
            Value::Compound(_) => self.get("color"),
        };
        value
            .map(|value| {
                let name = value.string().unwrap_or_default();
                TextColor::parse(name).ok_or_else(|| format!("'{name}' is not a valid color"))
            })
            .transpose()
    }
}

/// Parses a `minecraft:component` argument: a quoted or bare string, an
/// SNBT/JSON compound such as `{"text":"Red","color":"red"}`, or a list of
/// those. Styling other than the root colour is dropped.
pub fn parse_component(input: &str) -> Result<StyledText, String> {
    let value = parse_value(input)?;
    let mut text = String::new();
    value.plain_text(&mut text);
    Ok(StyledText {
        text,
        color: value.color()?,
    })
}

/// Parses a `minecraft:style` argument such as `{"color":"gold"}`.
pub fn parse_style(input: &str) -> Result<Option<TextColor>, String> {
    let value = parse_value(input)?;
    match value {
        Value::Compound(_) => value.color(),
        _ => Err("expected a style compound like {\"color\":\"red\"}".into()),
    }
}

fn parse_value(input: &str) -> Result<Value, String> {
    let mut reader = Reader {
        chars: input.chars().collect(),
        index: 0,
    };
    let value = reader.value()?;
    reader.skip_whitespace();
    if reader.index < reader.chars.len() {
        return Err(format!(
            "unexpected '{}' after the text component",
            reader.chars[reader.index]
        ));
    }
    Ok(value)
}

struct Reader {
    chars: Vec<char>,
    index: usize,
}

impl Reader {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.index += 1;
        }
    }

    fn expect(&mut self, expected: char) -> Result<(), String> {
        self.skip_whitespace();
        match self.peek() {
            Some(c) if c == expected => {
                self.index += 1;
                Ok(())
            }
            Some(c) => Err(format!("expected '{expected}' but found '{c}'")),
            None => Err(format!("expected '{expected}' but the text ended")),
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.skip_whitespace();
        match self.peek() {
            Some('{') => self.compound(),
            Some('[') => self.list(),
            Some(_) => self.string().map(Value::Text),
            None => Err("expected a text component".into()),
        }
    }

    fn compound(&mut self) -> Result<Value, String> {
        self.expect('{')?;
        let mut entries = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some('}') {
            self.index += 1;
            return Ok(Value::Compound(entries));
        }
        loop {
            self.skip_whitespace();
            let key = self.string()?;
            self.expect(':')?;
            entries.push((key, self.value()?));
            self.skip_whitespace();
            match self.peek() {
                Some(',') => self.index += 1,
                Some('}') => {
                    self.index += 1;
                    return Ok(Value::Compound(entries));
                }
                _ => return Err("expected ',' or '}' in compound".into()),
            }
        }
    }

    fn list(&mut self) -> Result<Value, String> {
        self.expect('[')?;
        let mut values = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(']') {
            self.index += 1;
            return Ok(Value::List(values));
        }
        loop {
            values.push(self.value()?);
            self.skip_whitespace();
            match self.peek() {
                Some(',') => self.index += 1,
                Some(']') => {
                    self.index += 1;
                    return Ok(Value::List(values));
                }
                _ => return Err("expected ',' or ']' in list".into()),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        match self.peek() {
            Some(quote @ ('"' | '\'')) => {
                self.index += 1;
                self.quoted(quote)
            }
            _ => {
                let start = self.index;
                while self
                    .peek()
                    .is_some_and(|c| c.is_alphanumeric() || "_-.+".contains(c))
                {
                    self.index += 1;
                }
                if start == self.index {
                    return Err(match self.peek() {
                        Some(c) => format!("unexpected '{c}' in text component"),
                        None => "expected a text component".into(),
                    });
                }
                Ok(self.chars[start..self.index].iter().collect())
            }
        }
    }

    fn quoted(&mut self, quote: char) -> Result<String, String> {
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err("unterminated quoted string".into());
            };
            self.index += 1;
            match c {
                '\\' => {
                    let Some(escaped) = self.peek() else {
                        return Err("unterminated escape".into());
                    };
                    self.index += 1;
                    match escaped {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'u' => {
                            let hex: String = self.chars.iter().skip(self.index).take(4).collect();
                            let code = u32::from_str_radix(&hex, 16)
                                .ok()
                                .filter(|_| hex.len() == 4)
                                .and_then(char::from_u32)
                                .ok_or_else(|| format!("invalid unicode escape '\\u{hex}'"))?;
                            out.push(code);
                            self.index += 4;
                        }
                        other => out.push(other),
                    }
                }
                c if c == quote => return Ok(out),
                c => out.push(c),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(input: &str) -> StyledText {
        parse_component(input).unwrap_or_else(|err| panic!("{input}: {err}"))
    }

    #[test]
    fn plain_and_quoted_strings() {
        assert_eq!(text("Reds").text, "Reds");
        assert_eq!(text("\"Red Team\"").text, "Red Team");
        assert_eq!(text("'it\\'s \"red\"'").text, "it's \"red\"");
        assert_eq!(text("\"\\u00e9t\\u00e9\"").text, "été");
        assert_eq!(text("\"[R] \"").text, "[R] ");
    }

    #[test]
    fn compounds_keep_text_extra_and_root_color() {
        let red = text(r#"{"text":"Red","color":"red","bold":true}"#);
        assert_eq!(red.text, "Red");
        assert_eq!(red.color, Some(TextColor::Red));

        let snbt = text("{text:'A',extra:[' ',{text:B,color:blue}],color:'#ff8800'}");
        assert_eq!(snbt.text, "A B");
        assert_eq!(snbt.color, Some(TextColor::Rgb(0xff8800)));

        let list = text(r#"[{"text":"x","color":"gold"},"y"]"#);
        assert_eq!(list.text, "xy");
        assert_eq!(list.color, Some(TextColor::Gold));
    }

    #[test]
    fn malformed_components_are_errors() {
        for input in [
            "",
            "\"open",
            "{text:'a'",
            "{text 'a'}",
            "a b",
            "[a,]",
            "{color:nope}",
            "\"\\u12\"",
        ] {
            assert!(parse_component(input).is_err(), "{input:?}");
        }
    }

    #[test]
    fn styles_read_the_color() {
        assert_eq!(
            parse_style(r#"{"color":"aqua","italic":true}"#),
            Ok(Some(TextColor::Aqua))
        );
        assert_eq!(parse_style("{}"), Ok(None));
        assert!(parse_style("red").is_err());
    }
}

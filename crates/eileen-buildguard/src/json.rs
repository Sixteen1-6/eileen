//! The smallest reader and writer for the guard's line formats.
//!
//! The machine-wide files hold flat JSON objects of unsigned integers, booleans and short
//! strings without escapes. This crate takes no serde (it is std only, so xtask compiles in
//! seconds), so it reads and writes exactly that shape and refuses anything else.

use std::fmt::Write as _;

/// One field's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Value<'a> {
    /// A non-negative integer.
    Num(u64),
    /// `true` or `false`.
    Bool(bool),
    /// A string with no escapes.
    Str(&'a str),
}

/// A flat object's fields, in file order.
pub(crate) struct Object<'a>(Vec<(&'a str, Value<'a>)>);

impl<'a> Object<'a> {
    fn get(&self, key: &str) -> Option<Value<'a>> {
        self.0.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    }

    pub(crate) fn u64(&self, key: &str) -> Option<u64> {
        match self.get(key)? {
            Value::Num(n) => Some(n),
            Value::Bool(_) | Value::Str(_) => None,
        }
    }

    pub(crate) fn u32(&self, key: &str) -> Option<u32> {
        u32::try_from(self.u64(key)?).ok()
    }

    pub(crate) fn bool(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            Value::Bool(b) => Some(b),
            Value::Num(_) | Value::Str(_) => None,
        }
    }

    pub(crate) fn str(&self, key: &str) -> Option<&'a str> {
        match self.get(key)? {
            Value::Str(s) => Some(s),
            Value::Num(_) | Value::Bool(_) => None,
        }
    }
}

struct Cursor<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Option<()> {
        self.skip_space();
        (self.peek()? == byte).then(|| self.pos += 1)
    }

    fn string(&mut self) -> Option<&'a str> {
        self.eat(b'"')?;
        let start = self.pos;
        loop {
            match self.peek()? {
                b'"' => break,
                b'\\' | 0..=0x1f => return None,
                _ => self.pos += 1,
            }
        }
        let text = self.text.get(start..self.pos)?;
        self.pos += 1;
        Some(text)
    }

    fn value(&mut self) -> Option<Value<'a>> {
        self.skip_space();
        match self.peek()? {
            b'"' => self.string().map(Value::Str),
            b't' => self.word("true").map(|()| Value::Bool(true)),
            b'f' => self.word("false").map(|()| Value::Bool(false)),
            b'0'..=b'9' => {
                let start = self.pos;
                while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                    self.pos += 1;
                }
                self.text.get(start..self.pos)?.parse().ok().map(Value::Num)
            }
            _ => None,
        }
    }

    fn word(&mut self, word: &str) -> Option<()> {
        let end = self.pos + word.len();
        (self.text.get(self.pos..end)? == word).then(|| self.pos = end)
    }
}

/// Parses one line holding one flat object; `None` for anything else.
pub(crate) fn parse(line: &str) -> Option<Object<'_>> {
    let mut cursor = Cursor { text: line, pos: 0 };
    cursor.eat(b'{')?;
    let mut fields = Vec::new();
    cursor.skip_space();
    if cursor.peek() == Some(b'}') {
        cursor.pos += 1;
    } else {
        loop {
            let key = cursor.string()?;
            cursor.eat(b':')?;
            fields.push((key, cursor.value()?));
            cursor.skip_space();
            match cursor.peek()? {
                b',' => cursor.pos += 1,
                b'}' => {
                    cursor.pos += 1;
                    break;
                }
                _ => return None,
            }
        }
    }
    cursor.skip_space();
    (cursor.pos == line.len()).then_some(Object(fields))
}

/// Writes one flat object in the documented spacing: `{"a": 1, "b": true}`.
#[derive(Default)]
pub(crate) struct Writer(String);

impl Writer {
    fn key(&mut self, key: &str) {
        self.0.push_str(if self.0.is_empty() { "{\"" } else { ", \"" });
        self.0.push_str(key);
        self.0.push_str("\": ");
    }

    pub(crate) fn num(mut self, key: &str, value: u64) -> Self {
        self.key(key);
        let _infallible = write!(self.0, "{value}");
        self
    }

    pub(crate) fn bool(mut self, key: &str, value: bool) -> Self {
        self.key(key);
        self.0.push_str(if value { "true" } else { "false" });
        self
    }

    pub(crate) fn str(mut self, key: &str, value: &str) -> Self {
        self.key(key);
        self.0.push('"');
        self.0.push_str(value);
        self.0.push('"');
        self
    }

    /// The object, closed, with no newline.
    pub(crate) fn finish(mut self) -> String {
        self.0.push('}');
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{Writer, parse};

    #[test]
    fn writes_and_reads_back_the_documented_shape() {
        let line = Writer::default()
            .num("at_ms", 12)
            .bool("slot_held", false)
            .str("src", "guard")
            .finish();
        assert_eq!(line, r#"{"at_ms": 12, "slot_held": false, "src": "guard"}"#, "spacing");
        let object = parse(&line).unwrap();
        assert_eq!(object.u64("at_ms"), Some(12), "number");
        assert_eq!(object.bool("slot_held"), Some(false), "bool");
        assert_eq!(object.str("src"), Some("guard"), "string");
        assert_eq!(object.u32("missing"), None, "missing");
    }

    #[test]
    fn refuses_anything_else() {
        for bad in [
            "",
            "{",
            r#"{"a": -1}"#,
            r#"{"a": 1.5}"#,
            r#"{"a": "x\"y"}"#,
            r#"{"a": 1} trailing"#,
            r#"{"a" 1}"#,
            r#"{"a": nul}"#,
        ] {
            assert!(parse(bad).is_none(), "{bad:?}");
        }
        assert!(parse(" { } ").is_some(), "empty object");
    }
}

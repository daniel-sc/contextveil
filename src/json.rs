//! JSON5 source-document parsing and exact RFC 6901 pointer selection.
//!
//! Object members are checked while deserializing because parsing directly into
//! a map would discard duplicate names before they can be rejected.

use std::collections::HashSet;
use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
const DUPLICATE_MARKER: &str = "contextveil duplicate object member";
const MAX_NESTING_DEPTH: usize = 128;

/// A source-document value that can represent JSON5's non-finite numbers.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Object),
}

impl Value {
    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Self::Object(object) => Some(object),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Number(value)
                if value.is_finite()
                    && *value >= 0.0
                    && value.fract() == 0.0
                    && *value < 18_446_744_073_709_551_616.0 =>
            {
                Some(*value as u64)
            }
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Self::String(_))
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.as_object()?.get(name)
    }

    pub fn pointer(&self, pointer: &str) -> Option<&Value> {
        if pointer.is_empty() {
            Some(self)
        } else if pointer.starts_with('/') {
            select(self, pointer)
        } else {
            None
        }
    }
}

/// An insertion-ordered object used to keep Known Source discovery deterministic.
#[derive(Debug, Clone, PartialEq)]
pub struct Object(Vec<(String, Value)>);

impl Object {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0
            .iter()
            .find_map(|(key, value)| (key == name).then_some(value))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter().map(|(key, value)| (key, value))
    }

    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.0.iter().map(|(_, value)| value)
    }
}

impl<'a> IntoIterator for &'a Object {
    type Item = (&'a String, &'a Value);
    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, (String, Value)>,
        fn(&(String, Value)) -> (&String, &Value),
    >;

    fn into_iter(self) -> Self::IntoIter {
        fn pair(entry: &(String, Value)) -> (&String, &Value) {
            (&entry.0, &entry.1)
        }
        self.0.iter().map(pair)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    Malformed,
    DuplicateMember,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerError {
    Invalid,
    EmptyFinalToken,
    Wildcard,
}

/// Encodes one object member name as an RFC 6901 reference token.
pub fn encode_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Validates a plain RFC 6901 pointer and returns its decoded final token.
pub fn final_token(pointer: &str) -> Result<String, PointerError> {
    if !pointer.starts_with('/') {
        return Err(PointerError::Invalid);
    }

    let mut final_token = None;
    for encoded in pointer[1..].split('/') {
        let token = decode_token(encoded)?;
        if token == "*" {
            return Err(PointerError::Wildcard);
        }
        final_token = Some(token);
    }

    match final_token {
        Some(token) if !token.is_empty() => Ok(token),
        _ => Err(PointerError::EmptyFinalToken),
    }
}

/// Parses one complete JSON5 document, rejecting duplicate members at any depth.
pub fn parse(text: &str) -> Result<Value, ParseError> {
    preflight(text)?;
    json5::from_str::<StrictValue>(text)
        .map(|value| value.0)
        .map_err(|error| {
            if error.to_string().contains(DUPLICATE_MARKER) {
                ParseError::DuplicateMember
            } else {
                ParseError::Malformed
            }
        })
}

/// Rejects lexical cases the parser dependency accepts too loosely and bounds
/// recursion before deserialization reaches the process stack.
fn preflight(text: &str) -> Result<(), ParseError> {
    #[derive(Clone, Copy)]
    enum State {
        Normal,
        String(char),
        LineComment,
        BlockComment,
    }

    let mut state = State::Normal;
    let mut depth = 0usize;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match state {
            State::Normal => match character {
                '\'' | '"' => state = State::String(character),
                '/' if characters.peek() == Some(&'/') => {
                    characters.next();
                    state = State::LineComment;
                }
                '/' if characters.peek() == Some(&'*') => {
                    characters.next();
                    state = State::BlockComment;
                }
                '{' | '[' => {
                    depth += 1;
                    if depth > MAX_NESTING_DEPTH {
                        return Err(ParseError::Malformed);
                    }
                }
                '}' | ']' => depth = depth.saturating_sub(1),
                _ => {}
            },
            State::String(quote) => match character {
                '\\' => {
                    if characters.next() == Some('\r') && characters.peek() == Some(&'\n') {
                        characters.next();
                    }
                }
                character if character == quote => state = State::Normal,
                _ => {}
            },
            State::LineComment => {
                if matches!(character, '\n' | '\r' | '\u{2028}' | '\u{2029}') {
                    state = State::Normal;
                }
            }
            State::BlockComment => {
                if character == '*' && characters.peek() == Some(&'/') {
                    characters.next();
                    state = State::Normal;
                }
            }
        }
    }
    if matches!(state, State::BlockComment) {
        Err(ParseError::Malformed)
    } else {
        Ok(())
    }
}

/// Selects exactly one value using an already validated pointer.
pub fn select<'a>(value: &'a Value, pointer: &str) -> Option<&'a Value> {
    let mut selected = value;
    for encoded in pointer[1..].split('/') {
        let token = decode_token(encoded).ok()?;
        selected = match selected {
            Value::Object(object) => object.get(&token)?,
            Value::Array(array) => {
                let bytes = token.as_bytes();
                let valid_index = token == "0"
                    || (matches!(bytes.first(), Some(b'1'..=b'9'))
                        && bytes[1..].iter().all(u8::is_ascii_digit));
                if !valid_index {
                    return None;
                }
                array.get(token.parse::<usize>().ok()?)?
            }
            _ => return None,
        };
    }
    Some(selected)
}

fn decode_token(encoded: &str) -> Result<String, PointerError> {
    let mut decoded = String::with_capacity(encoded.len());
    let mut characters = encoded.chars();
    while let Some(character) = characters.next() {
        if character != '~' {
            decoded.push(character);
            continue;
        }
        match characters.next() {
            Some('0') => decoded.push('~'),
            Some('1') => decoded.push('/'),
            _ => return Err(PointerError::Invalid),
        }
    }
    Ok(decoded)
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictVisitor)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = StrictValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON5 value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(value as f64)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(value as f64)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(StrictValue(Value::Number(value)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value.to_string())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<StrictValue>()? {
            values.push(value.0);
        }
        Ok(StrictValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut names = HashSet::new();
        let mut values = Vec::new();
        while let Some(name) = object.next_key::<String>()? {
            if !names.insert(name.clone()) {
                return Err(serde::de::Error::custom(DUPLICATE_MARKER));
            }
            values.push((name, object.next_value::<StrictValue>()?.0));
        }
        Ok(StrictValue(Value::Object(Object(values))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointers_are_validated_and_final_tokens_are_decoded() {
        assert_eq!(
            final_token("/tokens/access_token"),
            Ok("access_token".into())
        );
        assert_eq!(final_token("/a~1b/~0key"), Ok("~key".into()));
        for pointer in ["", "#/*", "a/b", "/", "/a/", "/a/~2b"] {
            assert!(final_token(pointer).is_err(), "accepted {pointer:?}");
        }
        assert_eq!(final_token("/tokens/*"), Err(PointerError::Wildcard));
    }

    #[test]
    fn reference_tokens_are_encoded_in_rfc6901_order() {
        assert_eq!(encode_token("plain"), "plain");
        assert_eq!(encode_token("a/b"), "a~1b");
        assert_eq!(encode_token("a~b/c"), "a~0b~1c");
        assert_eq!(
            final_token(&format!("/{}", encode_token("a~b/c"))),
            Ok("a~b/c".into())
        );
    }

    #[test]
    fn exact_selection_handles_objects_arrays_and_escaping() {
        let value = parse(r#"{"a/b":{"~key":["zero","one"]}}"#).expect("valid JSON");
        assert_eq!(
            select(&value, "/a~1b/~0key/1"),
            Some(&Value::String("one".into()))
        );
        for token in ["01", "+1", "+01", "-1", "-", "", "999999999999999999999"] {
            assert_eq!(select(&value, &format!("/a~1b/~0key/{token}")), None);
        }
        assert_eq!(
            select(&value, "/a~1b/~0key/0"),
            Some(&Value::String("zero".into()))
        );
        assert_eq!(select(&value, "/missing"), None);
    }

    #[test]
    fn duplicate_members_at_every_depth_are_rejected() {
        assert_eq!(parse(r#"{"a":1,"a":2}"#), Err(ParseError::DuplicateMember));
        assert_eq!(
            parse(r#"{"selected":"ok","other":{"x":1,"x":2}}"#),
            Err(ParseError::DuplicateMember)
        );
    }

    #[test]
    fn full_json5_forms_parse_and_exact_pointers_still_select() {
        let value = parse(
            r#"{
                // JSON5 source documents may use the complete human-friendly grammar.
                /* Block comments are part of the grammar too. */
                unquoted: 'selected',
                'single-quoted key': 'single-quoted value',
                hex: 0xdecaf,
                leadingDecimal: .5,
                trailingDecimal: 5.,
                exponent: 1e3,
                positiveInfinity: +Infinity,
                negativeInfinity: -Infinity,
                notANumber: NaN,
                trailing: [true, null,],
                multiline: 'line one\
line two',
            }"#,
        )
        .expect("valid JSON5");

        assert_eq!(
            select(&value, "/unquoted").and_then(Value::as_str),
            Some("selected")
        );
        assert_eq!(
            select(&value, "/single-quoted key").and_then(Value::as_str),
            Some("single-quoted value")
        );
        for pointer in [
            "/hex",
            "/positiveInfinity",
            "/negativeInfinity",
            "/notANumber",
            "/trailing",
        ] {
            assert!(select(&value, pointer).is_some(), "missing {pointer}");
        }
        assert!(
            matches!(select(&value, "/positiveInfinity"), Some(Value::Number(value)) if value.is_infinite())
        );
        assert!(
            matches!(select(&value, "/notANumber"), Some(Value::Number(value)) if value.is_nan())
        );

        let value = parse("{\\u0061ccess: '\\x76alue',\u{a0}other: 'ok'}")
            .expect("JSON5 identifier escapes, string escapes, and whitespace");
        assert_eq!(
            select(&value, "/access").and_then(Value::as_str),
            Some("value")
        );
    }

    #[test]
    fn json5_duplicate_members_at_every_depth_are_rejected() {
        assert_eq!(
            parse("{token: 1, 'token': 2}"),
            Err(ParseError::DuplicateMember)
        );
        assert_eq!(
            parse("{selected: 'ok', other: {nested: 1, nested: 2}}"),
            Err(ParseError::DuplicateMember)
        );
    }

    #[test]
    fn malformed_and_trailing_input_are_rejected() {
        assert_eq!(parse("{"), Err(ParseError::Malformed));
        assert_eq!(parse("{} trailing"), Err(ParseError::Malformed));
        assert_eq!(parse("{} /*"), Err(ParseError::Malformed));
    }

    #[test]
    fn excessive_nesting_is_rejected_before_deserialization() {
        let document = format!(
            "{}'selected'{}",
            "[".repeat(MAX_NESTING_DEPTH + 1),
            "]".repeat(MAX_NESTING_DEPTH + 1)
        );
        assert_eq!(parse(&document), Err(ParseError::Malformed));
    }
}

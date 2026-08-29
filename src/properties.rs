//! Java properties parsing with the crate's default Windows-1252 decoding.
//!
//! Parsing is transactional: entries are exposed only after the complete input
//! has parsed successfully, and repeated decoded keys retain their last value.

use std::collections::HashMap;
use std::io::Cursor;

use java_properties::{LineContent, PropertiesIter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Properties {
    entries: Vec<(String, String)>,
    index: HashMap<String, usize>,
    duplicates: Vec<String>,
}

impl Properties {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.index
            .get(key)
            .map(|index| self.entries[*index].1.as_str())
    }

    pub fn duplicates(&self) -> &[String] {
        &self.duplicates
    }

    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }
}

pub fn parse(bytes: &[u8]) -> Result<Properties, ParseError> {
    // java-properties 2.0.0 and encoding_rs 0.8.35 can re-enter a finished
    // decoder when a non-empty file has no physical line ending. A final line
    // ending is semantically equivalent for Properties and avoids that bug.
    let mut terminated = Vec::with_capacity(bytes.len() + 1);
    terminated.extend_from_slice(bytes);
    terminated.push(b'\n');
    parse_inner(&terminated).map_err(|_| ParseError)
}

fn parse_inner(bytes: &[u8]) -> Result<Properties, java_properties::PropertiesError> {
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut duplicates = Vec::new();

    for line in PropertiesIter::new(Cursor::new(bytes)) {
        let line = line?;
        let LineContent::KVPair(key, value) = line.consume_content() else {
            continue;
        };
        if let Some(position) = index.get(&key).copied() {
            entries[position].1 = value;
            if !duplicates.contains(&key) {
                duplicates.push(key);
            }
        } else {
            index.insert(key.clone(), entries.len());
            entries.push((key, value));
        }
    }

    Ok(Properties {
        entries,
        index,
        duplicates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_parser_decodes_windows_1252() {
        let parsed = parse(b"price=\x80\nname=caf\xe9\n").expect("properties");
        assert_eq!(parsed.get("price"), Some("\u{20ac}"));
        assert_eq!(parsed.get("name"), Some("caf\u{e9}"));
    }

    #[test]
    fn separators_continuations_and_escapes_follow_java_properties() {
        let parsed = parse(
            b"colon:one\nspace two\nequals=three\ncontinued=first\\\n  second\nescaped\\ key=tab\\tunicode\\u0021\n",
        )
        .expect("properties");
        assert_eq!(parsed.get("colon"), Some("one"));
        assert_eq!(parsed.get("space"), Some("two"));
        assert_eq!(parsed.get("equals"), Some("three"));
        assert_eq!(parsed.get("continued"), Some("firstsecond"));
        assert_eq!(parsed.get("escaped key"), Some("tab\tunicode!"));
    }

    #[test]
    fn duplicate_decoded_keys_use_the_last_value() {
        let parsed = parse(b"a\\u0062=first\nab=second\nab=third\n").expect("properties");
        assert_eq!(parsed.get("ab"), Some("third"));
        assert_eq!(parsed.duplicates(), ["ab"]);
    }

    #[test]
    fn a_late_error_returns_no_partial_parse() {
        assert!(parse(b"token=must-not-escape\nbroken=\\u12xz\n").is_err());
    }

    #[test]
    fn empty_and_hostile_inputs_do_not_panic() {
        for input in [b"".as_slice(), b"\0\0\0", &[0xff, 0xfe, 0xfd]] {
            let _ = parse(input);
        }
    }
}

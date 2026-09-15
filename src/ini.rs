//! ContextVeil's thin wrapper around the `rust-ini` parser.
//!
//! The parser is deliberately kept behind this module so the rest of the core
//! does not depend on the library's first-match getters.  Iterating properties
//! preserves occurrences; ContextVeil applies last-assignment-wins within each
//! exact section/key identity.

use std::collections::{HashMap, HashSet};

/// A parsed INI document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ini {
    /// Entries in first-assignment order after last-assignment-wins.
    entries: Vec<(Option<String>, String, String)>,
    index: HashMap<(Option<String>, String), usize>,
    duplicate_keys: Vec<String>,
}

/// The value-free location of a library parse error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
}

impl Ini {
    /// Returns the current value for one exact section/key identity.
    pub fn get(&self, section: Option<&str>, key: &str) -> Option<&str> {
        let identity = (section.map(str::to_owned), key.to_owned());
        self.index
            .get(&identity)
            .map(|position| self.entries[*position].2.as_str())
    }

    /// Returns entries in first-assignment order, with duplicate assignments
    /// represented by their final value.
    pub fn entries(&self) -> impl Iterator<Item = (Option<&str>, &str, &str)> {
        self.entries
            .iter()
            .map(|(section, key, value)| (section.as_deref(), key.as_str(), value.as_str()))
    }

    /// Keys assigned more than once, retained for the shared diagnostic shape.
    pub fn duplicate_keys(&self) -> &[String] {
        &self.duplicate_keys
    }
}

/// Parses one UTF-8 INI document using the approved `rust-ini` dialect.
pub fn parse(input: &str) -> Result<Ini, ParseError> {
    // `rust-ini` does not treat a leading BOM as grammar whitespace, while
    // ContextVeil accepts the optional UTF-8 BOM at the source boundary.
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let parsed = ::ini::Ini::load_from_str_opt(
        input,
        ::ini::ParseOption {
            enabled_escape: false,
            ..::ini::ParseOption::default()
        },
    )
    .map_err(|error| ParseError {
        line: error.line,
        column: error.col,
    })?;

    let mut result = Ini::default();
    let mut duplicate_keys = HashSet::new();
    for (section, properties) in parsed.iter() {
        let section = section.map(str::to_owned);
        for (key, value) in properties.iter() {
            let identity = (section.clone(), key.to_owned());
            if let Some(position) = result.index.get(&identity) {
                result.entries[*position].2 = value.to_owned();
                if duplicate_keys.insert(key) {
                    result.duplicate_keys.push(key.to_owned());
                }
            } else {
                result.index.insert(identity, result.entries.len());
                result
                    .entries
                    .push((section.clone(), key.to_owned(), value.to_owned()));
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_section_keys_use_the_last_assignment() {
        let parsed =
            parse("token=first\n[prod]\ntoken=second\n[prod]\ntoken=third\n").expect("valid INI");
        assert_eq!(parsed.get(None, "token"), Some("first"));
        assert_eq!(parsed.get(Some("prod"), "token"), Some("third"));
        assert_eq!(parsed.duplicate_keys(), &["token"]);
    }

    #[test]
    fn duplicate_warnings_are_unique_per_key() {
        let parsed = parse(
            "token=one\ntoken=two\ntoken=three\npassword=first\npassword=last\n[prod]\ntoken=four\ntoken=five\npassword=other\npassword=final\n",
        )
        .expect("valid INI");
        assert_eq!(parsed.get(None, "token"), Some("three"));
        assert_eq!(parsed.get(Some("prod"), "token"), Some("five"));
        assert_eq!(parsed.duplicate_keys(), &["token", "password"]);
    }

    #[test]
    fn sectionless_and_empty_named_sections_are_distinct() {
        let parsed =
            parse("token=plain\n[]\ntoken=empty\n[default]\ntoken=named\n").expect("valid INI");
        assert_eq!(parsed.get(None, "token"), Some("plain"));
        assert_eq!(parsed.get(Some(""), "token"), Some("empty"));
        assert_eq!(parsed.get(Some("default"), "token"), Some("named"));
        assert!(parsed.duplicate_keys().is_empty());
    }

    #[test]
    fn escape_decoding_is_disabled_but_quoted_multiline_values_work() {
        let parsed =
            parse("[auth]\npath=C:\\\\token\nvalue=\"first\nsecond\"\n").expect("valid INI");
        assert_eq!(parsed.get(Some("auth"), "path"), Some("C:\\\\token"));
        assert_eq!(parsed.get(Some("auth"), "value"), Some("first\nsecond"));
    }

    #[test]
    fn parser_errors_expose_only_a_location() {
        let error = parse("[broken\nTOKEN=value\n").expect_err("malformed INI");
        assert!(error.line > 0);
        assert!(error.column > 0);
    }

    #[test]
    fn accepted_dialect_keeps_case_and_literals() {
        let parsed = parse(
            "\u{feff}TOKEN=plain\r\n[token]\r\nName=\"quoted # value\"\r\npath=C:\\\\Windows\\\\token\r\nexpr=${TOKEN}\r\n[DEFAULT]\r\nName=default\r\n",
        )
        .expect("valid INI");
        assert_eq!(parsed.get(None, "TOKEN"), Some("plain"));
        assert_eq!(parsed.get(Some("token"), "Name"), Some("quoted # value"));
        assert_eq!(parsed.get(Some("token"), "name"), None);
        assert_eq!(
            parsed.get(Some("token"), "path"),
            Some("C:\\\\Windows\\\\token")
        );
        assert_eq!(parsed.get(Some("token"), "expr"), Some("${TOKEN}"));
        assert_eq!(parsed.get(Some("DEFAULT"), "Name"), Some("default"));
    }

    #[test]
    fn backslash_newline_continuation_is_supported() {
        let parsed = parse("[token]\nvalue=first\\\n  second\n").expect("valid INI");
        assert_eq!(parsed.get(Some("token"), "value"), Some("first  second"));
    }
}

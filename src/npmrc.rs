//! Dependency-free parsing for ContextVeil's narrow npmrc scalar grammar.
//!
//! Parsing is deliberately key-local: valid top-level assignments remain
//! available when unrelated lines are malformed, while recoverable keyed
//! problems are retained for callers selecting those exact keys (`SRC-018`).

use std::collections::HashMap;

/// Parsed top-level scalar assignments and keyed syntax issues from one npmrc.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Npmrc {
    entries: Vec<(String, String)>,
    index: HashMap<String, usize>,
    duplicates: Vec<String>,
    issues: Vec<(String, Vec<Issue>)>,
    issue_index: HashMap<String, usize>,
}

impl Npmrc {
    /// Current value of a valid top-level scalar key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.index
            .get(key)
            .map(|position| self.entries[*position].1.as_str())
    }

    /// Valid scalar entries in first-assignment order, after last-valid-wins.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// Keys with more than one valid scalar assignment, each listed once.
    pub fn duplicates(&self) -> &[String] {
        &self.duplicates
    }

    /// Every malformed or unsupported occurrence attributable to `key`.
    pub fn issue(&self, key: &str) -> Option<&[Issue]> {
        let position = *self.issue_index.get(key)?;
        Some(self.issues[position].1.as_slice())
    }

    fn insert(&mut self, key: String, value: String) {
        if let Some(position) = self.index.get(&key).copied() {
            self.entries[position].1 = value;
            if !self.duplicates.contains(&key) {
                self.duplicates.push(key);
            }
        } else {
            self.index.insert(key.clone(), self.entries.len());
            self.entries.push((key, value));
        }
    }

    fn add_issue(&mut self, key: String, issue: Issue) {
        if let Some(position) = self.issue_index.get(&key).copied() {
            self.issues[position].1.push(issue);
        } else {
            self.issue_index.insert(key.clone(), self.issues.len());
            self.issues.push((key, vec![issue]));
        }
    }
}

/// A value-free problem attributable to one exact key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Issue {
    /// One-based physical line where the occurrence starts.
    pub line: usize,
    pub kind: IssueKind,
}

/// Value-free classifications for malformed and unsupported keyed syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    UnsupportedSection,
    UnsupportedArray,
    KeyOnly,
    UnterminatedQuote,
    TrailingContent,
}

impl IssueKind {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::UnsupportedSection => "is contained in an unsupported section",
            Self::UnsupportedArray => "uses unsupported array syntax",
            Self::KeyOnly => "is a key-only field without an assignment",
            Self::UnterminatedQuote => "has an unterminated or multiline quoted value",
            Self::TrailingContent => "has unexpected text after a quoted value",
        }
    }
}

/// Parses already validated UTF-8 npmrc text using `SRC-018`.
pub fn parse(input: &str) -> Npmrc {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut npmrc = Npmrc::default();
    let mut in_section = false;

    for (line_index, physical_line) in input.split('\n').enumerate() {
        let line_number = line_index + 1;
        let line = physical_line.strip_suffix('\r').unwrap_or(physical_line);
        let trimmed = line.trim();

        if trimmed.is_empty() || matches!(trimmed.as_bytes().first(), Some(b'#' | b';')) {
            continue;
        }
        if is_section_header(trimmed) {
            in_section = true;
            continue;
        }
        // A malformed section-like line has no recoverable scalar key.
        if trimmed.starts_with('[') {
            continue;
        }

        let Some((raw_key, raw_value)) = line.split_once('=') else {
            npmrc.add_issue(
                trimmed.to_string(),
                Issue {
                    line: line_number,
                    kind: IssueKind::KeyOnly,
                },
            );
            continue;
        };

        let raw_key = raw_key.trim();
        if raw_key.is_empty() {
            continue;
        }
        let (key, is_array) = match raw_key.strip_suffix("[]") {
            Some(base) if !base.trim().is_empty() => (base.trim(), true),
            Some(_) => continue,
            None => (raw_key, false),
        };

        let unsupported = if is_array {
            Some(IssueKind::UnsupportedArray)
        } else if in_section {
            Some(IssueKind::UnsupportedSection)
        } else {
            None
        };
        if let Some(kind) = unsupported {
            npmrc.add_issue(
                key.to_string(),
                Issue {
                    line: line_number,
                    kind,
                },
            );
            continue;
        }

        match parse_value(raw_value) {
            Ok(value) => npmrc.insert(key.to_string(), value),
            Err(kind) => npmrc.add_issue(
                key.to_string(),
                Issue {
                    line: line_number,
                    kind,
                },
            ),
        }
    }

    npmrc
}

fn is_section_header(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('[') else {
        return false;
    };
    let Some(closing) = rest.find(']') else {
        return false;
    };
    let trailing = rest[closing + 1..].trim_start();
    trailing.is_empty() || matches!(trailing.as_bytes().first(), Some(b'#' | b';'))
}

fn parse_value(raw: &str) -> Result<String, IssueKind> {
    let value = raw.trim_start();
    match value.as_bytes().first() {
        Some(b'\'') => parse_single_quoted(value),
        Some(b'"') => parse_double_quoted(value),
        _ => Ok(parse_unquoted(raw)),
    }
}

fn parse_single_quoted(value: &str) -> Result<String, IssueKind> {
    let Some(closing) = value[1..].find('\'') else {
        return Err(IssueKind::UnterminatedQuote);
    };
    let closing = closing + 1;
    finish_quoted(&value[closing + 1..])?;
    Ok(value[1..closing].to_string())
}

fn parse_double_quoted(value: &str) -> Result<String, IssueKind> {
    let bytes = value.as_bytes();
    let mut decoded = String::new();
    let mut position = 1;

    while position < bytes.len() {
        match bytes[position] {
            b'"' => {
                finish_quoted(&value[position + 1..])?;
                return Ok(decoded);
            }
            b'\\' => match bytes.get(position + 1) {
                Some(b'\\') => {
                    decoded.push('\\');
                    position += 2;
                }
                Some(b'"') => {
                    decoded.push('"');
                    position += 2;
                }
                Some(b'n') => {
                    decoded.push('\n');
                    position += 2;
                }
                Some(b'r') => {
                    decoded.push('\r');
                    position += 2;
                }
                Some(b't') => {
                    decoded.push('\t');
                    position += 2;
                }
                Some(_) => {
                    decoded.push('\\');
                    position += 1;
                }
                None => return Err(IssueKind::UnterminatedQuote),
            },
            _ => {
                let character = value[position..]
                    .chars()
                    .next()
                    .expect("position is on a character boundary");
                decoded.push(character);
                position += character.len_utf8();
            }
        }
    }

    Err(IssueKind::UnterminatedQuote)
}

fn finish_quoted(trailing: &str) -> Result<(), IssueKind> {
    let trailing = trailing.trim_start();
    if trailing.is_empty() || matches!(trailing.as_bytes().first(), Some(b'#' | b';')) {
        Ok(())
    } else {
        Err(IssueKind::TrailingContent)
    }
}

fn parse_unquoted(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut decoded = String::new();
    let mut position = 0;

    while position < bytes.len() {
        match bytes[position] {
            b'#' | b';' => break,
            b'\\' => match bytes.get(position + 1) {
                Some(b'#') => {
                    decoded.push('#');
                    position += 2;
                }
                Some(b';') => {
                    decoded.push(';');
                    position += 2;
                }
                Some(b'\\') => {
                    decoded.push('\\');
                    position += 2;
                }
                _ => {
                    decoded.push('\\');
                    position += 1;
                }
            },
            _ => {
                let character = raw[position..]
                    .chars()
                    .next()
                    .expect("position is on a character boundary");
                decoded.push(character);
                position += character.len_utf8();
            }
        }
    }

    decoded.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(input: &str) -> Npmrc {
        parse(input)
    }

    #[test]
    fn parses_trimmed_exact_and_scoped_keys_in_insertion_order() {
        let npmrc = parsed(
            " plain = one\n@scope:registry=https://example.invalid/a=b\n//registry.example/:_authToken=three\n",
        );
        assert_eq!(npmrc.get("plain"), Some("one"));
        assert_eq!(
            npmrc.get("@scope:registry"),
            Some("https://example.invalid/a=b")
        );
        assert_eq!(npmrc.get("//registry.example/:_authToken"), Some("three"));
        assert_eq!(
            npmrc.entries().map(|(key, _)| key).collect::<Vec<_>>(),
            ["plain", "@scope:registry", "//registry.example/:_authToken"]
        );
    }

    #[test]
    fn accepts_initial_bom_crlf_blank_lines_and_both_full_line_comments() {
        let npmrc = parsed("\u{feff}\r\n  # ignored\r\n\t; ignored\r\nkey=value\r\n");
        assert_eq!(npmrc.entries().collect::<Vec<_>>(), [("key", "value")]);
        assert!(npmrc.issue("# ignored").is_none());
    }

    #[test]
    fn unquoted_values_stop_at_first_unescaped_comment_and_decode_only_three_escapes() {
        let npmrc = parsed(
            "hash=left\\#right#comment\nsemicolon=left\\;right;comment\nslashes=a\\\\b\\q\npair=a\\\\#comment\n",
        );
        assert_eq!(npmrc.get("hash"), Some("left#right"));
        assert_eq!(npmrc.get("semicolon"), Some("left;right"));
        assert_eq!(npmrc.get("slashes"), Some("a\\b\\q"));
        assert_eq!(npmrc.get("pair"), Some("a\\"));
    }

    #[test]
    fn quoted_values_keep_comments_literal_and_apply_quote_specific_decoding() {
        let npmrc = parsed(
            "single='literal \\n # ;' ; comment\ndouble=\"line\\nquote\\\" tab\\t unknown\\q # ;\"#comment\n",
        );
        assert_eq!(npmrc.get("single"), Some("literal \\n # ;"));
        assert_eq!(
            npmrc.get("double"),
            Some("line\nquote\" tab\t unknown\\q # ;")
        );
    }

    #[test]
    fn unterminated_quotes_and_trailing_junk_are_keyed_without_exposing_values() {
        let npmrc = parsed("single='not closed\ndouble=\"not closed\ntrailing='closed' junk\n");
        assert_eq!(
            npmrc.issue("single"),
            Some(
                [Issue {
                    line: 1,
                    kind: IssueKind::UnterminatedQuote
                }]
                .as_slice()
            )
        );
        assert_eq!(npmrc.issue("double").expect("double issue")[0].line, 2);
        assert_eq!(
            npmrc.issue("trailing").expect("trailing issue")[0].kind,
            IssueKind::TrailingContent
        );
        assert!(!IssueKind::TrailingContent.reason().contains("closed"));
    }

    #[test]
    fn duplicate_valid_assignments_are_unique_and_last_valid_wins() {
        let npmrc = parsed(
            "first=one\nsecond=two\nsecond=updated\nfirst='bad' junk\nfirst=final\nsecond=last\n",
        );
        assert_eq!(npmrc.get("first"), Some("final"));
        assert_eq!(npmrc.get("second"), Some("last"));
        assert_eq!(npmrc.duplicates(), ["second", "first"]);
        assert_eq!(
            npmrc.entries().map(|(key, _)| key).collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert_eq!(npmrc.issue("first").expect("first issue")[0].line, 4);
    }

    #[test]
    fn arrays_key_only_fields_and_section_assignments_are_keyed_issues() {
        let npmrc = parsed(
            "array[]=one\nkey-only\n=unkeyed\n[]=unkeyed-array\n[profile]\ninside=value\nother='bad' junk\n",
        );
        assert_eq!(
            npmrc.issue("array").expect("array issue")[0].kind,
            IssueKind::UnsupportedArray
        );
        assert_eq!(
            npmrc.issue("key-only").expect("key-only issue")[0].kind,
            IssueKind::KeyOnly
        );
        assert_eq!(
            npmrc.issue("inside").expect("section issue")[0].kind,
            IssueKind::UnsupportedSection
        );
        assert_eq!(
            npmrc.issue("other").expect("section issue")[0].kind,
            IssueKind::UnsupportedSection
        );
        assert!(npmrc.issue("").is_none());
        assert_eq!(npmrc.entries().count(), 0);
    }

    #[test]
    fn malformed_unkeyed_lines_are_ignored_and_keyed_issues_accumulate() {
        let npmrc =
            parsed("[broken section\n=value\nchosen='bad' junk\nchosen=\"bad\nchosen=valid\n");
        assert_eq!(npmrc.get("chosen"), Some("valid"));
        assert_eq!(
            npmrc.issue("chosen"),
            Some(
                [
                    Issue {
                        line: 3,
                        kind: IssueKind::TrailingContent
                    },
                    Issue {
                        line: 4,
                        kind: IssueKind::UnterminatedQuote
                    }
                ]
                .as_slice()
            )
        );
        assert!(npmrc.issue("[broken section").is_none());
    }

    #[test]
    fn environment_expression_text_is_literal_in_every_value_form() {
        let npmrc = parsed("plain=${NAME}\nsingle='${NAME}'\ndouble=\"${NAME}\"\n");
        for key in ["plain", "single", "double"] {
            assert_eq!(npmrc.get(key), Some("${NAME}"));
        }
    }
}

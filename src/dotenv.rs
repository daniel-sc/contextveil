//! Deterministic dotenv parsing.
//!
//! `SRC-003` fixes the grammar exactly. The parser performs no variable
//! interpolation, command substitution, or code execution, and it imposes no
//! SecretSieve-specific size cap (`SRC-008`, `LIM-010`).
//!
//! Malformed syntax is a malfunction (`SRC-006`), not an unresolved source, so
//! the caller must disable the whole effective registry for the event.

use std::collections::HashMap;

/// Parsed assignments from one dotenv file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dotenv {
    /// Assignments in file order after applying last-key-wins (`SRC-004`).
    entries: Vec<(String, String)>,
    /// Position of each key in `entries`, so lookup and last-key-wins stay linear
    /// in file size rather than quadratic in key count (`SRC-008`: files have no
    /// size cap, so this has to hold for large files too).
    index: HashMap<String, usize>,
    /// Keys assigned more than once, in first-occurrence order.
    duplicates: Vec<String>,
}

impl Dotenv {
    /// Current value of one key, or `None` when the key is absent.
    pub fn get(&self, key: &str) -> Option<&str> {
        let position = *self.index.get(key)?;
        Some(self.entries[position].1.as_str())
    }

    /// Every assignment in file order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// Keys that were assigned more than once (`SRC-004`).
    pub fn duplicates(&self) -> &[String] {
        &self.duplicates
    }
}

/// A malformed dotenv file, located but never quoted.
///
/// The offending text is deliberately absent: dotenv files hold credentials, so
/// no diagnostic may echo their content (`SEC-004`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError {
    /// One-based line where the problem was detected.
    pub line: usize,
    pub kind: ParseErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// The line is neither blank, a comment, nor a valid assignment.
    InvalidLine,
    /// A quoted value has no closing quote.
    UnterminatedQuote,
    /// Text other than a comment follows a closing quote.
    TrailingContent,
}

impl ParseErrorKind {
    pub fn reason(&self) -> &'static str {
        match self {
            ParseErrorKind::InvalidLine => "is not a valid assignment",
            ParseErrorKind::UnterminatedQuote => "opens a quoted value that is never closed",
            ParseErrorKind::TrailingContent => "has unexpected text after a quoted value",
        }
    }
}

/// Parses dotenv text.
///
/// The input must already be valid UTF-8; invalid encoding is a malfunction
/// diagnosed by the caller that read the file.
pub fn parse(input: &str) -> Result<Dotenv, ParseError> {
    // CRLF endings normalize to LF everywhere, including inside a multiline
    // quoted value (`SRC-003`).
    let normalized = if input.contains('\r') {
        input.replace("\r\n", "\n")
    } else {
        input.to_string()
    };
    let text = normalized.strip_prefix('\u{feff}').unwrap_or(&normalized);
    let bytes = text.as_bytes();

    let mut parser = Parser {
        text,
        bytes,
        position: 0,
        counted_to: 0,
        counted_lines: 0,
    };
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut duplicates: Vec<String> = Vec::new();

    while let Some((key, value)) = parser.next_assignment()? {
        match index.get(&key) {
            Some(position) => {
                // Last assignment wins (`SRC-004`).
                entries[*position].1 = value;
                if !duplicates.contains(&key) {
                    duplicates.push(key);
                }
            }
            None => {
                index.insert(key.clone(), entries.len());
                entries.push((key, value));
            }
        }
    }

    Ok(Dotenv {
        entries,
        index,
        duplicates,
    })
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    position: usize,
    /// Offset up to which newlines have already been counted, and the count.
    ///
    /// Parsing only moves forward, so line numbers are accumulated instead of
    /// recomputed from the start of the file. Recomputing made a large file cost
    /// quadratic time, which matters because `SRC-008` allows any size.
    counted_to: usize,
    counted_lines: usize,
}

impl<'a> Parser<'a> {
    /// Returns the next assignment, skipping blank and comment lines.
    fn next_assignment(&mut self) -> Result<Option<(String, String)>, ParseError> {
        loop {
            self.skip_grammar_whitespace();
            match self.peek() {
                None => return Ok(None),
                Some(b'\n') => {
                    self.position += 1;
                    continue;
                }
                Some(b'#') => {
                    self.skip_to_line_end();
                    continue;
                }
                Some(_) => break,
            }
        }

        let line = self.line_of(self.position);
        let invalid = ParseError {
            line,
            kind: ParseErrorKind::InvalidLine,
        };

        // An assignment may begin with the exact token `export` followed by at
        // least one whitespace character. `export=value` instead defines the key
        // `export`, and `export =value` is malformed (`SRC-003`).
        if self.text[self.position..].starts_with("export")
            && matches!(self.bytes.get(self.position + 6), Some(b' ' | b'\t'))
        {
            self.position += 6;
            self.skip_grammar_whitespace();
        }

        let key = self.take_key().ok_or(invalid)?;
        self.skip_grammar_whitespace();
        if self.peek() != Some(b'=') {
            return Err(invalid);
        }
        self.position += 1;

        let value = self.take_value(line)?;
        Ok(Some((key, value)))
    }

    /// Consumes a `[A-Za-z_][A-Za-z0-9_.-]*` key.
    fn take_key(&mut self) -> Option<String> {
        let start = self.position;
        match self.peek() {
            Some(byte) if byte.is_ascii_alphabetic() || byte == b'_' => self.position += 1,
            _ => return None,
        }
        while let Some(byte) = self.peek() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-') {
                self.position += 1;
            } else {
                break;
            }
        }
        Some(self.text[start..self.position].to_string())
    }

    fn take_value(&mut self, line: usize) -> Result<String, ParseError> {
        let value_start = self.position;
        let mut lookahead = self.position;
        while matches!(self.bytes.get(lookahead), Some(b' ' | b'\t')) {
            lookahead += 1;
        }
        match self.bytes.get(lookahead) {
            Some(b'\'') => {
                self.position = lookahead + 1;
                self.take_single_quoted(line)
            }
            Some(b'"') => {
                self.position = lookahead + 1;
                self.take_double_quoted(line)
            }
            _ => {
                self.position = value_start;
                Ok(self.take_unquoted())
            }
        }
    }

    /// A single-quoted value is literal until the matching quote and may span
    /// physical lines.
    fn take_single_quoted(&mut self, line: usize) -> Result<String, ParseError> {
        let start = self.position;
        while let Some(byte) = self.peek() {
            if byte == b'\'' {
                let value = self.text[start..self.position].to_string();
                self.position += 1;
                self.finish_quoted(line)?;
                return Ok(value);
            }
            self.position += 1;
        }
        Err(ParseError {
            line,
            kind: ParseErrorKind::UnterminatedQuote,
        })
    }

    /// A double-quoted value decodes only `\\`, `\"`, `\n`, `\r`, and `\t`; any
    /// other backslash pair retains the backslash (`SRC-003`).
    fn take_double_quoted(&mut self, line: usize) -> Result<String, ParseError> {
        let mut value = String::new();
        while let Some(byte) = self.peek() {
            match byte {
                b'"' => {
                    self.position += 1;
                    self.finish_quoted(line)?;
                    return Ok(value);
                }
                b'\\' => {
                    match self.bytes.get(self.position + 1) {
                        Some(b'\\') => value.push('\\'),
                        Some(b'"') => value.push('"'),
                        Some(b'n') => value.push('\n'),
                        Some(b'r') => value.push('\r'),
                        Some(b't') => value.push('\t'),
                        Some(_) => {
                            value.push('\\');
                            let next = self.text[self.position + 1..]
                                .chars()
                                .next()
                                .expect("a byte follows the backslash");
                            value.push(next);
                            self.position += 1 + next.len_utf8();
                            continue;
                        }
                        None => {
                            return Err(ParseError {
                                line,
                                kind: ParseErrorKind::UnterminatedQuote,
                            });
                        }
                    }
                    self.position += 2;
                }
                _ => {
                    let character = self.text[self.position..]
                        .chars()
                        .next()
                        .expect("the position is on a character boundary");
                    value.push(character);
                    self.position += character.len_utf8();
                }
            }
        }
        Err(ParseError {
            line,
            kind: ParseErrorKind::UnterminatedQuote,
        })
    }

    /// After a closing quote only whitespace and an optional comment are valid.
    fn finish_quoted(&mut self, line: usize) -> Result<(), ParseError> {
        self.skip_grammar_whitespace();
        match self.peek() {
            None | Some(b'\n') => Ok(()),
            Some(b'#') => {
                self.skip_to_line_end();
                Ok(())
            }
            Some(_) => Err(ParseError {
                line,
                kind: ParseErrorKind::TrailingContent,
            }),
        }
    }

    /// An unquoted value runs to the physical line end. A comment starts only at
    /// a `#` preceded by grammar whitespace, and surrounding whitespace is
    /// trimmed. Backslashes are literal.
    fn take_unquoted(&mut self) -> String {
        let start = self.position;
        while !matches!(self.peek(), None | Some(b'\n')) {
            self.position += 1;
        }
        let raw = &self.text[start..self.position];

        let mut end = raw.len();
        let raw_bytes = raw.as_bytes();
        for (index, byte) in raw_bytes.iter().enumerate() {
            if *byte == b'#' && index > 0 && matches!(raw_bytes[index - 1], b' ' | b'\t') {
                end = index;
                break;
            }
        }
        raw[..end].trim_matches([' ', '\t']).to_string()
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn skip_grammar_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.position += 1;
        }
    }

    fn skip_to_line_end(&mut self) {
        while !matches!(self.peek(), None | Some(b'\n')) {
            self.position += 1;
        }
    }

    fn line_of(&mut self, offset: usize) -> usize {
        debug_assert!(offset >= self.counted_to, "parsing only moves forward");
        self.counted_lines += self.text[self.counted_to..offset].matches('\n').count();
        self.counted_to = offset;
        self.counted_lines + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(input: &str) -> Dotenv {
        parse(input).expect("valid dotenv")
    }

    fn value(input: &str, key: &str) -> String {
        parsed(input).get(key).expect("key is present").to_string()
    }

    #[test]
    fn simple_assignments_parse() {
        let dotenv = parsed("A=1\nB=two\n");
        assert_eq!(dotenv.get("A"), Some("1"));
        assert_eq!(dotenv.get("B"), Some("two"));
        assert_eq!(dotenv.get("C"), None);
    }

    #[test]
    fn blank_lines_and_comments_are_ignored() {
        let dotenv = parsed("\n  \n# comment\n\t# indented comment\nA=1\n");
        assert_eq!(dotenv.entries().count(), 1);
    }

    #[test]
    fn a_missing_final_newline_is_accepted() {
        assert_eq!(value("A=1", "A"), "1");
    }

    #[test]
    fn a_leading_bom_is_ignored() {
        assert_eq!(value("\u{feff}A=1\n", "A"), "1");
    }

    #[test]
    fn crlf_endings_normalize_to_lf() {
        assert_eq!(value("A=1\r\nB=2\r\n", "A"), "1");
        assert_eq!(value("A=\"line1\r\nline2\"\r\n", "A"), "line1\nline2");
    }

    #[test]
    fn whitespace_around_the_equals_sign_is_allowed() {
        assert_eq!(value("A = 1\n", "A"), "1");
        assert_eq!(value("A\t=\t1\n", "A"), "1");
        assert_eq!(value("  A=1\n", "A"), "1");
    }

    #[test]
    fn export_is_accepted_only_as_a_separate_token() {
        assert_eq!(value("export A=1\n", "A"), "1");
        assert_eq!(value("export\tA=1\n", "A"), "1");
        // `export=value` defines the key `export`.
        assert_eq!(value("export=1\n", "export"), "1");
        // `export =value` is malformed.
        assert_eq!(
            parse("export =1\n"),
            Err(ParseError {
                line: 1,
                kind: ParseErrorKind::InvalidLine
            })
        );
        // `exported` is an ordinary key, not the export token.
        assert_eq!(value("exported=1\n", "exported"), "1");
    }

    #[test]
    fn key_syntax_is_enforced() {
        assert!(parse("A_b.c-d=1\n").is_ok());
        assert!(parse("_leading=1\n").is_ok());
        for invalid in ["1LEADING=1\n", "with space=1\n", "with$=1\n", "=novalue\n"] {
            assert_eq!(
                parse(invalid).map(|_| ()),
                Err(ParseError {
                    line: 1,
                    kind: ParseErrorKind::InvalidLine
                }),
                "expected `{invalid}` to be malformed"
            );
        }
    }

    #[test]
    fn a_line_without_an_equals_sign_is_malformed() {
        assert_eq!(
            parse("JUST_A_KEY\n"),
            Err(ParseError {
                line: 1,
                kind: ParseErrorKind::InvalidLine
            })
        );
    }

    #[test]
    fn unquoted_values_trim_whitespace_and_keep_backslashes() {
        assert_eq!(value("A=  spaced  \n", "A"), "spaced");
        assert_eq!(value("A=C:\\path\\to\\file\n", "A"), "C:\\path\\to\\file");
        assert_eq!(value("A=with\\nliteral\n", "A"), "with\\nliteral");
        assert_eq!(value("A=inner space\n", "A"), "inner space");
        assert_eq!(value("A=\n", "A"), "");
        assert_eq!(value("A=   \n", "A"), "");
    }

    #[test]
    fn unquoted_comments_start_only_after_whitespace() {
        assert_eq!(value("A=value # comment\n", "A"), "value");
        assert_eq!(value("A=value\t# comment\n", "A"), "value");
        assert_eq!(value("A=#hash\n", "A"), "#hash");
        assert_eq!(value("A=va#lue\n", "A"), "va#lue");
        assert_eq!(value("A= # comment\n", "A"), "");
    }

    #[test]
    fn single_quoted_values_are_literal() {
        assert_eq!(value("A='raw \\n value'\n", "A"), "raw \\n value");
        assert_eq!(
            value("A='with \"double\" quotes'\n", "A"),
            "with \"double\" quotes"
        );
        assert_eq!(value("A='# not a comment'\n", "A"), "# not a comment");
        assert_eq!(value("A='line1\nline2'\n", "A"), "line1\nline2");
        assert_eq!(value("A=''\n", "A"), "");
    }

    #[test]
    fn double_quoted_values_decode_only_the_listed_escapes() {
        assert_eq!(value(r#"A="a\nb""#, "A"), "a\nb");
        assert_eq!(value(r#"A="a\tb""#, "A"), "a\tb");
        assert_eq!(value(r#"A="a\rb""#, "A"), "a\rb");
        assert_eq!(value(r#"A="a\\b""#, "A"), "a\\b");
        assert_eq!(value(r#"A="a\"b""#, "A"), "a\"b");
        // Any other backslash pair retains the backslash.
        assert_eq!(value(r#"A="a\qb""#, "A"), "a\\qb");
        assert_eq!(value(r#"A="a\$b""#, "A"), "a\\$b");
        assert_eq!(value("A=\"line1\nline2\"\n", "A"), "line1\nline2");
    }

    #[test]
    fn no_interpolation_or_substitution_happens() {
        assert_eq!(value("A=$HOME\n", "A"), "$HOME");
        assert_eq!(value(r#"A="${HOME}""#, "A"), "${HOME}");
        assert_eq!(value("A=`whoami`\n", "A"), "`whoami`");
        assert_eq!(value("A=$(whoami)\n", "A"), "$(whoami)");
    }

    #[test]
    fn unterminated_quotes_are_malformed() {
        assert_eq!(
            parse("A='unterminated\n"),
            Err(ParseError {
                line: 1,
                kind: ParseErrorKind::UnterminatedQuote
            })
        );
        assert_eq!(
            parse("A=\"unterminated\n"),
            Err(ParseError {
                line: 1,
                kind: ParseErrorKind::UnterminatedQuote
            })
        );
        assert_eq!(
            parse("A=\"trailing backslash\\"),
            Err(ParseError {
                line: 1,
                kind: ParseErrorKind::UnterminatedQuote
            })
        );
    }

    #[test]
    fn text_after_a_closing_quote_is_malformed() {
        assert_eq!(
            parse("A='value' junk\n"),
            Err(ParseError {
                line: 1,
                kind: ParseErrorKind::TrailingContent
            })
        );
        // A comment after a closing quote is fine.
        assert_eq!(value("A='value' # ok\n", "A"), "value");
        assert_eq!(value("A=\"value\"\t# ok\n", "A"), "value");
    }

    #[test]
    fn error_lines_are_reported_relative_to_the_file() {
        assert_eq!(
            parse("A=1\nB=2\n\nnot valid\n").map(|_| ()),
            Err(ParseError {
                line: 4,
                kind: ParseErrorKind::InvalidLine
            })
        );
    }

    #[test]
    fn the_last_assignment_wins_and_duplicates_are_reported() {
        let dotenv = parsed("A=first\nB=other\nA=second\n");
        assert_eq!(dotenv.get("A"), Some("second"));
        assert_eq!(dotenv.duplicates(), ["A"]);
        // File order is preserved for the surviving entries.
        let keys: Vec<&str> = dotenv.entries().map(|(key, _)| key).collect();
        assert_eq!(keys, ["A", "B"]);
    }

    #[test]
    fn utf8_keys_and_values_survive_unchanged() {
        assert_eq!(value("A=pässwörd✓\n", "A"), "pässwörd✓");
        assert_eq!(value("A=\"pässwörd✓\"\n", "A"), "pässwörd✓");
        assert_eq!(value("A='pässwörd✓'\n", "A"), "pässwörd✓");
    }

    #[test]
    fn errors_never_quote_file_content() {
        let error = parse("SECRET_LOOKING_LINE without equals\n").expect_err("malformed");
        assert!(!error.kind.reason().contains("SECRET_LOOKING_LINE"));
    }
}

//! Terminal sanitization for untrusted strings and paths.
//!
//! `SEC-006`: every untrusted string rendered to a terminal, including labels,
//! paths, key names, and masked previews, occupies one logical line. C0 and C1
//! controls, DEL, escape, bidi controls, and Unicode line or paragraph
//! separators are rendered as visible escapes, and non-UTF-8 path bytes are
//! rendered as `\xNN` rather than emitted raw.
//!
//! Sanitization is a rendering step. Preview selection happens before escaping,
//! so an escape representation never reveals additional source characters.

use std::path::Path;

/// Renders one untrusted string safely for terminal output.
pub fn text(value: &str) -> String {
    let mut rendered = String::with_capacity(value.len());
    for character in value.chars() {
        push_escaped(&mut rendered, character);
    }
    rendered
}

/// Renders an untrusted path safely, including non-UTF-8 bytes.
pub fn path(value: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        bytes(value.as_os_str().as_bytes())
    }
    #[cfg(not(unix))]
    {
        text(&value.to_string_lossy())
    }
}

/// Renders untrusted bytes, escaping anything that is not valid UTF-8.
pub fn bytes(value: &[u8]) -> String {
    let mut rendered = String::with_capacity(value.len());
    let mut remaining = value;
    loop {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                rendered.push_str(&text(valid));
                return rendered;
            }
            Err(error) => {
                let (valid, rest) = remaining.split_at(error.valid_up_to());
                rendered.push_str(&text(
                    std::str::from_utf8(valid).expect("prefix is valid UTF-8"),
                ));
                let invalid_len = error.error_len().unwrap_or(rest.len());
                for byte in &rest[..invalid_len] {
                    rendered.push_str(&format!("\\x{byte:02x}"));
                }
                remaining = &rest[invalid_len..];
            }
        }
    }
}

fn push_escaped(rendered: &mut String, character: char) {
    match character {
        // The escape character itself, so an escaped rendering is unambiguous.
        '\\' => rendered.push_str("\\\\"),
        '\n' => rendered.push_str("\\n"),
        '\r' => rendered.push_str("\\r"),
        '\t' => rendered.push_str("\\t"),
        '\u{1b}' => rendered.push_str("\\e"),
        // Remaining C0 controls and DEL.
        character if (character as u32) < 0x20 || character as u32 == 0x7f => {
            rendered.push_str(&format!("\\x{:02x}", character as u32));
        }
        // C1 controls, bidi overrides and isolates, and line or paragraph
        // separators, all of which can rewrite a rendered line.
        character
            if matches!(character as u32,
                0x80..=0x9f
                | 0x200e | 0x200f
                | 0x202a..=0x202e
                | 0x2066..=0x2069
                | 0x2028 | 0x2029) =>
        {
            rendered.push_str(&format!("\\u{{{:04x}}}", character as u32));
        }
        character => rendered.push(character),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_text_is_unchanged() {
        assert_eq!(text("GITHUB_TOKEN"), "GITHUB_TOKEN");
        assert_eq!(
            text("~/projects/app/.env.local"),
            "~/projects/app/.env.local"
        );
        assert_eq!(text("pässwörd✓"), "pässwörd✓");
    }

    #[test]
    fn control_characters_become_visible_escapes() {
        assert_eq!(text("a\nb"), "a\\nb");
        assert_eq!(text("a\rb"), "a\\rb");
        assert_eq!(text("a\tb"), "a\\tb");
        assert_eq!(text("a\u{0}b"), "a\\x00b");
        assert_eq!(text("a\u{7f}b"), "a\\x7fb");
        assert_eq!(text("a\u{1}b"), "a\\x01b");
    }

    #[test]
    fn escape_sequences_cannot_reach_the_terminal() {
        let injected = "\u{1b}[31mred\u{1b}[0m";
        let rendered = text(injected);
        assert_eq!(rendered, "\\e[31mred\\e[0m");
        assert!(!rendered.contains('\u{1b}'));
    }

    #[test]
    fn bidi_and_separator_controls_are_escaped() {
        assert_eq!(text("a\u{202e}b"), "a\\u{202e}b");
        assert_eq!(text("a\u{2066}b"), "a\\u{2066}b");
        assert_eq!(text("a\u{200f}b"), "a\\u{200f}b");
        assert_eq!(text("a\u{2028}b"), "a\\u{2028}b");
        assert_eq!(text("a\u{2029}b"), "a\\u{2029}b");
        assert_eq!(text("a\u{85}b"), "a\\u{0085}b");
    }

    #[test]
    fn backslashes_are_escaped_so_rendering_is_unambiguous() {
        assert_eq!(text("a\\nb"), "a\\\\nb");
        assert_ne!(text("a\\nb"), text("a\nb"));
    }

    #[test]
    fn every_rendering_occupies_one_logical_line() {
        for input in [
            "a\nb",
            "a\rb",
            "a\u{2028}b",
            "a\u{2029}b",
            "a\u{85}b",
            "a\u{b}b",
            "a\u{c}b",
        ] {
            let rendered = text(input);
            assert!(
                !rendered.contains([
                    '\n', '\r', '\u{b}', '\u{c}', '\u{85}', '\u{2028}', '\u{2029}'
                ]),
                "`{input:?}` rendered as more than one logical line"
            );
        }
    }

    #[test]
    fn invalid_utf8_bytes_are_escaped() {
        assert_eq!(bytes(b"ok"), "ok");
        assert_eq!(bytes(&[b'a', 0xff, b'b']), "a\\xffb");
        assert_eq!(bytes(&[0xc3, 0x28]), "\\xc3(");
        assert_eq!(bytes(&[0xed, 0xa0, 0x80]), "\\xed\\xa0\\x80");
    }

    #[test]
    #[cfg(unix)]
    fn non_utf8_paths_are_rendered_without_raw_bytes() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let raw = OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff, 0xfe, b'/', 0x1b]);
        let rendered = path(std::path::Path::new(&raw));
        assert_eq!(rendered, "/tmp/\\xff\\xfe/\\e");
        assert!(!rendered.contains('\u{1b}'));
    }
}

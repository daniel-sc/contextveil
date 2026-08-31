//! Masked value previews.
//!
//! `SET-010`: complete candidate plaintext must never be shown. Length and the
//! first and last units are counted in Unicode scalar values, not UTF-8 bytes or
//! grapheme clusters:
//!
//! | Character length | Preview |
//! | --- | --- |
//! | 0-4 | fully masked |
//! | 5-15 | first 2 and last 2 characters |
//! | 16-39 | first 4 and last 4 characters |
//! | 40+ | first 4 and last 4 characters, with a compact skipped-character marker |
//!
//! Deterministic value fingerprints are forbidden, so nothing here hashes a
//! value. Selection happens before escaping, so an escape representation cannot
//! reveal additional source characters (`SEC-006`).

use crate::sanitize;

const MASK: char = '*';
const COMPACT_PREVIEW_THRESHOLD: usize = 40;
const COMPACT_MASK_COUNT: usize = 4;

/// Renders a masked, terminal-safe preview with its character length.
pub fn describe(value: &str) -> String {
    let length = value.chars().count();
    let plural = if length == 1 {
        "character"
    } else {
        "characters"
    };
    format!("{} ({length} {plural})", mask(value))
}

/// Masks a value according to `SET-010` and escapes the result.
pub fn mask(value: &str) -> String {
    let characters: Vec<char> = value.chars().collect();
    let length = characters.len();
    let revealed = match length {
        0..=4 => 0,
        5..=15 => 2,
        _ => 4,
    };

    let mut preview = String::new();
    if revealed == 0 {
        // Escaping happens after selection, so a fully masked short value shows
        // only mask characters regardless of its content.
        return MASK.to_string().repeat(length);
    }
    preview.extend(characters.iter().take(revealed));
    let masked = length - revealed * 2;
    if length >= COMPACT_PREVIEW_THRESHOLD {
        preview.push_str(&MASK.to_string().repeat(COMPACT_MASK_COUNT));
        preview.push_str(&format!(
            "...skipped {} chars...",
            masked - COMPACT_MASK_COUNT * 2
        ));
        preview.push_str(&MASK.to_string().repeat(COMPACT_MASK_COUNT));
    } else {
        preview.push_str(&MASK.to_string().repeat(masked));
    }
    preview.extend(characters.iter().skip(length - revealed));
    sanitize::text(&preview)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_values_are_fully_masked() {
        assert_eq!(mask(""), "");
        assert_eq!(mask("a"), "*");
        assert_eq!(mask("abcd"), "****");
    }

    #[test]
    fn medium_values_reveal_two_characters_at_each_end() {
        assert_eq!(mask("abcde"), "ab*de");
        assert_eq!(mask("0123456789abcde"), "01***********de");
        assert_eq!(mask("0123456789abcde").chars().count(), 15);
    }

    #[test]
    fn long_values_reveal_four_characters_at_each_end() {
        assert_eq!(mask("0123456789abcdef"), "0123********cdef");
        let long = "x".repeat(64);
        assert_eq!(mask(&long), "xxxx****...skipped 48 chars...****xxxx");
    }

    #[test]
    fn compact_previews_start_at_forty_characters() {
        assert_eq!(
            mask(&"y".repeat(39)),
            "yyyy*******************************yyyy"
        );
        assert_eq!(
            mask(&"y".repeat(40)),
            "yyyy****...skipped 24 chars...****yyyy"
        );
    }

    #[test]
    fn boundaries_follow_the_specified_table() {
        assert_eq!(mask(&"y".repeat(4)), "****");
        assert_eq!(mask(&"y".repeat(5)), "yy*yy");
        assert_eq!(mask(&"y".repeat(15)), "yy***********yy");
        assert_eq!(mask(&"y".repeat(16)), "yyyy********yyyy");
    }

    #[test]
    fn length_is_counted_in_unicode_scalar_values() {
        // Four scalar values, ten UTF-8 bytes: still fully masked.
        assert_eq!(mask("äöüß"), "****");
        // Five scalar values reveal the first and last two.
        assert_eq!(mask("äöüßé"), "äö*ßé");
        assert_eq!(describe("äöüß"), "**** (4 characters)");
    }

    #[test]
    fn previews_are_terminal_safe() {
        let hostile = format!("ab{}[31mcdefghij", '\u{1b}');
        let preview = mask(&hostile);
        assert!(!preview.contains('\u{1b}'));
        assert!(preview.starts_with("ab"));
        assert!(preview.ends_with("ij"));
    }

    #[test]
    fn a_preview_never_reveals_more_than_the_table_allows() {
        let value = "0123456789abcdef";
        let preview = mask(value);
        for revealed in ["4567", "89ab"] {
            assert!(!preview.contains(revealed));
        }
    }

    #[test]
    fn no_fingerprint_is_derived_from_the_value() {
        // Two different values of equal length mask identically apart from the
        // revealed characters, so nothing about the middle is disclosed.
        assert_eq!(mask("abZZZZZZZZZZZcd"), mask("abYYYYYYYYYYYcd"));
    }

    #[test]
    fn descriptions_report_the_character_length() {
        assert_eq!(describe("a"), "* (1 character)");
        assert_eq!(describe("abcdefghij"), "ab******ij (10 characters)");
    }
}

//! Resolved values and their emit-safe identities.
//!
//! `REG-003` and `REG-004` fix how a label is derived: from the key or name
//! only, never from a path, and reduced to a conservative character set before
//! it can reach a placeholder or a terminal.

/// Identity of one enrolled source, used for diagnostics and deduplication.
///
/// Extended with dotenv variants by `T020`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SourceId {
    /// An environment variable inherited by the hook process.
    Env { name: String },
}

impl SourceId {
    pub fn env(name: impl Into<String>) -> Self {
        SourceId::Env { name: name.into() }
    }

    /// The key or name a label derives from. Never a path (`REG-003`).
    pub fn key(&self) -> &str {
        match self {
            SourceId::Env { name } => name,
        }
    }

    /// Emit-safe label for this source.
    pub fn label(&self) -> String {
        safe_label(self.key())
    }
}

/// A current, non-empty UTF-8 value obtained from an enrolled source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSecret {
    pub value: String,
    pub label: String,
    pub source: SourceId,
}

impl ResolvedSecret {
    pub fn new(source: SourceId, value: String) -> Self {
        let label = source.label();
        Self {
            value,
            label,
            source,
        }
    }
}

/// Reduces a key or name to the `REG-004` label character set.
///
/// ASCII letters, digits, `_`, `-`, and `.` are preserved; every other
/// non-empty run collapses to a single `_`. Labels need not be unique.
pub fn safe_label(name: &str) -> String {
    let mut label = String::with_capacity(name.len());
    let mut in_replaced_run = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.') {
            label.push(character);
            in_replaced_run = false;
        } else if !in_replaced_run {
            label.push('_');
            in_replaced_run = true;
        }
    }
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_keep_only_the_allowed_character_set() {
        assert_eq!(safe_label("GITHUB_TOKEN"), "GITHUB_TOKEN");
        assert_eq!(safe_label("api.key-1"), "api.key-1");
        assert_eq!(safe_label("weird key!!name"), "weird_key_name");
        assert_eq!(safe_label("ünïcode"), "_n_code");
        assert_eq!(safe_label("   "), "_");
        assert_eq!(safe_label(""), "");
    }

    #[test]
    fn labels_collapse_control_and_escape_sequences() {
        // Terminal-hostile input must not survive into a placeholder.
        assert_eq!(safe_label("A\u{1b}[31mB"), "A_31mB");
        assert_eq!(safe_label("line\nbreak"), "line_break");
        assert_eq!(safe_label("bidi\u{202e}override"), "bidi_override");
    }

    #[test]
    fn labels_derive_from_the_key_only() {
        let source = SourceId::env("GITHUB_TOKEN");
        assert_eq!(source.label(), "GITHUB_TOKEN");
        assert_eq!(source.key(), "GITHUB_TOKEN");
    }

    #[test]
    fn case_is_preserved_because_names_are_case_sensitive() {
        assert_eq!(safe_label("Token"), "Token");
        assert_ne!(safe_label("Token"), safe_label("TOKEN"));
    }
}

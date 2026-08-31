//! Resolved values and their emit-safe identities.
//!
//! `REG-003` and `REG-004` fix how a label is derived: from the key or name
//! only, never from a path, and reduced to a conservative character set before
//! it can reach a placeholder or a terminal.

use std::cmp::Ordering;
use std::path::PathBuf;

/// Identity of one enrolled source (`CFG-006`).
///
/// The path is already expanded and lexically normalized, without filesystem
/// canonicalization or symlink resolution, so identity does not depend on
/// filesystem state.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SourceId {
    /// An environment variable inherited by the hook process.
    Env { name: String },
    /// One key in a dotenv file.
    DotenvKey { path: PathBuf, key: String },
    /// Every current key in a dotenv file.
    DotenvAll { path: PathBuf },
    /// One exact RFC 6901 pointer in a JSON file.
    Json { path: PathBuf, pointer: String },
    /// One decoded key in a Java properties file.
    Properties { path: PathBuf, key: String },
    /// One exact key in an npmrc file.
    Npmrc { path: PathBuf, key: String },
}

impl SourceId {
    pub fn env(name: impl Into<String>) -> Self {
        SourceId::Env { name: name.into() }
    }

    pub fn dotenv_key(path: PathBuf, key: impl Into<String>) -> Self {
        SourceId::DotenvKey {
            path,
            key: key.into(),
        }
    }

    pub fn dotenv_all(path: PathBuf) -> Self {
        SourceId::DotenvAll { path }
    }

    pub fn json(path: PathBuf, pointer: impl Into<String>) -> Self {
        SourceId::Json {
            path,
            pointer: pointer.into(),
        }
    }

    pub fn properties(path: PathBuf, key: impl Into<String>) -> Self {
        SourceId::Properties {
            path,
            key: key.into(),
        }
    }

    pub fn npmrc(path: PathBuf, key: impl Into<String>) -> Self {
        SourceId::Npmrc {
            path,
            key: key.into(),
        }
    }

    /// Emit-safe label for this source, when it has a key (`REG-003`).
    pub fn label(&self) -> Option<String> {
        match self {
            SourceId::Env { name } => Some(safe_label(name)),
            SourceId::DotenvKey { key, .. } => Some(safe_label(key)),
            SourceId::DotenvAll { .. } => None,
            SourceId::Json { pointer, .. } => crate::json::final_token(pointer)
                .ok()
                .map(|token| safe_label(&token)),
            SourceId::Properties { key, .. } => Some(safe_label(key)),
            SourceId::Npmrc { key, .. } => Some(safe_label(npmrc_label(key))),
        }
    }

    /// The file this identity refers to, if any.
    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            SourceId::Env { .. } => None,
            SourceId::DotenvKey { path, .. }
            | SourceId::DotenvAll { path }
            | SourceId::Json { path, .. }
            | SourceId::Properties { path, .. }
            | SourceId::Npmrc { path, .. } => Some(path),
        }
    }

    fn kind_order(&self) -> u8 {
        match self {
            SourceId::Env { .. } => 0,
            SourceId::DotenvKey { .. } => 1,
            SourceId::DotenvAll { .. } => 2,
            SourceId::Json { .. } => 3,
            SourceId::Properties { .. } => 4,
            SourceId::Npmrc { .. } => 5,
        }
    }
}

impl Ord for SourceId {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (SourceId::Env { name: left }, SourceId::Env { name: right }) => left.cmp(right),
            (
                SourceId::DotenvKey {
                    path: left_path,
                    key: left_key,
                },
                SourceId::DotenvKey {
                    path: right_path,
                    key: right_key,
                },
            ) => left_path.cmp(right_path).then(left_key.cmp(right_key)),
            (SourceId::DotenvAll { path: left }, SourceId::DotenvAll { path: right }) => {
                left.cmp(right)
            }
            (
                SourceId::Json {
                    path: left_path,
                    pointer: left_pointer,
                },
                SourceId::Json {
                    path: right_path,
                    pointer: right_pointer,
                },
            ) => left_path
                .cmp(right_path)
                .then(left_pointer.cmp(right_pointer)),
            (
                SourceId::Properties {
                    path: left_path,
                    key: left_key,
                },
                SourceId::Properties {
                    path: right_path,
                    key: right_key,
                },
            ) => left_path.cmp(right_path).then(left_key.cmp(right_key)),
            (
                SourceId::Npmrc {
                    path: left_path,
                    key: left_key,
                },
                SourceId::Npmrc {
                    path: right_path,
                    key: right_key,
                },
            ) => left_path.cmp(right_path).then(left_key.cmp(right_key)),
            _ => self.kind_order().cmp(&other.kind_order()),
        }
    }
}

/// Final colon-delimited npmrc field used for labels and generic name gating.
pub fn npmrc_label(key: &str) -> &str {
    key.rsplit_once(':').map_or(key, |(_, field)| field)
}

impl PartialOrd for SourceId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
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
    /// Builds a resolved secret.
    ///
    /// Only keyed identities resolve to a value, so the label is always
    /// derivable; an identity without a key yields an empty label, which the
    /// matcher then treats as unnamed rather than emitting `<SECRET:>`.
    pub fn new(source: SourceId, value: String) -> Self {
        let label = source.label().unwrap_or_default();
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
        assert_eq!(
            SourceId::env("GITHUB_TOKEN").label().as_deref(),
            Some("GITHUB_TOKEN")
        );
        assert_eq!(
            SourceId::dotenv_key(PathBuf::from("/secret/path/.env"), "API_KEY")
                .label()
                .as_deref(),
            Some("API_KEY")
        );
        // A wildcard entry has no key, so it has no label.
        assert_eq!(SourceId::dotenv_all(PathBuf::from("/x/.env")).label(), None);
        assert_eq!(
            SourceId::json(PathBuf::from("/secret/auth.json"), "/a~1b")
                .label()
                .as_deref(),
            Some("a_b")
        );
        assert_eq!(
            SourceId::npmrc(
                PathBuf::from("/secret/.npmrc"),
                "//registry.example/:_authToken"
            )
            .label()
            .as_deref(),
            Some("_authToken")
        );
        assert_eq!(
            SourceId::npmrc(PathBuf::from("/secret/.npmrc"), "token")
                .label()
                .as_deref(),
            Some("token")
        );
    }

    #[test]
    fn identities_distinguish_source_kinds_and_json_pointers() {
        let path = PathBuf::from("/project/.env");
        assert_ne!(
            SourceId::dotenv_key(path.clone(), "A"),
            SourceId::dotenv_all(path.clone())
        );
        assert_ne!(
            SourceId::dotenv_key(path.clone(), "A"),
            SourceId::dotenv_key(path.clone(), "B")
        );
        assert_ne!(SourceId::env("A"), SourceId::env("a"));
        assert_ne!(
            SourceId::json(path.clone(), "/A"),
            SourceId::json(path, "/a")
        );
    }

    #[test]
    fn identities_use_the_contractual_total_order() {
        let path = PathBuf::from("/project/source");
        let mut identities = vec![
            SourceId::npmrc(path.clone(), "//z/:_authToken"),
            SourceId::npmrc(path.clone(), "//a/:_authToken"),
            SourceId::properties(path.clone(), "z"),
            SourceId::json(path.clone(), "/b"),
            SourceId::dotenv_all(path.clone()),
            SourceId::dotenv_key(path.clone(), "B"),
            SourceId::env("B"),
            SourceId::json(path.clone(), "/a"),
            SourceId::dotenv_key(path, "A"),
            SourceId::env("A"),
        ];

        identities.sort();

        assert_eq!(
            identities,
            vec![
                SourceId::env("A"),
                SourceId::env("B"),
                SourceId::dotenv_key(PathBuf::from("/project/source"), "A"),
                SourceId::dotenv_key(PathBuf::from("/project/source"), "B"),
                SourceId::dotenv_all(PathBuf::from("/project/source")),
                SourceId::json(PathBuf::from("/project/source"), "/a"),
                SourceId::json(PathBuf::from("/project/source"), "/b"),
                SourceId::properties(PathBuf::from("/project/source"), "z"),
                SourceId::npmrc(PathBuf::from("/project/source"), "//a/:_authToken"),
                SourceId::npmrc(PathBuf::from("/project/source"), "//z/:_authToken"),
            ]
        );
    }

    #[test]
    fn case_is_preserved_because_names_are_case_sensitive() {
        assert_eq!(safe_label("Token"), "Token");
        assert_ne!(safe_label("Token"), safe_label("TOKEN"));
    }
}

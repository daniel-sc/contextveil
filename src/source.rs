//! Source references and their resolution.
//!
//! `SRC-001` and `SRC-002` govern environment resolution: case-sensitive names,
//! and unset, empty, or non-UTF-8 values are unresolved rather than failures.
//! Dotenv resolution arrives with `T020`.
//!
//! Values are resolved afresh for every event and never cached across processes
//! (`SRC-009`).

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};

use crate::secret::{ResolvedSecret, SourceId};

/// One enrolled source reference from a configuration file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRef {
    Env { name: String },
}

impl SourceRef {
    pub fn id(&self) -> SourceId {
        match self {
            SourceRef::Env { name } => SourceId::env(name.clone()),
        }
    }
}

/// Why a source has no usable value right now.
///
/// An unresolved source is normal and stays silent during runtime
/// (`RED-009`); it is not a malfunction (`SRC-005`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unresolved {
    /// The environment variable is not set.
    Absent,
    /// The value exists but is empty.
    Empty,
    /// The value is not valid UTF-8 and must not enter the matcher.
    NonUtf8,
}

/// Result of resolving one source reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Resolved(ResolvedSecret),
    Unresolved { source: SourceId, why: Unresolved },
}

/// The environment a run resolves against.
///
/// A snapshot is taken once per process so tests can supply a fixed
/// environment without mutating process state.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    variables: HashMap<OsString, OsString>,
}

impl Environment {
    /// Snapshots the environment inherited by this process.
    pub fn from_process() -> Self {
        Self {
            variables: std::env::vars_os().collect(),
        }
    }

    /// Builds a fixed environment for tests and synthetic checks.
    pub fn from_pairs<K, V, I>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<OsString>,
        V: Into<OsString>,
    {
        Self {
            variables: pairs
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.variables
            .get(OsStr::new(name))
            .map(OsString::as_os_str)
    }

    /// Returns a UTF-8 variable value, or `None` when unset or undecodable.
    pub fn get_str(&self, name: &str) -> Option<&str> {
        self.get(name).and_then(OsStr::to_str)
    }
}

/// Resolves one source reference against the current environment.
pub fn resolve(source: &SourceRef, environment: &Environment) -> Resolution {
    match source {
        SourceRef::Env { name } => {
            let id = SourceId::env(name.clone());
            match environment.get(name) {
                None => Resolution::Unresolved {
                    source: id,
                    why: Unresolved::Absent,
                },
                Some(raw) => match raw.to_str() {
                    None => Resolution::Unresolved {
                        source: id,
                        why: Unresolved::NonUtf8,
                    },
                    Some("") => Resolution::Unresolved {
                        source: id,
                        why: Unresolved::Empty,
                    },
                    Some(value) => Resolution::Resolved(ResolvedSecret::new(id, value.to_string())),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Canary;

    fn env_ref(name: &str) -> SourceRef {
        SourceRef::Env {
            name: name.to_string(),
        }
    }

    #[test]
    fn a_present_value_resolves_with_a_safe_label() {
        let canary = Canary::generate("GITHUB_TOKEN");
        let environment = Environment::from_pairs([("GITHUB_TOKEN", canary.value())]);
        match resolve(&env_ref("GITHUB_TOKEN"), &environment) {
            Resolution::Resolved(secret) => {
                assert_eq!(secret.value, canary.value());
                assert_eq!(secret.label, "GITHUB_TOKEN");
            }
            other => panic!("expected a resolved secret, got {other:?}"),
        }
    }

    #[test]
    fn names_are_case_sensitive() {
        let environment = Environment::from_pairs([("TOKEN", "value")]);
        assert!(matches!(
            resolve(&env_ref("token"), &environment),
            Resolution::Unresolved {
                why: Unresolved::Absent,
                ..
            }
        ));
    }

    #[test]
    fn unset_and_empty_values_are_unresolved() {
        let environment = Environment::from_pairs([("EMPTY", "")]);
        assert!(matches!(
            resolve(&env_ref("MISSING"), &environment),
            Resolution::Unresolved {
                why: Unresolved::Absent,
                ..
            }
        ));
        assert!(matches!(
            resolve(&env_ref("EMPTY"), &environment),
            Resolution::Unresolved {
                why: Unresolved::Empty,
                ..
            }
        ));
    }

    #[test]
    #[cfg(unix)]
    fn non_utf8_values_are_unresolved_and_never_enter_the_matcher() {
        use std::os::unix::ffi::OsStringExt;
        let invalid = OsString::from_vec(vec![b'a', 0xff, b'b']);
        let environment = Environment::from_pairs([(OsString::from("BINARY"), invalid)]);
        assert!(matches!(
            resolve(&env_ref("BINARY"), &environment),
            Resolution::Unresolved {
                why: Unresolved::NonUtf8,
                ..
            }
        ));
    }

    #[test]
    fn the_process_snapshot_reads_inherited_variables() {
        let environment = Environment::from_process();
        // PATH is present in every supported development and hook environment.
        assert!(environment.get("PATH").is_some());
    }
}

//! Configuration file locations, parsing, and validation.
//!
//! `CFG-001` fixes the global path, `CFG-006` through `CFG-008` fix the schema,
//! and `CFG-012` makes parsing strict per file: an invalid or unreadable file
//! disables the whole effective registry rather than contributing part of it.
//!
//! Diagnostics carry a stable classification and a location, never file text.
//! Project configuration is attacker-influenced (`LIM-008`), so parser messages
//! are not echoed. Project files and dotenv sources arrive with `T020`.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::source::{Environment, SourceRef};

/// The only supported configuration schema version (`CFG-006`).
pub const SCHEMA_VERSION: i64 = 1;

/// A validated configuration file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub sources: Vec<SourceRef>,
}

/// Outcome of loading one configuration file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Load {
    /// The file does not exist. Normal for a project, incomplete setup for the
    /// global file (`CFG-013`).
    Missing,
    Valid(Config),
    Invalid(ConfigError),
}

/// A stable, secret-safe reason a configuration file cannot be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub path: PathBuf,
    pub kind: ConfigErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigErrorKind {
    /// Permission denial or a non-`NotFound` I/O error (`CFG-012`).
    Unreadable,
    /// The file is not valid UTF-8.
    NotUtf8,
    /// TOML syntax error at a one-based position in the file.
    Syntax { line: usize, column: usize },
    /// `version` is absent or is not `1`.
    UnsupportedVersion,
    /// An entry violates `CFG-006` through `CFG-008`.
    InvalidEntry { index: usize, problem: EntryProblem },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryProblem {
    UnknownSourceType,
    MissingName,
    EmptyName,
    /// A field that does not belong to the entry's source type (`CFG-007`).
    UnexpectedField,
    /// The same source identity appears twice in one file (`CFG-006`).
    DuplicateIdentity,
}

impl ConfigErrorKind {
    /// Short reason suitable for a warning. Contains no file content.
    pub fn reason(&self) -> String {
        match self {
            ConfigErrorKind::Unreadable => "the file could not be read".to_string(),
            ConfigErrorKind::NotUtf8 => "the file is not valid UTF-8".to_string(),
            ConfigErrorKind::Syntax { line, column } => {
                format!("invalid TOML syntax at line {line}, column {column}")
            }
            ConfigErrorKind::UnsupportedVersion => {
                format!("`version = {SCHEMA_VERSION}` is required")
            }
            ConfigErrorKind::InvalidEntry { index, problem } => {
                let position = index + 1;
                format!("secret entry {position} {}", problem.reason())
            }
        }
    }
}

impl EntryProblem {
    fn reason(&self) -> &'static str {
        match self {
            EntryProblem::UnknownSourceType => "uses an unknown source type",
            EntryProblem::MissingName => "is missing a required field",
            EntryProblem::EmptyName => "has an empty name",
            EntryProblem::UnexpectedField => "sets a field its source type does not accept",
            EntryProblem::DuplicateIdentity => "duplicates an earlier source identity",
        }
    }
}

/// Returns the global configuration path (`CFG-001`).
///
/// `XDG_CONFIG_HOME` is honored only when it is a non-empty absolute path, as
/// the XDG base directory specification requires.
pub fn global_config_path(environment: &Environment) -> Option<PathBuf> {
    let base = match environment.get_str("XDG_CONFIG_HOME") {
        Some(value) if !value.is_empty() && Path::new(value).is_absolute() => PathBuf::from(value),
        _ => {
            let home = environment.get_str("HOME")?;
            if home.is_empty() {
                return None;
            }
            PathBuf::from(home).join(".config")
        }
    };
    Some(base.join("secretsieve").join("config.toml"))
}

/// Loads and validates one configuration file.
pub fn load(path: &Path) -> Load {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Load::Missing,
        Err(_) => {
            return Load::Invalid(ConfigError {
                path: path.to_path_buf(),
                kind: ConfigErrorKind::Unreadable,
            });
        }
    };
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => {
            return Load::Invalid(ConfigError {
                path: path.to_path_buf(),
                kind: ConfigErrorKind::NotUtf8,
            });
        }
    };
    match parse(&text) {
        Ok(config) => Load::Valid(config),
        Err(kind) => Load::Invalid(ConfigError {
            path: path.to_path_buf(),
            kind,
        }),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: Option<i64>,
    #[serde(default)]
    secret: Vec<RawSecret>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSecret {
    source: String,
    name: Option<String>,
}

/// Parses configuration text strictly (`CFG-006`).
pub fn parse(text: &str) -> Result<Config, ConfigErrorKind> {
    let raw: RawConfig = toml::from_str(text).map_err(|error| {
        let (line, column) = error
            .span()
            .map(|span| position_of(text, span.start))
            .unwrap_or((1, 1));
        ConfigErrorKind::Syntax { line, column }
    })?;

    if raw.version != Some(SCHEMA_VERSION) {
        return Err(ConfigErrorKind::UnsupportedVersion);
    }

    let mut sources: Vec<SourceRef> = Vec::with_capacity(raw.secret.len());
    for (index, entry) in raw.secret.iter().enumerate() {
        let invalid = |problem| ConfigErrorKind::InvalidEntry { index, problem };
        let source = match entry.source.as_str() {
            "env" => {
                let name = entry
                    .name
                    .as_deref()
                    .ok_or(invalid(EntryProblem::MissingName))?;
                if name.is_empty() {
                    return Err(invalid(EntryProblem::EmptyName));
                }
                SourceRef::Env {
                    name: name.to_string(),
                }
            }
            _ => return Err(invalid(EntryProblem::UnknownSourceType)),
        };
        if sources.contains(&source) {
            return Err(invalid(EntryProblem::DuplicateIdentity));
        }
        sources.push(source);
    }

    Ok(Config { sources })
}

/// Converts a byte offset into a one-based line and column.
fn position_of(text: &str, offset: usize) -> (usize, usize) {
    let clamped = offset.min(text.len());
    let prefix = &text[..clamped];
    let line = prefix.matches('\n').count() + 1;
    let column = prefix
        .rfind('\n')
        .map(|index| prefix[index + 1..].chars().count() + 1)
        .unwrap_or_else(|| prefix.chars().count() + 1);
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_source(name: &str) -> SourceRef {
        SourceRef::Env {
            name: name.to_string(),
        }
    }

    #[test]
    fn a_minimal_global_config_parses() {
        let config = parse(
            r#"
version = 1

[[secret]]
source = "env"
name = "GITHUB_TOKEN"
"#,
        )
        .expect("valid config");
        assert_eq!(config.sources, vec![env_source("GITHUB_TOKEN")]);
    }

    #[test]
    fn an_empty_registry_is_valid() {
        assert_eq!(parse("version = 1\n"), Ok(Config { sources: vec![] }));
    }

    #[test]
    fn the_version_is_required_and_pinned() {
        assert_eq!(
            parse("[[secret]]\nsource = \"env\"\nname = \"A\"\n"),
            Err(ConfigErrorKind::UnsupportedVersion)
        );
        assert_eq!(
            parse("version = 2\n"),
            Err(ConfigErrorKind::UnsupportedVersion)
        );
    }

    #[test]
    fn unknown_fields_invalidate_the_file() {
        assert!(matches!(
            parse("version = 1\nunexpected = true\n"),
            Err(ConfigErrorKind::Syntax { .. })
        ));
        assert!(matches!(
            parse("version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"A\"\nextra = 1\n"),
            Err(ConfigErrorKind::Syntax { .. })
        ));
    }

    #[test]
    fn environment_entries_require_a_non_empty_name() {
        assert_eq!(
            parse("version = 1\n\n[[secret]]\nsource = \"env\"\n"),
            Err(ConfigErrorKind::InvalidEntry {
                index: 0,
                problem: EntryProblem::MissingName
            })
        );
        assert_eq!(
            parse("version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"\"\n"),
            Err(ConfigErrorKind::InvalidEntry {
                index: 0,
                problem: EntryProblem::EmptyName
            })
        );
    }

    #[test]
    fn unknown_source_types_invalidate_the_file() {
        assert_eq!(
            parse("version = 1\n\n[[secret]]\nsource = \"keychain\"\nname = \"A\"\n"),
            Err(ConfigErrorKind::InvalidEntry {
                index: 0,
                problem: EntryProblem::UnknownSourceType
            })
        );
    }

    #[test]
    fn duplicate_identities_in_one_file_are_rejected() {
        let text = "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"A\"\n\n[[secret]]\nsource = \"env\"\nname = \"A\"\n";
        assert_eq!(
            parse(text),
            Err(ConfigErrorKind::InvalidEntry {
                index: 1,
                problem: EntryProblem::DuplicateIdentity
            })
        );
    }

    #[test]
    fn names_are_case_sensitive_so_case_variants_are_distinct() {
        let text = "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"A\"\n\n[[secret]]\nsource = \"env\"\nname = \"a\"\n";
        let config = parse(text).expect("valid config");
        assert_eq!(config.sources.len(), 2);
    }

    #[test]
    fn syntax_errors_report_a_position_and_no_file_text() {
        let error = parse("version = 1\nthis is not toml\n").expect_err("invalid");
        match error {
            ConfigErrorKind::Syntax { line, .. } => assert_eq!(line, 2),
            other => panic!("expected a syntax error, got {other:?}"),
        }
        assert!(!error.reason().contains("this is not toml"));
    }

    #[test]
    fn the_global_path_follows_xdg_rules() {
        let with_xdg = Environment::from_pairs([("XDG_CONFIG_HOME", "/xdg"), ("HOME", "/home/a")]);
        assert_eq!(
            global_config_path(&with_xdg),
            Some(PathBuf::from("/xdg/secretsieve/config.toml"))
        );

        let without_xdg = Environment::from_pairs([("HOME", "/home/a")]);
        assert_eq!(
            global_config_path(&without_xdg),
            Some(PathBuf::from("/home/a/.config/secretsieve/config.toml"))
        );

        // A relative or empty XDG_CONFIG_HOME is ignored, per the XDG spec.
        let relative = Environment::from_pairs([("XDG_CONFIG_HOME", "relative"), ("HOME", "/h")]);
        assert_eq!(
            global_config_path(&relative),
            Some(PathBuf::from("/h/.config/secretsieve/config.toml"))
        );
        let empty = Environment::from_pairs([("XDG_CONFIG_HOME", ""), ("HOME", "")]);
        assert_eq!(global_config_path(&empty), None);
    }

    #[test]
    fn a_missing_file_is_not_an_error() {
        let path = std::env::temp_dir().join("secretsieve-missing-config-does-not-exist.toml");
        assert_eq!(load(&path), Load::Missing);
    }
}

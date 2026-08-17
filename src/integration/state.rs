//! Ownership and acknowledgement state for installed integrations.
//!
//! `architecture.md`: integration ownership metadata lives beside the global
//! policy file so the policy TOML stays comprehensible. It records only what
//! SecretSieve installed and what the user approved. It never contains a
//! resolved value and is never treated as proof of health (`INT-006`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File name inside the global SecretSieve configuration directory.
pub const STATE_FILENAME: &str = "integrations.toml";

/// Recorded state for every integration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    #[serde(default = "one")]
    pub version: i64,
    /// The Claude integration, when SecretSieve installed one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude: Option<Managed>,
}

fn one() -> i64 {
    1
}

/// What SecretSieve installed for one harness, and what the user approved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Managed {
    /// The exact command string that was installed.
    pub command: String,
    /// Competing mutating hooks the user approved (`INT-005`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approved_conflicts: Vec<String>,
}

/// Path of the state file next to the global configuration file.
pub fn path(global_config_path: &Path) -> PathBuf {
    global_config_path.with_file_name(STATE_FILENAME)
}

/// Loads recorded state.
///
/// A missing or unreadable file yields empty state: ownership then cannot be
/// established from the record alone, which is the safe direction because
/// removal also requires the artifact to match its managed shape.
pub fn load(path: &Path) -> State {
    let Ok(text) = std::fs::read_to_string(path) else {
        return State::default();
    };
    toml::from_str(&text).unwrap_or_default()
}

/// Writes state atomically with user-only permissions.
pub fn save(path: &Path, state: &State) -> Result<(), crate::setup::write::WriteError> {
    let contents =
        toml::to_string_pretty(state).map_err(|_| crate::setup::write::WriteError::Serialize)?;
    crate::setup::write::write_text(path, &contents, true).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Canary;

    fn temporary() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "secretsieve-state-{}-{}",
            std::process::id(),
            Canary::generate("STATE").token()
        ));
        std::fs::create_dir_all(&root).expect("fixture root");
        root
    }

    #[test]
    fn state_round_trips() {
        let root = temporary();
        let file = path(&root.join("config.toml"));
        assert_eq!(file.file_name().expect("name"), STATE_FILENAME);

        let state = State {
            version: 1,
            claude: Some(Managed {
                command: "/opt/secretsieve hook claude".to_string(),
                approved_conflicts: vec!["/other/hook".to_string()],
            }),
        };
        save(&file, &state).expect("save");
        assert_eq!(load(&file), state);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_or_malformed_file_yields_empty_state() {
        let root = temporary();
        let file = root.join("integrations.toml");
        assert_eq!(load(&file), State::default());
        std::fs::write(&file, "this is not toml {{{").expect("write malformed state");
        assert_eq!(load(&file), State::default());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unknown_future_fields_do_not_discard_known_state() {
        let root = temporary();
        let file = root.join("integrations.toml");
        std::fs::write(
            &file,
            "version = 1\nfuture_field = true\n\n[claude]\ncommand = \"/x hook claude\"\n",
        )
        .expect("write state");
        let state = load(&file);
        assert_eq!(
            state.claude.expect("claude state").command,
            "/x hook claude"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    #[cfg(unix)]
    fn the_state_file_is_user_only() {
        use std::os::unix::fs::PermissionsExt;
        let root = temporary();
        let file = root.join("integrations.toml");
        save(&file, &State::default()).expect("save");
        let mode = std::fs::metadata(&file)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let _ = std::fs::remove_dir_all(&root);
    }
}

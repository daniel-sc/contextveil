//! Ownership and acknowledgement state for installed integrations.
//!
//! `architecture.md`: integration ownership metadata lives beside the global
//! policy file so the policy TOML stays comprehensible. It records only what
//! ContextVeil installed and what the user approved. It never contains a
//! resolved value and is never treated as proof of health (`INT-006`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::integration::Harness;

/// File name inside the global ContextVeil configuration directory.
pub const STATE_FILENAME: &str = "integrations.toml";

/// Recorded state for every integration.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    #[serde(default = "one")]
    pub version: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude: Option<Managed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex: Option<Managed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copilot: Option<Managed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opencode: Option<Managed>,
}

fn one() -> i64 {
    1
}

/// What ContextVeil installed for one harness, and what the user approved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Managed {
    /// The exact command or artifact identity that was installed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    /// Competing mutating hooks the user approved (`INT-005`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approved_conflicts: Vec<String>,
}

impl State {
    pub fn get(&self, harness: Harness) -> Option<&Managed> {
        match harness {
            Harness::Claude => self.claude.as_ref(),
            Harness::Codex => self.codex.as_ref(),
            Harness::Copilot => self.copilot.as_ref(),
            Harness::OpenCode => self.opencode.as_ref(),
        }
    }

    /// Mutable state for one harness, created empty when absent.
    pub fn entry(&mut self, harness: Harness) -> &mut Managed {
        match harness {
            Harness::Claude => self.claude.get_or_insert_with(Managed::default),
            Harness::Codex => self.codex.get_or_insert_with(Managed::default),
            Harness::Copilot => self.copilot.get_or_insert_with(Managed::default),
            Harness::OpenCode => self.opencode.get_or_insert_with(Managed::default),
        }
    }

    pub fn set(&mut self, harness: Harness, managed: Option<Managed>) {
        match harness {
            Harness::Claude => self.claude = managed,
            Harness::Codex => self.codex = managed,
            Harness::Copilot => self.copilot = managed,
            Harness::OpenCode => self.opencode = managed,
        }
    }

    /// Conflicts the user approved for one harness.
    pub fn approved_conflicts(&self, harness: Harness) -> Vec<String> {
        self.get(harness)
            .map(|managed| managed.approved_conflicts.clone())
            .unwrap_or_default()
    }
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
            "contextveil-state-{}-{}",
            std::process::id(),
            Canary::generate("STATE").token()
        ));
        std::fs::create_dir_all(&root).expect("fixture root");
        root
    }

    #[test]
    fn state_round_trips_for_every_harness() {
        let root = temporary();
        let file = path(&root.join("config.toml"));
        assert_eq!(file.file_name().expect("name"), STATE_FILENAME);

        let mut state = State::default();
        for harness in crate::integration::HARNESSES {
            state.set(
                harness,
                Some(Managed {
                    command: format!("/opt/contextveil hook {harness:?}"),
                    approved_conflicts: vec!["/other/hook".to_string()],
                }),
            );
        }
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
            state.get(Harness::Claude).expect("claude state").command,
            "/x hook claude"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn approvals_are_recorded_per_harness() {
        let mut state = State::default();
        state
            .entry(Harness::Codex)
            .approved_conflicts
            .push("/other".to_string());
        assert_eq!(state.approved_conflicts(Harness::Codex), ["/other"]);
        assert!(state.approved_conflicts(Harness::Claude).is_empty());
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

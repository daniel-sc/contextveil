//! Harness integration installation and inspection.
//!
//! Installers operate through documented host configuration surfaces, identify
//! their exact managed artifact, and preserve unrelated user configuration
//! (`architecture.md`, `INT-004`). Observed state, not persisted lifecycle
//! flags, determines whether an adapter is installed or functioning
//! (`INT-006`); the state file here records only ownership and user intent.
//!
//! Dispatch is a plain `match` over a small enum rather than a plugin framework.

pub mod claude;
pub mod codex;
pub mod hooks_json;
pub mod state;

use std::path::{Path, PathBuf};

use hooks_json::{Conflict, Installed};
use state::State;

use crate::source::Environment;

/// A supported coding agent (`SUP-002`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Harness {
    Claude,
    Codex,
}

/// Every harness setup and diagnostics know about, in presentation order.
pub const HARNESSES: [Harness; 2] = [Harness::Claude, Harness::Codex];

/// Support tier (`SUP-002`, `SUP-003`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Production,
    /// Functional and fixture-tested, but outside the support promise. It must
    /// be labeled everywhere it appears and must never be selected by default.
    Experimental,
}

impl Harness {
    pub fn label(self) -> &'static str {
        match self {
            Harness::Claude => "Claude Code",
            Harness::Codex => "Codex CLI",
        }
    }

    pub fn tier(self) -> Tier {
        match self {
            Harness::Claude => Tier::Production,
            Harness::Codex => Tier::Experimental,
        }
    }

    pub fn tier_label(self) -> &'static str {
        match self.tier() {
            Tier::Production => "production",
            Tier::Experimental => "EXPERIMENTAL",
        }
    }

    /// Extra step the host requires after installation, if any.
    pub fn post_install_note(self) -> Option<&'static str> {
        match self {
            Harness::Claude => None,
            // Verified against openai/codex: a newly added or changed hook is
            // `Untrusted` until the user reviews it, and untrusted hooks do not
            // run (`COD-001`).
            Harness::Codex => Some(
                "Codex will not run this hook until you trust it: start Codex and choose \
                 \"Trust all and continue\" on the \"Hooks need review\" screen, or review it in \
                 `/hooks`.",
            ),
        }
    }
}

/// Whether a harness looks present on this machine (`INT-001`, `INT-002`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detection {
    /// An executable or configuration directory was found.
    Detected,
    /// Nothing was found. Installation is still allowed, with disclosure.
    NotDetected,
}

/// Result of the offline synthetic protocol check (`INT-006`, `DIA-006`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verification {
    Passed,
    Failed(&'static str),
}

/// Everything setup and doctor need to know about one integration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspection {
    pub harness: Harness,
    /// The host file SecretSieve manages.
    pub artifact_path: PathBuf,
    pub detection: Detection,
    pub installed: Installed,
    pub conflicts: Vec<Conflict>,
    /// Executable path recorded in the installed artifact, for doctor's
    /// existence check.
    pub hook_executable: Option<PathBuf>,
    /// Timeout recorded in the installed artifact, checked against `RUN-004`.
    pub hook_timeout: Option<u64>,
    /// True when a host-level policy disables hooks entirely.
    pub disabled_by_policy: bool,
}

impl Inspection {
    /// True when a managed artifact exists that SecretSieve may rewrite.
    pub fn is_installed(&self) -> bool {
        matches!(
            self.installed,
            Installed::Current | Installed::Outdated { .. }
        )
    }
}

/// Inspects one integration without changing anything.
pub fn inspect(
    harness: Harness,
    environment: &Environment,
    home: &Path,
    executable: Option<&Path>,
    state: &State,
) -> Inspection {
    match harness {
        Harness::Claude => claude::inspect(environment, home, executable, state),
        Harness::Codex => codex::inspect(environment, home, executable, state),
    }
}

/// Installs or updates one integration.
pub fn install(
    harness: Harness,
    home: &Path,
    executable: &Path,
    state: &mut State,
) -> Result<(), InstallError> {
    match harness {
        Harness::Claude => claude::install(home, executable, state),
        Harness::Codex => codex::install(home, executable, state),
    }
}

/// Removes one integration.
///
/// Returns `Ok(false)` when a modified artifact was preserved (`INT-004`).
pub fn remove(harness: Harness, home: &Path, state: &mut State) -> Result<bool, InstallError> {
    match harness {
        Harness::Claude => claude::remove(home, state),
        Harness::Codex => codex::remove(home, state),
    }
}

/// Runs the offline synthetic protocol check for one integration.
pub fn verify_offline(harness: Harness, executable: &Path) -> Verification {
    match harness {
        Harness::Claude => claude::verify_offline(executable),
        Harness::Codex => codex::verify_offline(executable),
    }
}

/// Records the user's approval of a competing hook (`INT-005`).
pub fn approve_conflict(harness: Harness, state: &mut State, command: &str) {
    let managed = state.entry(harness);
    if !managed
        .approved_conflicts
        .iter()
        .any(|known| known == command)
    {
        managed.approved_conflicts.push(command.to_string());
    }
}

/// Why an installation or removal could not complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallError {
    Unreadable,
    Unexpected,
    Write,
    ExecutablePath,
}

impl InstallError {
    pub fn reason(&self) -> &'static str {
        match self {
            InstallError::Unreadable => {
                "the host configuration file is not valid JSON and was left unchanged"
            }
            InstallError::Unexpected => {
                "the host configuration file has an unexpected shape and was left unchanged"
            }
            InstallError::Write => "the host configuration file could not be written",
            InstallError::ExecutablePath => "the SecretSieve binary path could not be determined",
        }
    }
}

impl From<hooks_json::Problem> for InstallError {
    fn from(problem: hooks_json::Problem) -> Self {
        match problem {
            hooks_json::Problem::Unreadable => InstallError::Unreadable,
            hooks_json::Problem::Unexpected => InstallError::Unexpected,
            hooks_json::Problem::Write => InstallError::Write,
        }
    }
}

/// The absolute path of the running binary (`INT-003`).
pub fn current_executable() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// Locates an executable by name on `PATH` without running it.
pub fn find_executable(path_variable: Option<&str>, name: &str) -> Option<PathBuf> {
    let path_variable = path_variable?;
    for directory in path_variable.split(':').filter(|entry| !entry.is_empty()) {
        let candidate = Path::new(directory).join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Detects a harness from its executable name or its configuration directory.
pub fn detect(
    environment: &Environment,
    home: &Path,
    executable: &str,
    directory: &str,
) -> Detection {
    if find_executable(environment.get_str("PATH"), executable).is_some()
        || home.join(directory).is_dir()
    {
        Detection::Detected
    } else {
        Detection::NotDetected
    }
}

/// Quotes a command component for a host that runs the command through a shell.
///
/// `INT-003` forbids shell interpolation. Quoting is what enforces that: a path
/// containing a space, a quote, or a metacharacter must reach the process as one
/// literal argument rather than being re-split or expanded.
pub fn shell_quote(value: &str) -> String {
    let safe = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '@'));
    if safe {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Generates a conspicuous non-credential value for a synthetic check.
pub fn synthetic_canary(kind: &str) -> String {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos() as u64)
            .unwrap_or_default(),
    );
    format!(
        "SSCANARY-{kind}-{}-{:016x}",
        std::process::id(),
        hasher.finish()
    )
}

/// Environment variable name used by every synthetic check.
pub const SYNTHETIC_VARIABLE: &str = "SECRETSIEVE_VERIFY";

/// The placeholder a synthetic check expects to find.
pub const SYNTHETIC_PLACEHOLDER: &str = "<SECRET:SECRETSIEVE_VERIFY>";

/// A temporary configuration directory enrolling only the synthetic variable.
pub struct SyntheticConfig {
    root: PathBuf,
}

impl SyntheticConfig {
    pub fn create(kind: &str) -> Option<Self> {
        let root =
            std::env::temp_dir().join(format!("secretsieve-verify-{}", synthetic_canary(kind)));
        let configuration = root.join("secretsieve");
        std::fs::create_dir_all(&configuration).ok()?;
        std::fs::write(
            configuration.join("config.toml"),
            format!(
                "version = 1\n\n[[secret]]\nsource = \"env\"\nname = \"{SYNTHETIC_VARIABLE}\"\n"
            ),
        )
        .ok()?;
        Some(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for SyntheticConfig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paths_are_not_quoted() {
        assert_eq!(
            shell_quote("/usr/local/bin/secretsieve"),
            "/usr/local/bin/secretsieve"
        );
        assert_eq!(shell_quote("hook"), "hook");
    }

    #[test]
    fn awkward_paths_are_quoted_so_the_shell_cannot_split_or_expand_them() {
        assert_eq!(
            shell_quote("/home/a b/secretsieve"),
            "'/home/a b/secretsieve'"
        );
        assert_eq!(shell_quote("/tmp/$(whoami)/x"), "'/tmp/$(whoami)/x'");
        assert_eq!(shell_quote("/tmp/`id`/x"), "'/tmp/`id`/x'");
        assert_eq!(shell_quote("/tmp/a;rm -rf b"), "'/tmp/a;rm -rf b'");
        assert_eq!(shell_quote("/tmp/it's/x"), r"'/tmp/it'\''s/x'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn executables_are_found_only_where_they_exist() {
        let root = std::env::temp_dir().join(format!(
            "secretsieve-which-{}-{}",
            std::process::id(),
            crate::testing::Canary::generate("WHICH").token()
        ));
        std::fs::create_dir_all(&root).expect("fixture root");
        std::fs::write(root.join("claude"), "#!/bin/sh\n").expect("write fake executable");

        let path = format!("/nonexistent:{}", root.to_string_lossy());
        assert_eq!(
            find_executable(Some(&path), "claude"),
            Some(root.join("claude"))
        );
        assert_eq!(find_executable(Some(&path), "codex"), None);
        assert_eq!(find_executable(None, "claude"), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_claude_is_production() {
        assert_eq!(Harness::Claude.tier(), Tier::Production);
        for harness in HARNESSES {
            if harness != Harness::Claude {
                assert_eq!(harness.tier(), Tier::Experimental, "{}", harness.label());
                assert_eq!(harness.tier_label(), "EXPERIMENTAL");
            }
        }
    }

    #[test]
    fn synthetic_canaries_are_conspicuous_and_unique() {
        let first = synthetic_canary("TEST");
        let second = synthetic_canary("TEST");
        assert!(first.starts_with("SSCANARY-TEST-"));
        assert_ne!(first, second);
    }

    #[test]
    fn a_synthetic_config_enrolls_only_the_verification_variable() {
        let config = SyntheticConfig::create("TEST").expect("config");
        let contents =
            std::fs::read_to_string(config.root().join("secretsieve").join("config.toml"))
                .expect("read config");
        assert!(contents.contains(SYNTHETIC_VARIABLE));
        let root = config.root().to_path_buf();
        drop(config);
        assert!(!root.exists(), "the temporary configuration is removed");
    }
}

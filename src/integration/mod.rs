//! Harness integration installation and inspection.
//!
//! Installers operate through documented host configuration surfaces, identify
//! their exact managed artifact, and preserve unrelated user configuration
//! (`architecture.md`, `INT-004`). Observed state, not persisted lifecycle
//! flags, determines whether an adapter is installed or functioning
//! (`INT-006`); the state file here records only ownership and user intent.

pub mod claude;
pub mod state;

use std::path::PathBuf;

/// Whether a harness looks present on this machine (`INT-001`, `INT-002`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detection {
    /// An executable or configuration directory was found.
    Detected,
    /// Nothing was found. Installation is still allowed, with disclosure.
    NotDetected,
}

/// Locates an executable by name on `PATH` without running it.
pub fn find_executable(path_variable: Option<&str>, name: &str) -> Option<PathBuf> {
    let path_variable = path_variable?;
    for directory in path_variable.split(':').filter(|entry| !entry.is_empty()) {
        let candidate = std::path::Path::new(directory).join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
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
}

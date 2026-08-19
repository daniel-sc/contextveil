//! Path expansion, lexical normalization, and project-root selection.
//!
//! `CFG-010`: paths are stored as entered, a leading `~/` expands to the current
//! user's home, and a relative path resolves against the directory of the config
//! file that named it. Environment-variable, glob, and shell expansion never
//! occur.
//!
//! `CFG-006`: source identity uses the expanded, lexically normalized path with
//! `.` and `..` removed, and deliberately performs no filesystem canonicalization
//! or symlink resolution.

use std::path::{Component, Path, PathBuf};

/// The project configuration filename (`CFG-002`).
pub const PROJECT_CONFIG_FILENAME: &str = ".contextveil.toml";

/// Why an entered path cannot be turned into an absolute path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathProblem {
    /// A `~/` path was entered but the home directory is unknown.
    NoHome,
    /// The path is empty.
    Empty,
    /// The path stayed relative because its base directory is not absolute.
    NotAbsolute,
}

impl PathProblem {
    pub fn reason(&self) -> &'static str {
        match self {
            PathProblem::NoHome => "uses `~/` but the home directory is unknown",
            PathProblem::Empty => "has an empty path",
            PathProblem::NotAbsolute => "cannot be resolved to an absolute path",
        }
    }
}

/// Expands an entered path and normalizes it lexically.
///
/// `base` is the directory containing the configuration file that named the
/// path. `home` is the current user's home directory, when known.
pub fn expand(entered: &str, base: &Path, home: Option<&Path>) -> Result<PathBuf, PathProblem> {
    if entered.is_empty() {
        return Err(PathProblem::Empty);
    }

    let expanded = if entered == "~" {
        home.ok_or(PathProblem::NoHome)?.to_path_buf()
    } else if let Some(rest) = entered.strip_prefix("~/") {
        home.ok_or(PathProblem::NoHome)?.join(rest)
    } else {
        // `~user` is not expanded; it stays a literal relative path component.
        let candidate = Path::new(entered);
        if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            base.join(candidate)
        }
    };

    if !expanded.is_absolute() {
        return Err(PathProblem::NotAbsolute);
    }
    Ok(normalize(&expanded))
}

/// Removes `.` and `..` components lexically, without touching the filesystem.
///
/// Symlinks are deliberately not resolved: identity must stay stable and must
/// not depend on filesystem state (`CFG-006`).
pub fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    // Keep a leading `..` in a relative path; `/..` is `/`.
                    if !path.is_absolute() {
                        normalized.push("..");
                    }
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    if normalized.as_os_str().is_empty() {
        normalized.push(".");
    }
    normalized
}

/// Selects the project root for setup (`CFG-003`).
///
/// 1. the nearest ancestor containing `.contextveil.toml`;
/// 2. otherwise the enclosing Git worktree root;
/// 3. otherwise the current directory.
pub fn setup_project_root(current_directory: &Path) -> PathBuf {
    if let Some(root) = nearest_ancestor_with(current_directory, PROJECT_CONFIG_FILENAME) {
        return root;
    }
    if let Some(root) = nearest_ancestor_with(current_directory, ".git") {
        return root;
    }
    current_directory.to_path_buf()
}

/// Selects the one project configuration file for a runtime event (`CFG-004`).
///
/// Returns the nearest ancestor project config starting from the
/// adapter-provided project root. Parent and multi-root registries are never
/// merged in V1 (`LIM-019`).
pub fn runtime_project_config(project_root: &Path) -> Option<PathBuf> {
    nearest_ancestor_with(project_root, PROJECT_CONFIG_FILENAME)
        .map(|root| root.join(PROJECT_CONFIG_FILENAME))
}

fn nearest_ancestor_with(start: &Path, entry: &str) -> Option<PathBuf> {
    let mut candidate = Some(start);
    while let Some(directory) = candidate {
        if directory.join(entry).exists() {
            return Some(directory.to_path_buf());
        }
        candidate = directory.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/home/user")
    }

    #[test]
    fn absolute_paths_are_kept() {
        assert_eq!(
            expand("/etc/app/.env", Path::new("/project"), Some(&home())),
            Ok(PathBuf::from("/etc/app/.env"))
        );
    }

    #[test]
    fn relative_paths_resolve_against_the_config_directory() {
        assert_eq!(
            expand(".env.local", Path::new("/project/app"), Some(&home())),
            Ok(PathBuf::from("/project/app/.env.local"))
        );
        assert_eq!(
            expand("../shared/.env", Path::new("/project/app"), Some(&home())),
            Ok(PathBuf::from("/project/shared/.env"))
        );
    }

    #[test]
    fn a_leading_tilde_expands_to_the_home_directory() {
        assert_eq!(
            expand("~/shared/project.env", Path::new("/project"), Some(&home())),
            Ok(PathBuf::from("/home/user/shared/project.env"))
        );
        assert_eq!(
            expand("~", Path::new("/project"), Some(&home())),
            Ok(PathBuf::from("/home/user"))
        );
        assert_eq!(
            expand("~/x", Path::new("/project"), None),
            Err(PathProblem::NoHome)
        );
    }

    #[test]
    fn other_expansions_never_happen() {
        // `CFG-010`: no environment-variable, glob, or shell expansion.
        assert_eq!(
            expand("$HOME/.env", Path::new("/project"), Some(&home())),
            Ok(PathBuf::from("/project/$HOME/.env"))
        );
        assert_eq!(
            expand("*.env", Path::new("/project"), Some(&home())),
            Ok(PathBuf::from("/project/*.env"))
        );
        assert_eq!(
            expand("~other/.env", Path::new("/project"), Some(&home())),
            Ok(PathBuf::from("/project/~other/.env"))
        );
    }

    #[test]
    fn identity_normalization_is_lexical_only() {
        assert_eq!(
            expand("./a/./b/../c/.env", Path::new("/project"), Some(&home())),
            Ok(PathBuf::from("/project/a/c/.env"))
        );
        assert_eq!(normalize(Path::new("/../etc")), PathBuf::from("/etc"));
        assert_eq!(normalize(Path::new("a/../../b")), PathBuf::from("../b"));
        assert_eq!(normalize(Path::new("./")), PathBuf::from("."));
    }

    #[test]
    fn empty_and_unresolvable_paths_are_rejected() {
        assert_eq!(
            expand("", Path::new("/project"), Some(&home())),
            Err(PathProblem::Empty)
        );
        assert_eq!(
            expand("relative.env", Path::new("also/relative"), Some(&home())),
            Err(PathProblem::NotAbsolute)
        );
    }

    #[test]
    fn project_root_selection_prefers_the_nearest_config() {
        let fixture = tempdir("root-selection");
        let nested = fixture.join("repo/packages/app");
        std::fs::create_dir_all(&nested).expect("fixture directories");
        std::fs::create_dir_all(fixture.join("repo/.git")).expect("git directory");
        std::fs::write(fixture.join("repo/.contextveil.toml"), "version = 1\n")
            .expect("project config");
        std::fs::write(
            fixture.join("repo/packages/app/.contextveil.toml"),
            "version = 1\n",
        )
        .expect("nested project config");

        assert_eq!(setup_project_root(&nested), nested);
        assert_eq!(
            runtime_project_config(&nested),
            Some(nested.join(PROJECT_CONFIG_FILENAME))
        );

        let _ = std::fs::remove_dir_all(&fixture);
    }

    #[test]
    fn project_root_falls_back_to_the_git_worktree_then_the_directory() {
        let fixture = tempdir("root-fallback");
        let nested = fixture.join("repo/packages/app");
        std::fs::create_dir_all(&nested).expect("fixture directories");
        std::fs::create_dir_all(fixture.join("repo/.git")).expect("git directory");

        assert_eq!(setup_project_root(&nested), fixture.join("repo"));
        assert_eq!(runtime_project_config(&nested), None);

        let outside = fixture.join("plain");
        std::fs::create_dir_all(&outside).expect("plain directory");
        assert_eq!(setup_project_root(&outside), outside);

        let _ = std::fs::remove_dir_all(&fixture);
    }

    #[test]
    fn a_git_file_marks_a_worktree_root() {
        // Linked worktrees and submodules use a `.git` file, not a directory.
        let fixture = tempdir("git-file");
        let nested = fixture.join("worktree/src");
        std::fs::create_dir_all(&nested).expect("fixture directories");
        std::fs::write(fixture.join("worktree/.git"), "gitdir: /elsewhere\n").expect("git file");
        assert_eq!(setup_project_root(&nested), fixture.join("worktree"));
        let _ = std::fs::remove_dir_all(&fixture);
    }

    fn tempdir(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "contextveil-paths-{name}-{}-{}",
            std::process::id(),
            crate::testing::Canary::generate("PATH").token()
        ));
        std::fs::create_dir_all(&root).expect("fixture root");
        root
    }
}

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use contextveil::paths::setup_project_root;
use contextveil::testing::Canary;

fn fixture() -> PathBuf {
    let path = std::env::temp_dir().join(Canary::generate("TEST_TMP").token());
    std::fs::create_dir_all(&path).expect("fixture directory");
    path
}

fn run(base: &Path, status: &str) -> Output {
    Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/with-test-tmp.sh"
        ))
        .args([
            "bash",
            "-c",
            "test -d \"$TMPDIR\" || exit 99; printf '%s' \"$TMPDIR\"; exit \"$1\"",
            "test-tmp",
            status,
        ])
        .env("TMPDIR", base)
        .output()
        .expect("run temporary directory wrapper")
}

#[test]
fn private_temp_directory_is_removed_on_success_and_failure() {
    let base = fixture();
    for status in [0, 23] {
        let output = run(&base, &status.to_string());
        assert_eq!(output.status.code(), Some(status));
        let selected = PathBuf::from(String::from_utf8(output.stdout).expect("temporary path"));
        assert!(selected.starts_with(base.canonicalize().expect("physical base")));
        assert_ne!(selected, base);
        assert!(
            !selected.exists(),
            "child temporary directory was not removed"
        );
        assert!(base.exists(), "caller-owned directory was removed");
    }
    std::fs::remove_dir_all(base).expect("remove fixture");
}

#[test]
#[cfg(unix)]
fn ancestor_markers_cannot_escape_into_test_project_discovery() {
    let base = fixture();
    let nested = base.join("nested");
    std::fs::create_dir(&nested).expect("nested directory");
    let links = fixture();
    let alias = links.join("alias");
    std::os::unix::fs::symlink(&nested, &alias).expect("temporary directory alias");
    for marker in [".git", ".contextveil.toml"] {
        let path = base.join(marker);
        if marker == ".git" {
            std::fs::create_dir(&path).expect("Git marker");
        } else {
            std::fs::write(&path, "").expect("config marker");
        }
        for candidate in [&nested, &alias] {
            let output = run(candidate, "0");
            if output.status.success() {
                let selected =
                    PathBuf::from(String::from_utf8(output.stdout).expect("temporary path"));
                assert!(!selected.starts_with(&base));
                assert_eq!(setup_project_root(&selected), selected);
                assert!(!selected.exists());
            } else {
                // A host with no clean fallback must refuse to launch the child.
                assert_eq!(output.status.code(), Some(1));
                assert!(output.stdout.is_empty());
                assert!(
                    String::from_utf8_lossy(&output.stderr)
                        .contains("No clean test temporary directory")
                );
            }
        }
        assert!(path.exists(), "caller-owned marker was removed");
        if path.is_dir() {
            std::fs::remove_dir(path).expect("remove Git marker");
        } else {
            std::fs::remove_file(path).expect("remove config marker");
        }
    }
    std::fs::remove_dir_all(base).expect("remove fixture");
    std::fs::remove_dir_all(links).expect("remove aliases");
}

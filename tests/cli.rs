//! Process-level tests for the public command surface.
//!
//! These invoke the built binary instead of calling library functions so the
//! observable CLI contract in `specification.md` section 3 is exercised the way
//! a user and a host see it.

use std::process::{Command, Output};

fn secretsieve(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_secretsieve"))
        .args(args)
        .output()
        .expect("the secretsieve binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

#[test]
fn help_exits_zero_and_documents_the_public_commands() {
    let output = secretsieve(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    for command in ["setup", "status", "doctor"] {
        assert!(text.contains(command), "help omits `{command}`");
    }
    assert!(stderr(&output).is_empty());
}

#[test]
fn help_hides_harness_protocol_entry_points() {
    let text = stdout(&secretsieve(&["--help"]));
    assert!(!text.contains("hook"));
}

#[test]
fn version_reports_the_binary_version() {
    let output = secretsieve(&["--version"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output).trim(),
        format!("secretsieve {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn invalid_usage_exits_two_without_stdout() {
    for args in [
        vec![],
        vec!["init"],
        vec!["--unknown"],
        vec!["hook"],
        vec!["hook", "unsupported"],
    ] {
        let output = secretsieve(&args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "unexpected exit code for {args:?}"
        );
        assert!(
            stdout(&output).is_empty(),
            "usage errors must not use stdout"
        );
        assert!(!stderr(&output).is_empty());
    }
}

#[test]
fn unimplemented_commands_fail_loudly() {
    // Until their tasks land, the public commands must not imply protection.
    for command in ["status", "doctor"] {
        let output = secretsieve(&[command]);
        assert_ne!(output.status.code(), Some(0));
        assert!(stderr(&output).contains("not implemented"));
    }
}

#[test]
fn setup_refuses_to_run_without_a_terminal() {
    // `CLI-002`: a non-interactive invocation fails clearly and changes nothing.
    // The test harness never attaches a TTY, so this is the non-interactive case.
    let output = secretsieve(&["setup"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stdout(&output).is_empty());
    let message = stderr(&output);
    assert!(message.contains("interactive terminal"));
    assert!(message.contains("No file was changed"));
}

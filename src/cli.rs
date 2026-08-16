//! Public command surface.
//!
//! `specification.md` section 3 fixes the public commands: `setup`, `status`,
//! `doctor`, `--help`, and `--version`. Harness protocol entry points exist but
//! stay hidden from ordinary help and are treated as internal interfaces.

use std::ffi::OsString;
use std::io::Write;

/// Process exit status for every SecretSieve command.
///
/// `CLI-004` through `CLI-006` constrain the meaning of each value: zero on
/// success, one for a diagnosed failure, and two for invalid usage or an
/// inspection that could not complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Ok,
    Failure,
    Usage,
}

impl Exit {
    pub fn as_u8(self) -> u8 {
        match self {
            Exit::Ok => 0,
            Exit::Failure => 1,
            Exit::Usage => 2,
        }
    }
}

/// Public commands plus the hidden harness protocol entry points.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    Setup,
    Status,
    Doctor,
    Help,
    Version,
    /// Hidden per-harness hook entry point, for example `hook claude`.
    Hook(Harness),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Harness {
    Claude,
    Codex,
    Copilot,
    OpenCode,
}

impl Harness {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "claude" => Some(Harness::Claude),
            "codex" => Some(Harness::Codex),
            "copilot" => Some(Harness::Copilot),
            "opencode" => Some(Harness::OpenCode),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
            Harness::Copilot => "copilot",
            Harness::OpenCode => "opencode",
        }
    }
}

/// Why an invocation could not be turned into a command.
///
/// Rejected argument text is never echoed: argv is untrusted input and
/// `SEC-006` governs everything rendered to a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ParseError {
    MissingCommand,
    UnknownCommand,
    UnknownOption,
    MissingHarness,
    UnknownHarness,
    UnexpectedArgument,
}

impl ParseError {
    fn message(&self) -> &'static str {
        match self {
            ParseError::MissingCommand => "missing command",
            ParseError::UnknownCommand => "unknown command",
            ParseError::UnknownOption => "unknown option",
            ParseError::MissingHarness => "missing harness name",
            ParseError::UnknownHarness => "unknown harness name",
            ParseError::UnexpectedArgument => "unexpected argument",
        }
    }
}

const HELP: &str = "\
secretsieve - keep enrolled local credentials out of coding-agent model context

USAGE:
    secretsieve <COMMAND>

COMMANDS:
    setup     Enroll local sources and install coding-agent integrations
    status    Report current registry and integration state
    doctor    Run deeper configuration, source, and integration checks

OPTIONS:
    -h, --help       Print this help text
    -V, --version    Print version information

Setup is interactive and requires a terminal. Configuration lives in
${XDG_CONFIG_HOME:-~/.config}/secretsieve/config.toml and in .secretsieve.toml
at the selected project root.
";

/// Runs one CLI invocation.
///
/// `args` excludes the program name. Output is written through the supplied
/// writers so tests can capture both channels without spawning a process.
pub fn run(args: &[OsString], out: &mut dyn Write, err: &mut dyn Write) -> Exit {
    match parse(args) {
        Ok(Command::Help) => {
            let _ = write!(out, "{HELP}");
            Exit::Ok
        }
        Ok(Command::Version) => {
            let _ = writeln!(out, "secretsieve {}", crate::VERSION);
            Exit::Ok
        }
        Ok(Command::Setup) => unimplemented_command("setup", err),
        Ok(Command::Status) => unimplemented_command("status", err),
        Ok(Command::Doctor) => unimplemented_command("doctor", err),
        Ok(Command::Hook(harness)) => {
            unimplemented_command(&format!("hook {}", harness.as_str()), err)
        }
        Err(error) => {
            let _ = writeln!(err, "secretsieve: {}", error.message());
            let _ = writeln!(err, "Run `secretsieve --help` for usage.");
            Exit::Usage
        }
    }
}

/// Placeholder for a command whose task has not landed yet.
///
/// It fails loudly instead of pretending protection exists.
fn unimplemented_command(name: &str, err: &mut dyn Write) -> Exit {
    let _ = writeln!(
        err,
        "secretsieve: `{name}` is not implemented in this build"
    );
    Exit::Usage
}

fn parse(args: &[OsString]) -> Result<Command, ParseError> {
    let mut positional: Vec<&str> = Vec::new();

    for arg in args {
        let Some(text) = arg.to_str() else {
            // A non-UTF-8 argument can never name a command or option.
            return Err(ParseError::UnknownCommand);
        };
        match text {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            _ if text.starts_with('-') => return Err(ParseError::UnknownOption),
            _ => positional.push(text),
        }
    }

    let Some(command) = positional.first() else {
        return Err(ParseError::MissingCommand);
    };

    match *command {
        "setup" | "status" | "doctor" => {
            if positional.len() > 1 {
                return Err(ParseError::UnexpectedArgument);
            }
            Ok(match *command {
                "setup" => Command::Setup,
                "status" => Command::Status,
                _ => Command::Doctor,
            })
        }
        "hook" => match positional.get(1) {
            None => Err(ParseError::MissingHarness),
            Some(harness) => {
                if positional.len() > 2 {
                    return Err(ParseError::UnexpectedArgument);
                }
                Harness::parse(harness)
                    .map(Command::Hook)
                    .ok_or(ParseError::UnknownHarness)
            }
        },
        _ => Err(ParseError::UnknownCommand),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn invoke(values: &[&str]) -> (Exit, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let exit = run(&args(values), &mut out, &mut err);
        (
            exit,
            String::from_utf8(out).expect("stdout is UTF-8"),
            String::from_utf8(err).expect("stderr is UTF-8"),
        )
    }

    #[test]
    fn help_lists_only_public_commands() {
        let (exit, out, err) = invoke(&["--help"]);
        assert_eq!(exit, Exit::Ok);
        assert!(err.is_empty());
        for public in ["setup", "status", "doctor"] {
            assert!(out.contains(public), "help omits `{public}`");
        }
        assert!(
            !out.contains("hook"),
            "help must not advertise hidden harness entry points"
        );
    }

    #[test]
    fn version_reports_the_package_version() {
        let (exit, out, _) = invoke(&["--version"]);
        assert_eq!(exit, Exit::Ok);
        assert_eq!(out.trim(), format!("secretsieve {}", crate::VERSION));
    }

    #[test]
    fn short_flags_match_long_flags() {
        assert_eq!(invoke(&["-h"]).1, invoke(&["--help"]).1);
        assert_eq!(invoke(&["-V"]).1, invoke(&["--version"]).1);
    }

    #[test]
    fn public_commands_parse() {
        assert_eq!(parse(&args(&["setup"])), Ok(Command::Setup));
        assert_eq!(parse(&args(&["status"])), Ok(Command::Status));
        assert_eq!(parse(&args(&["doctor"])), Ok(Command::Doctor));
    }

    #[test]
    fn hidden_hook_entry_points_parse() {
        assert_eq!(
            parse(&args(&["hook", "claude"])),
            Ok(Command::Hook(Harness::Claude))
        );
        assert_eq!(
            parse(&args(&["hook", "opencode"])),
            Ok(Command::Hook(Harness::OpenCode))
        );
        assert_eq!(
            parse(&args(&["hook", "nope"])),
            Err(ParseError::UnknownHarness)
        );
        assert_eq!(parse(&args(&["hook"])), Err(ParseError::MissingHarness));
    }

    #[test]
    fn v1_rejects_removed_and_unknown_commands() {
        // `CLI-001`: setup is the only configuration workflow.
        for rejected in ["init", "install", "uninstall", "enroll"] {
            assert_eq!(parse(&args(&[rejected])), Err(ParseError::UnknownCommand));
        }
        assert_eq!(parse(&[]), Err(ParseError::MissingCommand));
        assert_eq!(parse(&args(&["--nope"])), Err(ParseError::UnknownOption));
        assert_eq!(
            parse(&args(&["status", "extra"])),
            Err(ParseError::UnexpectedArgument)
        );
    }

    #[test]
    fn usage_errors_never_echo_argument_text() {
        let (exit, out, err) = invoke(&["--definitely-not-an-option"]);
        assert_eq!(exit, Exit::Usage);
        assert!(out.is_empty());
        assert!(!err.contains("definitely-not-an-option"));
    }

    #[test]
    fn non_utf8_arguments_are_rejected_as_unknown_commands() {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let invalid = OsString::from_vec(vec![0xff, 0xfe]);
            assert_eq!(parse(&[invalid]), Err(ParseError::UnknownCommand));
        }
    }

    #[test]
    fn exit_codes_match_the_public_contract() {
        assert_eq!(Exit::Ok.as_u8(), 0);
        assert_eq!(Exit::Failure.as_u8(), 1);
        assert_eq!(Exit::Usage.as_u8(), 2);
    }
}

//! Public command surface.
//!
//! `specification.md` section 3 fixes the public commands: `setup`, `status`,
//! `doctor`, `--help`, and `--version`. Harness protocol entry points exist but
//! stay hidden from ordinary help and are treated as internal interfaces.

use std::ffi::OsString;
use std::io::{Read, Write};

use crate::source::Environment;

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
    ///
    /// Copilot needs a second word because its payloads carry no event name, so
    /// the installed command states which covered event it serves.
    Hook(Harness, Option<String>),
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
    MissingHookEvent,
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
            ParseError::MissingHookEvent => "missing hook event name",
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
/// `args` excludes the program name. Hook payloads arrive on `input` and
/// responses leave on `out` (`INT-003`); both are injected so tests can drive
/// the surface without spawning a process.
pub fn run(
    args: &[OsString],
    input: &mut dyn Read,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Exit {
    match parse(args) {
        Ok(Command::Help) => {
            let _ = write!(out, "{HELP}");
            Exit::Ok
        }
        Ok(Command::Version) => {
            let _ = writeln!(out, "secretsieve {}", crate::VERSION);
            Exit::Ok
        }
        Ok(Command::Setup) => run_setup(err),
        Ok(Command::Status) => {
            crate::diagnose::status(out, &Environment::from_process(), &current_directory())
        }
        Ok(Command::Doctor) => run_doctor(out),
        Ok(Command::Hook(Harness::Claude, _)) => run_claude_hook(input, out),
        Ok(Command::Hook(Harness::OpenCode, _)) => run_opencode_hook(input, out),
        Ok(Command::Hook(Harness::Codex, _)) => run_codex_hook(input, out),
        Ok(Command::Hook(Harness::Copilot, event)) => {
            run_copilot_hook(event.as_deref(), input, out, err)
        }
        Err(error) => {
            let _ = writeln!(err, "secretsieve: {}", error.message());
            let _ = writeln!(err, "Run `secretsieve --help` for usage.");
            Exit::Usage
        }
    }
}

/// Reads a hook payload from stdin.
///
/// A read error or non-UTF-8 bytes cannot be a host protocol envelope. Both
/// become an empty payload, which every adapter diagnoses as a malformed event
/// without ever echoing what it received (`RUN-006`).
fn read_payload(input: &mut dyn Read) -> String {
    let mut payload = Vec::new();
    let _ = input.read_to_end(&mut payload);
    String::from_utf8(payload).unwrap_or_default()
}

fn current_directory() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Runs `doctor`, offering the optional live canary on a terminal.
///
/// `DIA-005`: the paid, networked canary is disabled by default and requires
/// confirmation, so it is offered only when a terminal can answer.
fn run_doctor(out: &mut dyn Write) -> Exit {
    use std::io::IsTerminal;

    let live = if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        let mut terminal = crate::setup::ui::Terminal::new(std::io::stdin(), std::io::stdout());
        terminal.line(
            "The optional live Claude canary starts one paid, networked Claude Code request. It \
             is off by default.",
        );
        match terminal.confirm("Run the live Claude canary?", false) {
            Ok(true) => crate::diagnose::LiveCanary::Run,
            _ => crate::diagnose::LiveCanary::Skip,
        }
    } else {
        crate::diagnose::LiveCanary::Skip
    };

    crate::diagnose::doctor(
        out,
        &Environment::from_process(),
        &current_directory(),
        crate::integration::current_executable().as_deref(),
        live,
    )
}

/// Runs the interactive setup workflow.
///
/// `CLI-002`: setup requires an interactive TTY and must fail clearly, without
/// changing any file, when invoked non-interactively. The check belongs here,
/// at the process boundary, so the workflow itself stays drivable by tests.
fn run_setup(err: &mut dyn Write) -> Exit {
    use std::io::IsTerminal;

    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        let _ = writeln!(
            err,
            "secretsieve: `setup` requires an interactive terminal. No file was changed."
        );
        return Exit::Usage;
    }

    let environment = Environment::from_process();
    let current_directory =
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let mut terminal = crate::setup::ui::Terminal::new(std::io::stdin(), std::io::stdout());
    let executable = crate::integration::current_executable();
    let exit = crate::setup::run(
        &mut terminal,
        &environment,
        &current_directory,
        executable.as_deref(),
    );
    terminal.finish();
    exit
}

/// Runs the hidden Claude `PostToolUse` entry point.
///
/// Stdout carries host protocol output only (`architecture.md`), and a
/// diagnosed failure still exits zero so the host can present the warning
/// (`CLI-007`).
fn run_claude_hook(input: &mut dyn Read, out: &mut dyn Write) -> Exit {
    let payload = read_payload(input);
    let response = crate::adapter::claude::handle(&payload, &Environment::from_process());
    if let Some(stdout) = response.stdout {
        let _ = writeln!(out, "{stdout}");
    }
    response.exit
}

/// Runs the hidden Copilot entry point for one covered event.
///
/// Copilot surfaces stderr as a warning when a hook exits 2 and continues the
/// run with the original content, which is how a diagnosed malfunction is
/// reported for this host (`RUN-001`, `CLI-007`).
fn run_copilot_hook(
    event: Option<&str>,
    input: &mut dyn Read,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Exit {
    let Some(event) = event.and_then(crate::adapter::copilot::Event::parse) else {
        let _ = writeln!(err, "secretsieve: unknown hook event");
        return Exit::Usage;
    };
    let payload = read_payload(input);
    let response = crate::adapter::copilot::handle(event, &payload, &Environment::from_process());
    if let Some(stdout) = response.stdout {
        let _ = writeln!(out, "{stdout}");
    }
    if let Some(stderr) = response.stderr {
        let _ = writeln!(err, "{stderr}");
    }
    response.exit
}

/// Runs the hidden Codex `PostToolUse` entry point.
fn run_codex_hook(input: &mut dyn Read, out: &mut dyn Write) -> Exit {
    let payload = read_payload(input);
    let response = crate::adapter::codex::handle(&payload, &Environment::from_process());
    if let Some(stdout) = response.stdout {
        let _ = writeln!(out, "{stdout}");
    }
    response.exit
}

/// Runs the hidden OpenCode transport entry point (`OCO-001`).
///
/// The plugin sends one JSON request on stdin and reads one JSON response from
/// stdout. The response always carries its own status, so the process exits zero
/// whenever it produced one.
fn run_opencode_hook(input: &mut dyn Read, out: &mut dyn Write) -> Exit {
    let payload = read_payload(input);
    let response = crate::adapter::opencode::handle(&payload, &Environment::from_process());
    let _ = writeln!(out, "{}", response.to_json());
    response.exit()
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
        "hook" => {
            let Some(name) = positional.get(1) else {
                return Err(ParseError::MissingHarness);
            };
            let harness = Harness::parse(name).ok_or(ParseError::UnknownHarness)?;
            let event = positional.get(2).map(|event| event.to_string());
            match (harness, &event, positional.len()) {
                // Copilot serves two events, so its command names one.
                (Harness::Copilot, Some(_), 3) => Ok(Command::Hook(harness, event)),
                (Harness::Copilot, _, _) => Err(ParseError::MissingHookEvent),
                (_, None, 2) => Ok(Command::Hook(harness, None)),
                _ => Err(ParseError::UnexpectedArgument),
            }
        }
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
        let exit = run(&args(values), &mut std::io::empty(), &mut out, &mut err);
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
            Ok(Command::Hook(Harness::Claude, None))
        );
        assert_eq!(
            parse(&args(&["hook", "opencode"])),
            Ok(Command::Hook(Harness::OpenCode, None))
        );
        assert_eq!(
            parse(&args(&["hook", "copilot", "prompt"])),
            Ok(Command::Hook(Harness::Copilot, Some("prompt".to_string())))
        );
        assert_eq!(
            parse(&args(&["hook", "copilot"])),
            Err(ParseError::MissingHookEvent)
        );
        assert_eq!(
            parse(&args(&["hook", "claude", "extra"])),
            Err(ParseError::UnexpectedArgument)
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

//! Integration selection and removal: phase three of setup (`SET-001`).
//!
//! `INT-001`: every supported harness is detected, Claude is selected by default
//! when detected, and experimental integrations stay unselected unless
//! SecretSieve already installed them. `INT-002`: an undetected harness may
//! still be installed, with disclosure. `SET-014`: each integration action is a
//! separate transaction that restores its prior managed state on failure.
//!
//! This is plain code over concrete integrations, not a plugin framework
//! (`architecture.md`). Codex, Copilot, and OpenCode join it in `T070`, `T080`,
//! and `T090`.

use std::path::Path;

use crate::cli::Exit;
use crate::integration::claude::{self, Installed, Verification};
use crate::integration::state::State;
use crate::integration::{Detection, state};
use crate::sanitize;
use crate::setup::ui::{Cancelled, Terminal};
use crate::source::Environment;

/// Runs the integration phase.
///
/// Returns `Err` when a requested action failed or the user cancelled, so setup
/// returns nonzero (`CLI-004`).
pub fn phase(
    terminal: &mut Terminal<'_>,
    environment: &Environment,
    home: Option<&Path>,
    global_config_path: &Path,
    executable: Option<&Path>,
) -> Result<(), Exit> {
    terminal.line("Integrations");
    let Some(home) = home else {
        terminal.line("  skipped: the home directory is unknown.");
        terminal.blank();
        return Ok(());
    };

    let state_path = state::path(global_config_path);
    let mut state = state::load(&state_path);
    let inspection = claude::inspect(environment, home, executable, &state);

    let installed = matches!(
        inspection.installed,
        Installed::Current | Installed::Outdated { .. }
    );
    // `INT-001`: Claude is the production integration and is selected by default
    // when detected.
    let mut selected = installed || inspection.detection == Detection::Detected;

    loop {
        terminal.blank();
        terminal.line(&format!(
            "   1 [{}] Claude Code (production) - {}, {}",
            if selected { "x" } else { " " },
            match inspection.detection {
                Detection::Detected => "detected",
                Detection::NotDetected => "not detected",
            },
            describe(&inspection.installed)
        ));
        terminal.line(&format!(
            "        settings: {}",
            sanitize::path(&inspection.settings_path)
        ));
        for conflict in &inspection.conflicts {
            terminal.line(&format!(
                "        other PostToolUse hook: {} ({})",
                conflict.command,
                if conflict.approved {
                    "approved"
                } else {
                    "needs review"
                }
            ));
        }
        terminal.line(
            "  Installation is not proof of protection; run `secretsieve doctor` to check it.",
        );

        let answer = match terminal.ask("Toggle 1, Enter to apply, [s]kip, [q]uit:") {
            Ok(answer) => answer,
            Err(Cancelled) => return cancelled(terminal),
        };
        match answer.trim() {
            "" => break,
            "1" => selected = !selected,
            "s" => {
                terminal.line("  Skipped; integrations are unchanged.");
                terminal.blank();
                return Ok(());
            }
            "q" => return cancelled(terminal),
            other => {
                terminal.line(&format!("  Not a choice: {}", sanitize::text(other)));
            }
        }
    }

    if selected && !installed && inspection.detection == Detection::NotDetected {
        // `INT-002`: disclose that verification is limited.
        terminal.line(
            "  Claude Code was not detected. The hook will be installed, but SecretSieve cannot \
             confirm the host will load it.",
        );
    }

    let outcome = apply(
        terminal,
        home,
        executable,
        &inspection,
        selected,
        installed,
        &mut state,
    );

    // `INT-005`: each competing mutating hook is approved individually.
    if selected {
        for conflict in &inspection.conflicts {
            if conflict.approved {
                continue;
            }
            terminal.line(&format!(
                "  Another PostToolUse hook can also change tool results: {}",
                conflict.command
            ));
            terminal.line(
                "  SecretSieve cannot stop it from seeing the original result or replacing the \
                 sanitized one.",
            );
            match terminal.confirm("  Keep it and continue?", false) {
                Ok(true) => claude::approve_conflict(&mut state, &conflict.command),
                Ok(false) => {
                    terminal.line("  Leaving it unapproved; `secretsieve doctor` will report it.");
                }
                Err(Cancelled) => return cancelled(terminal),
            }
        }
    }

    if let Err(error) = state::save(&state_path, &state) {
        terminal.line(&format!(
            "  warning: the integration record could not be saved because {}.",
            error.reason()
        ));
    }
    terminal.blank();
    outcome
}

fn describe(installed: &Installed) -> &'static str {
    match installed {
        Installed::Absent => "not installed",
        Installed::Current => "installed",
        Installed::Outdated { .. } => "installed, pointing at another binary",
        Installed::Modified { .. } => "installed entry was modified by hand",
        Installed::SettingsUnreadable => "settings file is not valid JSON",
        Installed::SettingsUnexpected => "settings file has an unexpected shape",
    }
}

/// Performs the requested install or removal as one transaction (`SET-014`).
fn apply(
    terminal: &mut Terminal<'_>,
    home: &Path,
    executable: Option<&Path>,
    inspection: &claude::Inspection,
    selected: bool,
    installed: bool,
    state: &mut State,
) -> Result<(), Exit> {
    if let Installed::Modified { command } = &inspection.installed {
        // `INT-004`: a hand-modified entry is preserved, not rewritten.
        terminal.line(&format!(
            "  warning: the existing SecretSieve hook was modified ({command}); it was left \
             unchanged."
        ));
        return Ok(());
    }

    match (selected, installed) {
        (false, false) => Ok(()),
        (false, true) => match claude::remove(home, state) {
            Ok(true) => {
                terminal.line("  Removed the Claude hook.");
                Ok(())
            }
            Ok(false) => {
                terminal
                    .line("  warning: a modified SecretSieve hook was preserved and not removed.");
                Ok(())
            }
            Err(error) => {
                terminal.line(&format!("  Removal failed: {}.", error.reason()));
                Err(Exit::Failure)
            }
        },
        (true, _) => {
            let Some(executable) = executable else {
                terminal.line(&format!(
                    "  Installation failed: {}.",
                    claude::InstallError::ExecutablePath.reason()
                ));
                return Err(Exit::Failure);
            };
            let previous = state.claude.clone();
            if let Err(error) = claude::install(home, executable, state) {
                terminal.line(&format!("  Installation failed: {}.", error.reason()));
                return Err(Exit::Failure);
            }
            terminal.line("  Installed the Claude hook with a 5-second timeout.");

            // Offline synthetic verification of the real protocol path.
            match claude::verify_offline(executable) {
                Verification::Passed => {
                    terminal.line("  Offline protocol check passed.");
                    Ok(())
                }
                Verification::Failed(reason) => {
                    terminal.line(&format!("  Offline protocol check failed: {reason}."));
                    // Restore the exact prior managed state.
                    let restored = if previous.is_some() {
                        claude::install(home, executable, state).is_ok()
                    } else {
                        claude::remove(home, state).is_ok()
                    };
                    if !restored {
                        terminal.line(
                            "  warning: the previous integration state could not be restored.",
                        );
                    }
                    state.claude = previous;
                    Err(Exit::Failure)
                }
            }
        }
    }
}

fn cancelled(terminal: &mut Terminal<'_>) -> Result<(), Exit> {
    terminal.line("Setup cancelled. Nothing further was changed.");
    Err(Exit::Failure)
}

//! Integration selection and removal: phase three of setup (`SET-001`).
//!
//! `INT-001`: every supported harness is detected, Claude is selected by default
//! when detected, and experimental integrations stay unselected unless
//! SecretSieve already installed them. `INT-002`: an undetected harness may still
//! be installed, with disclosure. `SUP-003`: experimental integrations are
//! labeled and require an affirmative choice. `SET-014`: each integration action
//! is a separate transaction that restores its prior managed state on failure.
//!
//! Dispatch is a plain match over a small enum, not a plugin framework
//! (`architecture.md`).

use std::path::Path;

use crate::cli::Exit;
use crate::integration::hooks_json::Installed;
use crate::integration::state::{Managed, State};
use crate::integration::{
    self, Detection, HARNESSES, Harness, Inspection, Tier, Verification, state,
};
use crate::sanitize;
use crate::setup::ui::{Cancelled, Terminal};
use crate::source::Environment;

/// One selectable integration row.
struct Row {
    inspection: Inspection,
    selected: bool,
    /// Whether a managed artifact existed when the phase started.
    installed: bool,
}

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
    let mut rows: Vec<Row> = HARNESSES
        .iter()
        .map(|harness| {
            let inspection = integration::inspect(*harness, environment, home, executable, &state);
            let installed = inspection.is_installed();
            Row {
                // `INT-001`: production is selected by default when detected;
                // experimental integrations only when already installed.
                selected: installed
                    || (harness.tier() == Tier::Production
                        && inspection.detection == Detection::Detected),
                installed,
                inspection,
            }
        })
        .collect();

    loop {
        render(terminal, &rows);
        let answer = match terminal.ask("Toggle numbers, Enter to apply, [s]kip, [q]uit:") {
            Ok(answer) => answer,
            Err(Cancelled) => return cancelled(terminal),
        };
        match answer.trim() {
            "" => break,
            "s" => {
                terminal.line("  Skipped; integrations are unchanged.");
                terminal.blank();
                return Ok(());
            }
            "q" => return cancelled(terminal),
            selection => toggle(terminal, &mut rows, selection),
        }
    }

    let mut outcome = Ok(());
    for row in &rows {
        // `SET-014`: each action is its own transaction, and an earlier
        // completed action stays applied when a later one fails.
        let result = apply(terminal, home, executable, row, &mut state);
        if result.is_err() && outcome.is_ok() {
            outcome = result;
        }
        if row.selected
            && let Err(Cancelled) = approve_conflicts(terminal, row, &mut state)
        {
            return cancelled(terminal);
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

fn render(terminal: &mut Terminal<'_>, rows: &[Row]) {
    terminal.blank();
    for (index, row) in rows.iter().enumerate() {
        let harness = row.inspection.harness;
        terminal.line(&format!(
            "  {:>2} [{}] {} ({}) - {}, {}",
            index + 1,
            if row.selected { "x" } else { " " },
            harness.label(),
            harness.tier_label(),
            match row.inspection.detection {
                Detection::Detected => "detected",
                Detection::NotDetected => "not detected",
            },
            describe(&row.inspection.installed)
        ));
        terminal.line(&format!(
            "        file: {}",
            sanitize::path(&row.inspection.artifact_path)
        ));
        for conflict in &row.inspection.conflicts {
            terminal.line(&format!(
                "        other hook on the same event: {} ({})",
                conflict.command,
                if conflict.approved {
                    "approved"
                } else {
                    "needs review"
                }
            ));
        }
    }
    terminal
        .line("  Installation is not proof of protection; run `secretsieve doctor` to check it.");
}

fn describe(installed: &Installed) -> &'static str {
    match installed {
        Installed::Absent => "not installed",
        Installed::Current => "installed",
        Installed::Outdated { .. } => "installed, pointing at another binary",
        Installed::Modified { .. } => "installed entry was modified by hand",
        Installed::Unreadable => "host file is not valid JSON",
        Installed::Unexpected => "host file has an unexpected shape",
    }
}

fn toggle(terminal: &mut Terminal<'_>, rows: &mut [Row], selection: &str) {
    let mut unknown = Vec::new();
    for token in selection.split_whitespace() {
        match token.parse::<usize>() {
            Ok(number) if number >= 1 && number <= rows.len() => {
                let row = &mut rows[number - 1];
                row.selected = !row.selected;
                if row.selected
                    && !row.installed
                    && row.inspection.harness.tier() == Tier::Experimental
                {
                    // `SUP-003`: experimental installation is an affirmative
                    // choice, and the label follows it everywhere.
                    terminal.line(&format!(
                        "  {} is EXPERIMENTAL: functional and fixture-tested, but outside the \
                         production support promise.",
                        row.inspection.harness.label()
                    ));
                }
            }
            _ => unknown.push(sanitize::text(token)),
        }
    }
    if !unknown.is_empty() {
        terminal.line(&format!("  Not a choice: {}", unknown.join(", ")));
    }
}

/// Performs one requested install or removal (`SET-014`).
fn apply(
    terminal: &mut Terminal<'_>,
    home: &Path,
    executable: Option<&Path>,
    row: &Row,
    state: &mut State,
) -> Result<(), Exit> {
    let harness = row.inspection.harness;
    let label = harness.label();

    if let Installed::Modified { command } = &row.inspection.installed {
        // `INT-004`: a hand-modified entry is preserved, not rewritten.
        terminal.line(&format!(
            "  warning: the existing {label} hook was modified ({command}); it was left unchanged."
        ));
        return Ok(());
    }

    match (row.selected, row.installed) {
        (false, false) => Ok(()),
        (false, true) => match integration::remove(harness, home, state) {
            Ok(true) => {
                terminal.line(&format!("  Removed the {label} integration."));
                Ok(())
            }
            Ok(false) => {
                terminal.line(&format!(
                    "  warning: a modified {label} artifact was preserved and not removed."
                ));
                Ok(())
            }
            Err(error) => {
                terminal.line(&format!("  {label} removal failed: {}.", error.reason()));
                Err(Exit::Failure)
            }
        },
        (true, _) => {
            if !row.installed && row.inspection.detection == Detection::NotDetected {
                // `INT-002`: disclose that verification is limited.
                terminal.line(&format!(
                    "  {label} was not detected. The integration will be installed, but \
                     SecretSieve cannot confirm the host will load it."
                ));
            }
            let Some(executable) = executable else {
                terminal.line(&format!(
                    "  {label} installation failed: {}.",
                    integration::InstallError::ExecutablePath.reason()
                ));
                return Err(Exit::Failure);
            };

            let previous = state.get(harness).cloned();
            if let Err(error) = integration::install(harness, home, executable, state) {
                terminal.line(&format!(
                    "  {label} installation failed: {}.",
                    error.reason()
                ));
                return Err(Exit::Failure);
            }
            terminal.line(&format!(
                "  Installed the {label} integration with a 5-second timeout."
            ));
            if let Some(note) = harness.post_install_note() {
                terminal.line(&format!("  {note}"));
            }

            match integration::verify_offline(harness, executable) {
                Verification::Passed => {
                    terminal.line("  Offline protocol check passed.");
                    Ok(())
                }
                Verification::Failed(reason) => {
                    terminal.line(&format!("  Offline protocol check failed: {reason}."));
                    restore(terminal, harness, home, executable, previous, state);
                    Err(Exit::Failure)
                }
            }
        }
    }
}

/// Restores an integration's exact prior managed state (`SET-014`).
fn restore(
    terminal: &mut Terminal<'_>,
    harness: Harness,
    home: &Path,
    executable: &Path,
    previous: Option<Managed>,
    state: &mut State,
) {
    let restored = match &previous {
        Some(_) => integration::install(harness, home, executable, state).is_ok(),
        None => integration::remove(harness, home, state).is_ok(),
    };
    if !restored {
        terminal.line("  warning: the previous integration state could not be restored.");
    }
    state.set(harness, previous);
}

/// Individual approval for every competing mutating hook (`INT-005`).
fn approve_conflicts(
    terminal: &mut Terminal<'_>,
    row: &Row,
    state: &mut State,
) -> Result<(), Cancelled> {
    for conflict in &row.inspection.conflicts {
        if conflict.approved {
            continue;
        }
        terminal.line(&format!(
            "  Another {} hook can also change the same content: {}",
            row.inspection.harness.label(),
            conflict.command
        ));
        terminal.line(
            "  SecretSieve cannot stop it from seeing the original content or replacing the \
             sanitized one.",
        );
        if terminal.confirm("  Keep it and continue?", false)? {
            integration::approve_conflict(row.inspection.harness, state, &conflict.command);
        } else {
            terminal.line("  Leaving it unapproved; `secretsieve doctor` will report it.");
        }
    }
    Ok(())
}

fn cancelled(terminal: &mut Terminal<'_>) -> Result<(), Exit> {
    terminal.line("Setup cancelled. Nothing further was changed.");
    Err(Exit::Failure)
}

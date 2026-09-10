//! Integration selection and removal: phase three of setup (`SET-001`).
//!
//! `INT-001`: every supported harness is detected, setup presents detected or
//! already-managed harnesses only, Claude is selected by default when detected,
//! and experimental integrations stay unselected unless ContextVeil already
//! installed them. `SUP-003`: experimental integrations are labeled and require
//! affirmative installation. `SET-014`: each integration action is its own
//! transaction that restores its prior managed state on failure.
//!
//! Dispatch is a plain match over a small enum, not a plugin framework
//! (`docs/architecture.md`).

use std::fs::Permissions;
use std::io;
use std::path::{Path, PathBuf};

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
pub(super) struct Row {
    pub(super) inspection: Inspection,
    pub(super) selected: bool,
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
    let Some(home) = home else {
        terminal.line("Integrations");
        terminal.line("  skipped: the home directory is unknown.");
        terminal.blank();
        return Ok(());
    };

    let state_path = state::path(global_config_path);
    let mut state = state::load(&state_path);
    let initial_state = state.clone();
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
                inspection,
            }
        })
        .filter(|row| {
            row.inspection.detection == Detection::Detected || row.inspection.is_installed()
        })
        .collect();

    loop {
        terminal.line("Integrations");
        for line in render_rows(&rows).lines() {
            terminal.line(line);
        }
        for line in render_actions(rows.len()).lines() {
            terminal.line(line);
        }
        let answer = match terminal.ask(">") {
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

    for row in &rows {
        // `SET-014`: each action is its own transaction, and an earlier
        // completed action stays applied when a later one fails.
        if let Err(exit) = apply(terminal, home, executable, row, &mut state) {
            if state != initial_state
                && let Err(error) = state::save(&state_path, &state)
            {
                terminal.line(&format!(
                    "  warning: the integration record could not be saved because {}.",
                    error.reason()
                ));
            }
            terminal.blank();
            return Err(exit);
        }
        if row.selected
            && let Err(Cancelled) = approve_conflicts(terminal, row, &mut state)
        {
            return cancelled(terminal);
        }
    }

    if state != initial_state
        && let Err(error) = state::save(&state_path, &state)
    {
        terminal.line(&format!(
            "  warning: the integration record could not be saved because {}.",
            error.reason()
        ));
    }
    terminal.blank();
    Ok(())
}

/// Pure integration presentation used by setup and the broad rendering snapshot.
pub(super) fn render_rows(rows: &[Row]) -> String {
    let mut lines = vec![String::new()];
    if rows.is_empty() {
        lines.push("  No supported coding-agent installation was detected.".to_string());
        lines.push(
            "  Install or initialize your coding agent in this environment, then rerun setup."
                .to_string(),
        );
        return lines.join("\n");
    }
    for (index, row) in rows.iter().enumerate() {
        let harness = row.inspection.harness;
        lines.push(format!(
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
        lines.push(format!(
            "        file: {}",
            sanitize::path(&row.inspection.artifact_path)
        ));
        for conflict in &row.inspection.conflicts {
            lines.push(format!(
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
    lines.push(
        "  Installation is not proof of protection; run `contextveil doctor` to check it."
            .to_string(),
    );
    lines.join("\n")
}

pub(super) fn render_actions(row_count: usize) -> String {
    let mut lines = vec!["Choose an action:".to_string()];
    if row_count > 0 {
        lines.push("  [1 3]   toggle row(s) by space separated row numbers".to_string());
    }
    lines.extend([
        "  [Enter] apply".to_string(),
        "  [s]     skip".to_string(),
        "  [q]     quit".to_string(),
    ]);
    lines.join("\n")
}

fn describe(installed: &Installed) -> &'static str {
    match installed {
        Installed::Absent => "ContextVeil integration not installed",
        Installed::Current => "ContextVeil integration installed",
        Installed::Outdated { .. } => {
            "ContextVeil integration installed, pointing at another binary"
        }
        Installed::Modified { .. } => "ContextVeil integration entry was modified by hand",
        Installed::Unreadable => "ContextVeil integration host file is not valid JSON",
        Installed::Unexpected => "ContextVeil integration host file has an unexpected shape",
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
                    && !row.inspection.is_installed()
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
    let installed = row.inspection.is_installed();

    if let Installed::Modified { command } = &row.inspection.installed {
        // `INT-004`: a hand-modified entry is preserved, not rewritten.
        terminal.line(&format!(
            "  warning: the existing {label} hook was modified ({command}); it was left unchanged."
        ));
        return Ok(());
    }

    if matches!((row.selected, installed), (false, false)) {
        return Ok(());
    }

    let artifact = match ArtifactSnapshot::capture(&row.inspection.artifact_path) {
        Ok(snapshot) => snapshot,
        Err(_) => {
            terminal.line(&format!(
                "  {label} action failed: the existing integration artifact could not be read."
            ));
            return Err(Exit::Failure);
        }
    };
    let previous = state.get(harness).cloned();

    let result = match (row.selected, installed) {
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
            let Some(executable) = executable else {
                terminal.line(&format!(
                    "  {label} installation failed: {}.",
                    integration::InstallError::ExecutablePath.reason()
                ));
                return rollback(terminal, harness, artifact, previous, state);
            };

            if let Err(error) = integration::install(harness, home, executable, state) {
                terminal.line(&format!(
                    "  {label} installation failed: {}.",
                    error.reason()
                ));
                return rollback(terminal, harness, artifact, previous, state);
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
                    Err(Exit::Failure)
                }
            }
        }
    };

    if result.is_err() {
        rollback(terminal, harness, artifact, previous, state)
    } else {
        result
    }
}

/// The exact managed artifact before one integration action.
struct ArtifactSnapshot {
    path: PathBuf,
    prior: Option<(Vec<u8>, Permissions)>,
}

impl ArtifactSnapshot {
    fn capture(path: &Path) -> io::Result<Self> {
        match std::fs::read(path) {
            Ok(contents) => Ok(Self {
                path: path.to_path_buf(),
                prior: Some((contents, std::fs::metadata(path)?.permissions())),
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self {
                path: path.to_path_buf(),
                prior: None,
            }),
            Err(error) => Err(error),
        }
    }

    fn restore(self) -> bool {
        match self.prior {
            Some((contents, permissions)) => {
                crate::setup::write::restore_bytes(&self.path, &contents, &permissions).is_ok()
            }
            None => match std::fs::remove_file(&self.path) {
                Ok(()) => true,
                Err(error) if error.kind() == io::ErrorKind::NotFound => true,
                Err(_) => false,
            },
        }
    }
}

/// Restores an integration's exact prior artifact and ownership state (`SET-014`).
fn rollback(
    terminal: &mut Terminal<'_>,
    harness: Harness,
    artifact: ArtifactSnapshot,
    previous: Option<Managed>,
    state: &mut State,
) -> Result<(), Exit> {
    if !artifact.restore() {
        terminal.line("  warning: the previous integration state could not be restored.");
    }
    state.set(harness, previous);
    Err(Exit::Failure)
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
            "  ContextVeil cannot stop it from seeing the original content or replacing the \
             sanitized one.",
        );
        if terminal.confirm("  Keep it and continue?", false)? {
            integration::approve_conflict(row.inspection.harness, state, &conflict.command);
        } else {
            terminal.line("  Leaving it unapproved; `contextveil doctor` will report it.");
        }
    }
    Ok(())
}

fn cancelled(terminal: &mut Terminal<'_>) -> Result<(), Exit> {
    terminal.line("Setup cancelled. Nothing further was changed.");
    Err(Exit::Failure)
}

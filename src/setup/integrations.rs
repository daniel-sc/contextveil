//! Integration selection and removal: phase three of setup (`SET-001`).
//!
//! This is the extension point the Claude integration plugs into (`T050`). It
//! is deliberately a plain function over concrete integrations rather than a
//! plugin framework (`architecture.md`).

use crate::cli::Exit;
use crate::setup::ui::Terminal;

/// Runs the integration phase.
///
/// No integration installer exists in this build yet, so the phase reports that
/// honestly instead of implying protection is installed (`INT-006`).
pub fn phase(terminal: &mut Terminal<'_>) -> Result<(), Exit> {
    terminal.line("Integrations");
    terminal.line("  no coding-agent integration is available in this build yet");
    terminal.blank();
    Ok(())
}

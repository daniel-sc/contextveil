//! Harness protocol adapters.
//!
//! An adapter parses its host protocol, selects host-defined model-visible
//! fields, invokes the shared core, and maps the result back. It must not
//! implement matching, source resolution, or placeholder rules
//! (`architecture.md`).

pub mod claude;
pub mod codex;
pub mod copilot;
pub mod opencode;

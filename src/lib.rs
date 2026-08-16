//! SecretSieve replaces currently resolved values from user-enrolled local
//! sources before they reach a coding agent's model context.
//!
//! The library holds every security-relevant behavior: configuration loading,
//! source resolution, registry composition, and exact-value redaction. Harness
//! adapters translate host protocols only; see `architecture.md`.

pub mod cli;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

/// Version of the running binary, used by `--version` and diagnostics.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

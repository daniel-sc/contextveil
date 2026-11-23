# Changelog

Release notes are concise, user-facing changes. See
[`docs/release-process.md`](docs/release-process.md) for the release-note policy.

## [Unreleased]

No unreleased changes.

## [1.2.0] - 2026-11-23

### Added

- Add experimental OpenCode V2 session-request redaction while retaining V1
  integration support. V2 covers outgoing text without changing stored history;
  production qualification remains blocked by an upstream compaction defect.
- Add optional Luna qualification and a focused upstream compaction regression
  and patch.
- Verified ordinary HTTP session requests on OpenCode 2.3.2 and 2.4.0.
  If OpenCode shrinks a compaction request, it can send previously redacted
  checkpoint text to the provider ([upstream issue](https://github.com/anomalyco/opencode/issues/54916)).

## [1.1.0] - 2026-09-16

### Added

- Add project INI discovery, exact section/key enrollment, and opt-in protection
  of one key across all current and future sections.

### Fixed

- Fixed crashes when reading certain UTF-16 properties files that could interrupt
  setup, diagnostics, or runtime redaction. (#36)

## [1.0.0] - 2026-09-10

### Added

- Published the first stable V1 release with deterministic local redaction,
  guided source enrollment, and checksummed installers for Linux and macOS on
  x86_64 and arm64.
- Added bounded Known Source Rules for environment and dotenv values, JSON5,
  Java properties, npmrc credentials, and supported coding-agent credential
  documents.
- Added Claude Code production integration plus opt-in experimental adapters for
  OpenAI Codex CLI, GitHub Copilot CLI, and OpenCode.

### Changed

- Simplified first-run setup and status output, added explicit post-setup
  guidance, and limited new integration choices to detected harnesses.
- Setup now omits common literals from automatic suggestions, reports collisions,
  and presents masked previews while preserving manual enrollment.
- Runtime source resolution follows current environment and file-backed values
  without storing resolved credentials in configuration.

## [1.0.0-alpha.5] - 2026-09-08

### Changed

- Excluded common literals from automatic setup suggestions while preserving
  manual enrollment. (#27)
- Bounded collision analysis and limited binary scanning to textual regions.
- Improved setup guidance, masked previews, installer instructions, and harness
  integration documentation. (#24, #25, #29)

### Fixed

- Avoided repeated collision scans for ordinary setup selection changes. (#28)

## [1.0.0-alpha.4] - 2026-08-31

### Added

- Added exact npmrc secret sources and bounded npmrc Known Source discovery. (#13)

### Fixed

- Improved malformed npmrc candidate handling and preserved valid discovery entries.

## [1.0.0-alpha.3] - 2026-08-29

### Added

- Added Java-style properties files as exact-key secret sources. (#15)
- Broadened bounded Known Source discovery and made its probes declarative.
  (#1, #7, #11)

### Fixed

- Tightened properties discovery coverage and setup enrollment behavior. (#15)
- Improved deterministic candidate grouping, presentation, rollback, and adapter
  assurance. (#6, #8, #16)

## [1.0.0-alpha.2] - 2026-08-20

### Changed

- Rebranded the project from SecretSieve to ContextVeil.
- Simplified onboarding documentation and synchronized the release lockfile.

## [1.0.0-alpha.1] - 2026-08-17

### Added

- Published the initial prerelease with the local redaction core, interactive
  setup, status and doctor commands, supported harness integrations, and
  checksummed installers for the supported platforms.

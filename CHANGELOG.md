# Changelog

Release notes are concise, user-facing changes. See
[`docs/release-process.md`](docs/release-process.md) for the release-note policy.

## [Unreleased]

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

# Repository Instructions

## Read First

Read these files before changing behavior or architecture:

1. [vision.md](vision.md) for product intent and non-goals.
2. [CONTEXT.md](CONTEXT.md) for canonical domain language.
3. [specification.md](specification.md) for normative behavior.
4. [architecture.md](architecture.md) for mandatory technical boundaries.
5. [limitations.md](limitations.md) for accepted gaps and active deviations.
6. [tasks.md](tasks.md) for dependency order and current implementation work.

`specification.md` is authoritative for observable behavior.
`architecture.md` is authoritative for technical boundaries. Do not silently
resolve a conflict between them; surface and document it first.

## Tooling

Use `mise` as the only documented entry point for tool installation and routine
project tasks. Bootstrap work must create and maintain tasks equivalent to:

```bash
mise install
mise run format
mise run lint
mise run test
mise run check
mise run build
mise run fuzz-smoke
mise run release-check
```

Routine CI must call the applicable format/lint/test/check/build mise tasks;
fuzz and release jobs call `fuzz-smoke` and `release-check`. CI must not duplicate
their hidden command lines. Do not require globally installed Rust tools when
they can be pinned by mise or the Rust toolchain configuration.

## Engineering Approach

- Prefer lean, direct Rust code over frameworks or speculative abstractions.
- Build a vertical production path before extracting general adapter machinery.
- Extract shared abstractions after repeated concrete use, not in anticipation.
- Keep source resolution and redaction semantics independent of harness code.
- Keep adapters thin: protocol parsing, core invocation, result translation, and
  host presentation only.
- Do not add a daemon, cache, provider proxy, secret store, compatibility layer,
  or plugin system without a changed requirement.
- Preserve tactical discretion where the specification does not constrain
  observable behavior.
- Treat warnings as errors in maintained Rust code. Avoid `unsafe`; any necessary
  use requires a documented invariant and focused tests.
- Keep dependencies few, maintained, locked, and justified by concrete value.

## Security Rules

- Never commit or use real credentials in tests, fixtures, snapshots, examples,
  issue text, or logs.
- Use conspicuous generated canaries and assert they are absent from stdout,
  stderr, diagnostics, and adapter responses after intervention.
- Never include resolved values, source contents, matching lines, or deterministic
  value hashes in diagnostics.
- Sanitize untrusted labels and paths before terminal output.
- Pass hook payloads through stdin and structured stdout, never command-line
  arguments or shell interpolation.
- Preserve unrelated user and harness configuration exactly where practical.
- A runtime semantic change requires matcher and adapter conformance tests.

## Tests

- Run `mise run check` before considering a code change complete.
- Add unit or property tests for matcher and registry invariants.
- Add filesystem tests for discovery, config parsing, permissions, and setup
  idempotency.
- Add protocol fixtures for every changed adapter path.
- Add regression tests for every fixed leak, panic, malformed-input failure, or
  accidental plaintext diagnostic.
- Keep live paid/networked tests optional. Only the documented Claude release
  qualification may require a manual live run.

## Documentation Discipline

- Observable behavior changes update `specification.md` in the same change.
- Technical-boundary changes update `architecture.md`.
- Product direction changes update `vision.md`.
- New accepted gaps or deliberate implementation deviations update
  `limitations.md` with impact, workaround, and verification.
- Task state and sequencing changes update `tasks.md`.
- Canonical domain terminology changes update `CONTEXT.md`.
- Use code comments only for local, non-obvious constraints. Do not duplicate
  broad limitations in comments; link to a limitation ID where useful.
- A limitation entry records reality but does not silently authorize violating a
  normative requirement. Material deviations require explicit review.
- Before considering a change complete, reconcile all affected documentation
  with the implementation. Code and documentation must not knowingly disagree;
  update the contract or record an approved limitation in the same change.

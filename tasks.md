# SecretSieve Implementation Tasks

This is a non-normative, dependency-ordered implementation map. Requirements
live in [specification.md](specification.md); tasks must not silently redefine
them. Update task state and completion evidence as implementation proceeds.

## Working Rules

- Complete a narrow production path before generalizing adapter abstractions.
- Every task includes its tests and documentation changes.
- A task is complete only when its listed mise checks pass.
- Record known behavioral deviations in [limitations.md](limitations.md) in the
  same change. A limitation does not by itself approve a contract violation.
- Use generated canaries, never real credentials.
- Keep at most one implementation task actively in progress per working branch.

Status markers:

```text
[ ] not started
[>] in progress
[x] complete
[!] blocked
```

## Dependency Map

```text
T000 -> T001 -> T010
T010 -> T020 + T030
T020 + T030 -> T040
T020 + T030 + T040 -> T050
T040 + T050 -> T060
T020 + T030 + T040 + T060 -> T070 + T080 + T090
T050 + T060 + T070 + T080 + T090 -> T100
T100 -> T110 -> T120
```

Tasks at the same indentation may proceed in parallel after their dependencies
are complete.

## Foundation

### [x] T000: Establish Documentation Baseline

**Depends on:** none

**Evidence:**

- product intent, domain language, normative behavior, architecture boundaries,
  accepted limitations, repository instructions, and this task graph exist;
- cross-document review resolved security-relevant ambiguity before bootstrap;
- implementation may begin at `T001` without another product decision.

**Closes:** planning prerequisite only; no runtime requirement is implemented.

### [ ] T001: Bootstrap Rust And Mise

**Depends on:** `T000`

**Deliver:**

- initialize one lean Rust package with library and binary targets;
- commit a pinned Rust toolchain and lockfile;
- add mise configuration for tool installation and every canonical task;
- implement `--help` and `--version` with the public command skeleton;
- add rustfmt, Clippy with warnings denied, unit-test, build, fuzz-smoke, and
  release-check task entry points;
- add CI that calls mise tasks rather than duplicating commands;
- add MIT and Apache-2.0 license files plus security reporting policy;
- establish generated-canary test helpers and secret-safe assertion helpers.

**Acceptance:**

- `mise install` works on a clean supported development environment;
- `mise run format`, `lint`, `test`, `check`, and `build` pass;
- placeholder `fuzz-smoke` and `release-check` tasks fail clearly only if their
  later implementation is intentionally incomplete;
- no globally installed Rust utility is required outside mise/toolchain setup.

**Contributes to:** `SUP-001`, `CLI-001` through `CLI-003`, `REL-006`, `TST-005`
through `TST-007`.

### [ ] T010: Build The Claude Walking Slice

**Depends on:** `T001`

**Deliver:**

- parse one valid V1 global environment reference;
- resolve its current value without logging it;
- perform direct exact replacement in one string;
- parse a representative Claude `PostToolUse` fixture;
- return shape-preserving `updatedToolOutput` and safe intervention metadata;
- expose the hidden Claude hook command over stdin/stdout;
- add end-to-end process tests for clean, matched, unresolved, and invalid input.

This slice should use direct concrete code. Do not build a generic adapter or
resolver framework yet.

**Acceptance:**

- a generated canary in successful Claude tool output is absent from stdout and
  stderr and replaced before the fixture's model-visible boundary;
- no-match output follows Claude's expected protocol without user chatter;
- malformed hook input produces a secret-safe diagnosed response;
- `mise run check` passes.

**Contributes to:** `SEC-001`, `SEC-003` through `SEC-005`, `REG-001`, `RED-001`,
`RED-004`, `RED-008`, `RUN-006`, `CLA-002`, `TST-004`, `TST-005`.

## Core Completion

### [ ] T020: Implement Config, Registry, And Sources

**Depends on:** `T010`

**Deliver:**

- strict versioned global and project TOML parsing;
- environment, individual dotenv key, and wildcard dotenv source variants;
- path preservation and tilde/relative resolution;
- project-root selection for setup and runtime;
- dotenv parsing without expansion and last-key-wins diagnostics;
- unresolved versus malfunction classification;
- all-or-nothing effective registry construction;
- duplicate source validation and project-first equal-value canonicalization;
- secret-safe config/source diagnostics.

**Acceptance:**

- table-driven fixtures cover runtime/config portions of `CFG-001` through
  `CFG-013`, every `SRC-*`, and every `REG-*` rule;
- malformed or unreadable enrolled sources never create a partial matcher;
- absent/unset/empty sources remain silent and do not fail hook processing;
- config and diagnostics contain no generated canary value;
- `mise run check` passes.

**Contributes to:** `CFG-001` through `CFG-013`, `SRC-001` through `SRC-010`,
`REG-001` through `REG-004`, `TST-002`, relevant `TST-003` cases.

### [ ] T030: Complete Matcher And Structured Redaction

**Depends on:** `T010`; may proceed in parallel with `T020`

**Deliver:**

- leftmost-longest matching across an ordered resolved registry;
- exact case-sensitive UTF-8 and multiline behavior;
- recursive transformation of JSON string values only;
- canonical intervention counts;
- label sanitization and safe named/generic/empty placeholder fallback;
- no recursive placeholder matching or cross-leaf joining;
- property tests comparing the production matcher with a simple reference model;
- benchmarks for the specified representative workload.

**Acceptance:**

- every vector in `TST-001` has a focused regression test;
- generated placeholders and feedback do not reproduce tested active values;
- object keys and non-string values remain byte/semantically unchanged;
- the benchmark records the 100 ms target without making CI timing-dependent;
- `mise run check` passes.

**Contributes to:** `RED-001` through `RED-010`, `RUN-005`, `TST-001`.

### [ ] T040: Implement Unified Interactive Setup

**Depends on:** `T020`, `T030`

**Deliver:**

- one TTY-only `secretsieve setup` workflow;
- global and project phases with current selections preselected;
- an integration phase extension point initially exercised by the later Claude
  task, without introducing a general plugin framework;
- recursive project dotenv discovery including ignored/untracked files;
- maintained directory exclusions and no automatic file/directory-symlink or
  special-file traversal;
- bounded global probe locations and manual source entry;
- exact V1 name-gating vocabulary and suggestion scoring with explanatory
  signals;
- exact preview masking rules;
- current-project collision analysis and filename-only reporting;
- wildcard confirmation and unresolved-manual-source confirmation;
- safe atomic config writes and resumable multi-phase failure behavior;
- always-created valid project config;
- invalid-existing-config preservation.

**Acceptance:**

- isolated filesystem tests cover the enrollment/config portions of every
  `SET-*` rule and setup-related `CFG-*` rules on Linux and macOS;
- rerunning setup with no changes is idempotent;
- cancellation and non-TTY invocation produce no unintended writes;
- an invalid existing file is byte-for-byte unchanged;
- setup never prints a full canary or deterministic value fingerprint;
- non-UTF-8 paths and control-bearing previews are escaped and never persisted
  lossily;
- `mise run check` passes.

**Contributes to:** `CLI-001`, `CLI-002`, `CLI-004`, `CFG-003`, `CFG-014`,
`CFG-015`, `SET-001` through `SET-014`, `TST-003`.

## Production Integration

### [ ] T050: Finish Claude Production Integration

**Depends on:** `T020`, `T030`, `T040`

**Deliver:**

- complete Claude protocol fixture coverage for successful built-in and MCP tool
  response shapes that accept `updatedToolOutput`;
- user-settings installer with absolute exec-form command and 5-second timeout;
- preservation of unrelated settings and ownership-aware update/removal;
- potential `PostToolUse` conflict discovery and per-conflict approval;
- safe named/count `systemMessage` on intervention;
- diagnosed-malfunction warning and documented fail-open behavior;
- offline synthetic integration verification;
- explicit negative fixtures for failed tool results and schema rejection.

**Acceptance:**

- clean installation, repeat installation, upgrade, deselection removal, modified
  entry, malformed settings, disabled hook, and approved conflict are tested;
- every supported successful response retains its exact key/type shape;
- canaries are absent from model-visible replacement, stdout diagnostics, and
  stderr after intervention;
- `mise run check` passes.

**Contributes to:** `SEC-001`, `RUN-001`, `RUN-002`, `RUN-004`, `RUN-006`,
`INT-001` through `INT-006`, `CLA-001` through `CLA-005`, `TST-004`, `TST-005`.

### [ ] T060: Implement Status And Doctor

**Depends on:** `T040`, `T050`

**Deliver:**

- independent registry and integration facets;
- active/unresolved counts and `INACTIVE` handling;
- config/source/permission/duplicate diagnostics;
- current collision recheck with warning-only findings;
- installer ownership, disabled-hook, conflict, executable, timeout, and
  synthetic protocol checks;
- stable public exit-code behavior without a JSON output contract;
- optional confirmed Claude live canary using temporary source configuration;
- secret-safe terminal sanitization for every untrusted path and label.

**Acceptance:**

- exit-code matrix tests cover healthy, partially unresolved, fully inactive,
  malformed, approved-conflict, and inspection-failure cases;
- status performs no adapter protocol tests;
- doctor performs no network call unless the Claude canary is selected;
- diagnostic snapshots contain no canary values or source contents;
- `mise run check` passes.

**Contributes to:** `CLI-005` through `CLI-007`, `DIA-001` through `DIA-005`,
`DIA-007`, `DIA-008`, `SEC-004` through `SEC-006`.

## Experimental Integrations

### [ ] T070: Implement Codex Experimental Adapter

**Depends on:** `T020`, `T030`, `T040`, `T060`

**Deliver:**

- capture official release protocol fixtures for the tested Codex release;
- native `PostToolUse` parser and sanitized textual replacement path;
- wildcard user hook installation in `~/.codex/hooks.json`;
- documented host trust/review workflow;
- ownership-aware update/removal and conflict approval;
- offline synthetic doctor checks and clear semantic-degradation messaging;
- fail-open diagnosed-malfunction behavior.

**Acceptance:**

- fixtures cover clean output, string and structured intervention, non-zero Bash
  output where supported, unsupported paths, malformed protocol, and failure;
- original canaries are absent from blocked model-facing results;
- setup requires explicit experimental opt-in;
- `mise run check` passes.

**Contributes to:** `SEC-001`, `SUP-003`, `RUN-001`, `RUN-002`, `RUN-004`,
`RUN-006`, `INT-001` through `INT-006`, `COD-001` through `COD-004`, `DIA-006`,
`TST-004`, `TST-005`.

### [ ] T080: Implement Copilot Experimental Adapter

**Depends on:** `T020`, `T030`, `T040`, `T060`

**Deliver:**

- capture official release protocol fixtures for both covered events;
- shape-preserving transformed-prompt and successful-tool-text mutation;
- dedicated managed file under `~/.copilot/hooks/`;
- one safe intervention progress summary;
- ownership-aware update/removal and conflict warning;
- offline synthetic doctor checks and fail-open malfunction behavior.

**Acceptance:**

- fixtures cover prompt, successful result, failed result negative case, clean,
  malformed, timeout, and conflicting mutator scenarios;
- setup requires explicit experimental opt-in;
- canaries are absent from both covered model-facing outputs after intervention;
- `mise run check` passes.

**Contributes to:** `SEC-001`, `SUP-003`, `RUN-001`, `RUN-002`, `RUN-004`,
`RUN-006`, `INT-001` through `INT-006`, `COP-001` through `COP-004`, `DIA-006`,
`TST-004`, `TST-005`.

### [ ] T090: Implement OpenCode Experimental Adapter

**Depends on:** `T020`, `T030`, `T040`, `T060`

**Deliver:**

- a minimal TypeScript plugin using only `chat.message` and
  `tool.execute.after`;
- `Bun.spawn` one-shot JSON transport with absolute argv and 5-second timeout;
- managed global plugin-file installation, update, and removal;
- text-part/tool-output mutation using Rust responses;
- named/count TUI notification whose failure does not undo mutation;
- throw/abort behavior for subprocess, protocol, and source malfunction;
- offline synthetic doctor checks.

**Acceptance:**

- plugin code contains no resolver or matcher semantics;
- fixtures cover user text, successful standard tool output, clean events,
  subprocess failure, notification failure, and explicitly unsupported paths;
- setup requires explicit experimental opt-in;
- canaries are absent from covered mutated content and plugin diagnostics;
- `mise run check` passes.

**Contributes to:** `SEC-001`, `SUP-003`, `RUN-003`, `RUN-004`, `RUN-006`,
`INT-001` through `INT-006`, `OCO-001` through `OCO-004`, `DIA-006`, `TST-004`,
`TST-005`.

## Hardening And Release

### [ ] T100: Complete Security, Fuzz, And Performance Hardening

**Depends on:** `T050`, `T060`, `T070`, `T080`, `T090`

**Deliver:**

- matcher and parser fuzz targets with regression seed promotion;
- bounded `mise run fuzz-smoke` across JSON, TOML, dotenv, and matcher surfaces;
- full leak-regression suite across stdout, stderr, snapshots, and diagnostics;
- large-input and timeout behavior tests without adding product size caps;
- dependency and license review;
- terminal-control/path sanitization tests;
- benchmark report for the representative 100 ms workload;
- review every limitation against implemented behavior.

**Acceptance:**

- fuzz smoke runs without panic, unbounded recursion, or canary disclosure;
- maintained Rust builds warning-free and contains no undocumented `unsafe`;
- every open implementation deviation has an approved `DEV-*` entry;
- `mise run check` and `mise run fuzz-smoke` pass.

**Contributes to:** `SEC-003` through `SEC-006`, `RUN-004`, `RUN-005`, `TST-001`
through `TST-007`.

### [ ] T110: Build Release And Installer Pipeline

**Depends on:** `T100`

**Deliver:**

- reproducible release builds for Linux/macOS x86_64 and arm64;
- GitHub Release asset naming and SHA-256 checksum generation;
- install/upgrade script with platform detection, checksum verification, atomic
  replacement, `--install-dir`, `--version`, `--allow-major-upgrade`, and
  same-major default upgrade;
- clean install, repeat install, older-V1 upgrade, corrupt-download, and explicit
  major-upgrade tests;
- release notes support matrix and limitation links;
- `mise run release-check` using produced artifacts.

**Acceptance:**

- the installer never runs setup or changes harness/config files;
- a clean target can install and invoke help/version from release artifacts;
- an existing V1 config remains runtime-readable after upgrade;
- checksums and failure handling are verified in CI;
- `mise run release-check` passes.

**Contributes to:** `REL-001` through `REL-007`, `TST-007`.

### [ ] T120: Qualify And Publish V1

**Depends on:** `T110`

**Deliver:**

- complete all automated gates on the four release targets;
- manually run the paid/networked Claude intervention and resume qualification;
- record tested host versions as release evidence without adding runtime version
  gates;
- reconcile support matrix, status wording, and all limitation entries;
- complete a requirement-to-test traceability audit covering every `SEC-*`,
  `SUP-*`, `CLI-*`, `CFG-*`, `SRC-*`, `SET-*`, `REG-*`, `RED-*`, `RUN-*`,
  `INT-*`, adapter, diagnostic, release, and test requirement;
- verify installation, setup, intervention, status, doctor, removal, and upgrade
  from final artifacts;
- publish checksummed artifacts and security reporting instructions.

**Acceptance:**

- the Claude resume canary remains absent after resume, or resume coverage is
  removed and documented before release;
- Claude is the only integration presented as production;
- all experimental adapters are functional, tested, and opt-in;
- no unresolved unapproved V1 contract deviation remains;
- every normative requirement has passing completion evidence or an explicitly
  approved release-blocking disposition;
- final `mise run release-check` passes against the release candidate.

**Closes:** all V1 requirements, including `SEC-002`, `SUP-002` through `SUP-005`,
`REL-008`, and `TST-008`.

## Deferred Until A Changed Requirement

- additional source formats, literal storage, keychains, or secret managers;
- daemons, caches, value history, or cross-event session state;
- public adapter/resolver plugin APIs;
- provider proxies or final-dispatch interception wrappers;
- environment stripping, command policy, or placeholder rehydration;
- noninteractive setup or stable machine-readable status output;
- Windows support and package-manager distribution;
- OpenCode V2 or experimental full-context transforms;
- production promotion of experimental adapters.

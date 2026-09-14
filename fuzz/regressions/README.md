# Fuzz regression corpus

Every file here is replayed by `mise run fuzz-regressions` on every run,
regardless of the time budget. The directory name selects the target. The
separate `mise run fuzz-smoke` task generates bounded mutations and promotes
new failures into this corpus. Smoke mode uses seed `0` by default; set
`CONTEXTVEIL_FUZZ_SEED` to vary or reproduce a run, for example
`CONTEXTVEIL_FUZZ_SEED=34829922969 mise run fuzz-smoke`. Scheduled GitHub runs
use their run ID as the seed, while manually dispatched runs accept an
optional seed input.

Two kinds of file belong here:

- **Promoted failures.** When a target fails, the harness writes the exact input
  to `fuzz/regressions/<target>/<fingerprint>` and prints the path. Commit that
  file so the case can never regress silently.
- **Hand-picked edge cases.** Inputs that exercise a rule the grammar or matcher
  is easy to get wrong.

Files must never contain a real credential. Inputs are generated from the seeds
in `src/bin/fuzz_smoke.rs`. Adapter targets inject their temporary generated
canary at execution time, so it is not stored in the saved corpus input.

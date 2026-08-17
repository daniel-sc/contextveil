# Fuzz regression corpus

Every file here is replayed by `mise run fuzz-smoke` before any generated input,
on every run, regardless of the time budget. The directory name selects the
target.

Two kinds of file belong here:

- **Promoted failures.** When a target fails, the harness writes the exact input
  to `fuzz/regressions/<target>/<fingerprint>` and prints the path. Commit that
  file so the case can never regress silently.
- **Hand-picked edge cases.** Inputs that exercise a rule the grammar or matcher
  is easy to get wrong.

Files must never contain a real credential. The harness refuses to help there:
inputs are generated from the seeds in `src/bin/fuzz_smoke.rs`, and enrolled
values live in the environment rather than in any input.

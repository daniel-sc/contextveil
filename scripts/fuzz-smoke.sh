#!/usr/bin/env bash
# Bounded fuzz smoke run over untrusted input surfaces (`TST-006`).
#
# It replays the committed regression corpus and then mutates seeds
# deterministically, so a failure is always reproducible. Raise
# SECRETSIEVE_FUZZ_ITERATIONS or SECRETSIEVE_FUZZ_SECONDS for a longer run.
set -euo pipefail

cargo run --locked --release --features testing --bin fuzz_smoke

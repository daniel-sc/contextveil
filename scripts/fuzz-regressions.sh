#!/usr/bin/env bash
# Replay every committed fuzz regression without running mutation (`TST-006`).
set -euo pipefail

cargo run --locked --release --features testing --bin fuzz_smoke -- regressions

#!/usr/bin/env bash
# Bounded fuzz smoke run over untrusted input surfaces (`TST-006`).
#
# Implemented by task T100. The placeholder fails clearly so a green pipeline
# never implies coverage that does not exist yet.
set -euo pipefail

echo "mise run fuzz-smoke: not implemented yet (task T100)." >&2
echo "It must cover the matcher and untrusted JSON, TOML, and dotenv input." >&2
exit 1

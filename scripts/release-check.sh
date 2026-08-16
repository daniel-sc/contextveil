#!/usr/bin/env bash
# Release artifact, checksum, and installer verification (`REL-001` - `REL-004`,
# `TST-007`).
#
# Implemented by task T110. The placeholder fails clearly so a green pipeline
# never implies release readiness that does not exist yet.
set -euo pipefail

echo "mise run release-check: not implemented yet (task T110)." >&2
echo "It must verify release artifacts, checksums, clean install, and upgrade." >&2
exit 1

#!/usr/bin/env bash
set -euo pipefail

# Root discovery walks above fixtures, so even a unique temp directory needs clean ancestors.
for base in "${TMPDIR:-/tmp}" /tmp /var/tmp; do
  base="$(cd -- "$base" 2>/dev/null && pwd -P)" || continue
  ancestor="$base"
  while [[ ! -e "$ancestor/.git" && ! -e "$ancestor/.contextveil.toml" ]]; do
    [[ "$ancestor" == / ]] && break
    ancestor="$(dirname -- "$ancestor")"
  done
  [[ ! -e "$ancestor/.git" && ! -e "$ancestor/.contextveil.toml" ]] || continue

  work="$(mktemp -d "$base/contextveil-tests.XXXXXX" 2>/dev/null)" || continue
  trap 'rm -rf -- "$work"' EXIT
  export TMPDIR="$work"
  "$@"
  exit 0
done

echo 'No clean test temporary directory: set TMPDIR to a writable directory with no .git or .contextveil.toml in it or its ancestors.' >&2
exit 1

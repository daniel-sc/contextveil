#!/usr/bin/env bash
# Render the GitHub Release body from the matching CHANGELOG.md section.
set -euo pipefail

if [ "$#" -ne 1 ]; then
  printf 'usage: release-notes.sh TAG\n' >&2
  exit 2
fi

tag="$1"
case "${tag}" in
  v*) version="${tag#v}" ;;
  *) printf 'release-notes.sh: expected a v-prefixed tag\n' >&2; exit 2 ;;
esac

awk -v version="${version}" '
  BEGIN { heading = "## [" version "]" }
  /^## \[/ {
    if (index($0, heading) == 1 &&
        (length($0) == length(heading) || substr($0, length(heading) + 1, 1) == " ")) {
      in_release = 1
      print
      next
    }
    if (in_release) exit
  }
  in_release { print }
  END {
    if (!in_release) exit 1
  }
' CHANGELOG.md

#!/usr/bin/env bash
# Builds and packages one release artifact (`REL-001`).
#
# Usage: package.sh [TARGET] [OUTPUT_DIR]
#
# TARGET defaults to the host target. Archives are deterministic: the toolchain
# and dependency versions are pinned, symbols are stripped, and tar metadata is
# fixed, so rebuilding the same commit on the same platform produces the same
# bytes.
set -euo pipefail

target="${1:-$(rustc -vV | awk '/^host:/{print $2}')}"
output="${2:-dist}"
version="$(awk -F'"' '/^version = /{print $2; exit}' Cargo.toml)"

case "${target}" in
  x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu | x86_64-apple-darwin | aarch64-apple-darwin) ;;
  *)
    printf 'package.sh: %s is not a V1 release target\n' "${target}" >&2
    exit 1
    ;;
esac

if [ "${target}" != "$(rustc -vV | awk '/^host:/{print $2}')" ]; then
  rustup target add "${target}"
fi

cargo build --release --locked --target "${target}"

mkdir -p "${output}"
archive="${output}/secretsieve-${version}-${target}.tar.gz"
staging="$(mktemp -d)"
trap 'rm -rf "${staging}"' EXIT

cp "target/${target}/release/secretsieve" "${staging}/secretsieve"
chmod 755 "${staging}/secretsieve"
cp LICENSE-MIT LICENSE-APACHE README.md "${staging}/"

# Deterministic archive: fixed order, owner, and timestamp.
tar --create \
  --gzip \
  --sort=name \
  --mtime='UTC 2020-01-01' \
  --owner=0 \
  --group=0 \
  --numeric-owner \
  --directory "${staging}" \
  --file "${archive}" \
  secretsieve LICENSE-MIT LICENSE-APACHE README.md

checksums="${output}/secretsieve-${version}-SHA256SUMS"
(
  cd "${output}"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$(basename "${archive}")" >>"$(basename "${checksums}")"
  else
    shasum -a 256 "$(basename "${archive}")" >>"$(basename "${checksums}")"
  fi
)

printf 'package.sh: wrote %s\n' "${archive}"
printf 'package.sh: checksums in %s\n' "${checksums}"

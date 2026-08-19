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
archive="${output}/contextveil-${version}-${target}.tar.gz"
staging="$(mktemp -d)"
trap 'rm -rf "${staging}"' EXIT

cp "target/${target}/release/contextveil" "${staging}/contextveil"
chmod 755 "${staging}/contextveil"
cp LICENSE-MIT LICENSE-APACHE README.md "${staging}/"

# Deterministic archive: fixed member order, owner, and timestamp. Members are
# always listed explicitly, so their order in the archive is the order below.
#
# GNU tar and bsdtar spell the metadata options differently and macOS ships
# bsdtar, which rejects `--sort` and `--owner` outright. Determinism is required
# per platform, not across them, so each implementation gets its own invocation.
members="contextveil LICENSE-MIT LICENSE-APACHE README.md"
if tar --version 2>/dev/null | grep -q 'GNU tar'; then
  # shellcheck disable=SC2086
  tar --create \
    --gzip \
    --sort=name \
    --mtime='UTC 2020-01-01' \
    --owner=0 \
    --group=0 \
    --numeric-owner \
    --directory "${staging}" \
    --file "${archive}" \
    ${members}
else
  # bsdtar has no `--mtime`, so the staged files carry the fixed timestamp, and
  # `gzip -n` keeps the original name and timestamp out of the compressed stream.
  # shellcheck disable=SC2086
  (cd "${staging}" && TZ=UTC touch -t 202001010000 ${members})
  # shellcheck disable=SC2086
  tar --create \
    --numeric-owner \
    --uid 0 \
    --gid 0 \
    --uname '' \
    --gname '' \
    --directory "${staging}" \
    --file "${archive%.gz}" \
    ${members}
  gzip -n -f "${archive%.gz}"
fi

checksums="${output}/contextveil-${version}-SHA256SUMS"
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

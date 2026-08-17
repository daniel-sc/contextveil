#!/usr/bin/env bash
# SecretSieve installer and upgrader (`REL-002` through `REL-004`).
#
# It installs or upgrades the standalone binary only. It never runs setup, never
# edits SecretSieve configuration, and never touches coding-agent configuration
# (`REL-003`). Every download is verified against the release checksum file
# before it replaces anything.
#
# Usage:
#   install.sh [--install-dir DIR] [--version VERSION] [--allow-major-upgrade]
#
# With no installed binary it selects the latest stable release. With an
# installed binary it selects the latest release in that major version.
# `--version` selects an exact release, but crossing the installed major still
# requires `--allow-major-upgrade`. With an installed binary, a standalone
# `--allow-major-upgrade` selects the latest stable release across majors.
#
# The two environment variables below exist so `mise run release-check` can
# exercise this script against locally produced artifacts. They are not part of
# the supported interface.
#   SECRETSIEVE_RELEASE_INDEX  URL of a JSON document listing release tags
#   SECRETSIEVE_RELEASE_BASE   URL prefix that holds the release assets
set -euo pipefail

REPOSITORY="secretsieve/secretsieve"
DEFAULT_INSTALL_DIR="${HOME}/.local/bin"
RELEASE_INDEX="${SECRETSIEVE_RELEASE_INDEX:-https://api.github.com/repos/${REPOSITORY}/releases?per_page=100}"
RELEASE_BASE="${SECRETSIEVE_RELEASE_BASE:-https://github.com/${REPOSITORY}/releases/download}"

install_dir="${DEFAULT_INSTALL_DIR}"
requested_version=""
allow_major_upgrade="no"

fail() {
  printf 'install.sh: %s\n' "$1" >&2
  exit 1
}

usage() {
  cat >&2 <<'USAGE'
Usage: install.sh [--install-dir DIR] [--version VERSION] [--allow-major-upgrade]

  --install-dir DIR       Install into DIR instead of ~/.local/bin
  --version VERSION       Install exactly this release, for example 1.2.3
  --allow-major-upgrade   Permit crossing the installed major version
  -h, --help              Show this help

The installer only installs or upgrades the binary. It never runs setup and never
changes SecretSieve or coding-agent configuration.
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --install-dir)
      [ "$#" -ge 2 ] || fail "--install-dir needs a directory"
      install_dir="$2"
      shift 2
      ;;
    --install-dir=*)
      install_dir="${1#*=}"
      shift
      ;;
    --version)
      [ "$#" -ge 2 ] || fail "--version needs a version"
      requested_version="$2"
      shift 2
      ;;
    --version=*)
      requested_version="${1#*=}"
      shift
      ;;
    --allow-major-upgrade)
      allow_major_upgrade="yes"
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      usage
      fail "unknown option"
      ;;
  esac
done

# --- platform detection (`SUP-001`, `REL-002`) --------------------------------

detect_target() {
  local system machine
  system="$(uname -s)"
  machine="$(uname -m)"
  case "${system}" in
    Linux) system="unknown-linux-gnu" ;;
    Darwin) system="apple-darwin" ;;
    *) fail "unsupported operating system: ${system}. V1 supports Linux and macOS." ;;
  esac
  case "${machine}" in
    x86_64 | amd64) machine="x86_64" ;;
    arm64 | aarch64) machine="aarch64" ;;
    *) fail "unsupported architecture: ${machine}. V1 supports x86_64 and arm64." ;;
  esac
  printf '%s-%s' "${machine}" "${system}"
}

require_tool() {
  command -v "$1" >/dev/null 2>&1 || fail "this installer needs \`$1\`"
}

checksum_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    fail "this installer needs \`sha256sum\` or \`shasum\`"
  fi
}

fetch() {
  # `--fail` turns an HTTP error into a nonzero exit instead of a saved error page.
  curl --fail --silent --show-error --location "$1" --output "$2"
}

# --- version selection (`REL-002`, `REL-004`) --------------------------------

installed_version() {
  local binary="$1"
  [ -x "${binary}" ] || return 1
  "${binary}" --version 2>/dev/null | awk '/^secretsieve /{print $2; exit}'
}

major_of() {
  printf '%s' "${1%%.*}"
}

# Every published release tag, newest first.
list_versions() {
  local index
  index="$(mktemp)"
  fetch "${RELEASE_INDEX}" "${index}" || fail "the release list could not be downloaded"
  # Tags look like "tag_name": "v1.2.3". Prereleases are excluded by requiring
  # exactly three numeric components.
  grep -o '"tag_name"[[:space:]]*:[[:space:]]*"v\{0,1\}[0-9]\{1,\}\.[0-9]\{1,\}\.[0-9]\{1,\}"' "${index}" |
    sed 's/.*"v\{0,1\}\([0-9.]*\)"/\1/' |
    sort -t. -k1,1nr -k2,2nr -k3,3nr |
    awk '!seen[$0]++'
  rm -f "${index}"
}

select_version() {
  local current="$1" versions latest
  versions="$(list_versions)"
  [ -n "${versions}" ] || fail "no stable release was found"

  if [ -n "${requested_version}" ]; then
    printf '%s\n' "${versions}" | grep -qx "${requested_version}" ||
      fail "release ${requested_version} was not found"
    printf '%s' "${requested_version}"
    return
  fi

  if [ -z "${current}" ] || [ "${allow_major_upgrade}" = "yes" ]; then
    # No installed binary, or an explicit opt-in: the latest stable release.
    printf '%s' "$(printf '%s\n' "${versions}" | head -n1)"
    return
  fi

  # `REL-004`: an ordinary rerun stays inside the installed major version.
  latest="$(printf '%s\n' "${versions}" | awk -F. -v major="$(major_of "${current}")" '$1 == major {print; exit}')"
  [ -n "${latest}" ] || fail "no release was found in major version $(major_of "${current}")"
  printf '%s' "${latest}"
}

# --- main --------------------------------------------------------------------

require_tool curl
require_tool tar
require_tool uname

target="$(detect_target)"
binary_path="${install_dir}/secretsieve"
current_version="$(installed_version "${binary_path}" || true)"
version="$(select_version "${current_version}")"

if [ -n "${current_version}" ]; then
  printf 'install.sh: found secretsieve %s in %s\n' "${current_version}" "${install_dir}"
  if [ "$(major_of "${version}")" != "$(major_of "${current_version}")" ] &&
    [ "${allow_major_upgrade}" != "yes" ]; then
    fail "installing ${version} would cross major version $(major_of "${current_version}"); rerun with --allow-major-upgrade"
  fi
  if [ "${version}" = "${current_version}" ]; then
    printf 'install.sh: secretsieve %s is already installed\n' "${version}"
    exit 0
  fi
fi

archive="secretsieve-${version}-${target}.tar.gz"
checksums="secretsieve-${version}-SHA256SUMS"
work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT

printf 'install.sh: downloading secretsieve %s for %s\n' "${version}" "${target}"
fetch "${RELEASE_BASE}/v${version}/${archive}" "${work}/${archive}" ||
  fail "the release archive could not be downloaded"
fetch "${RELEASE_BASE}/v${version}/${checksums}" "${work}/${checksums}" ||
  fail "the checksum file could not be downloaded"

expected="$(awk -v name="${archive}" '$2 == name || $2 == "*" name {print $1; exit}' "${work}/${checksums}")"
[ -n "${expected}" ] || fail "the checksum file does not list ${archive}"
actual="$(checksum_of "${work}/${archive}")"
if [ "${expected}" != "${actual}" ]; then
  fail "checksum mismatch for ${archive}; the download was not installed"
fi
printf 'install.sh: checksum verified\n'

tar -xzf "${work}/${archive}" -C "${work}" ||
  fail "the release archive could not be extracted"
[ -f "${work}/secretsieve" ] || fail "the release archive does not contain \`secretsieve\`"
chmod 755 "${work}/secretsieve"

mkdir -p "${install_dir}" || fail "${install_dir} could not be created"
# Replace atomically: stage next to the target so the rename cannot cross a
# filesystem boundary, then rename over it.
staged="${install_dir}/.secretsieve.install.$$"
cp "${work}/secretsieve" "${staged}" || fail "${install_dir} is not writable"
chmod 755 "${staged}"
mv -f "${staged}" "${binary_path}" || {
  rm -f "${staged}"
  fail "the binary could not be moved into place"
}

printf 'install.sh: installed secretsieve %s to %s\n' "${version}" "${binary_path}"
case ":${PATH}:" in
  *":${install_dir}:"*) ;;
  *) printf 'install.sh: add %s to your PATH to run `secretsieve`\n' "${install_dir}" ;;
esac
printf 'install.sh: nothing else was changed. Run `secretsieve setup` when you are ready.\n'

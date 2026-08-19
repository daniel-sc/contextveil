#!/usr/bin/env bash
# ContextVeil installer and upgrader (`REL-002` through `REL-004`).
#
# It installs or upgrades the standalone binary only. It never runs setup, never
# edits ContextVeil configuration, and never touches coding-agent configuration
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
# A prerelease such as 1.0.0-alpha.1 is never selected automatically. Naming it
# with `--version` installs it.
#
# The two environment variables below exist so `mise run release-check` can
# exercise this script against locally produced artifacts. They are not part of
# the supported interface.
#   CONTEXTVEIL_RELEASE_INDEX  URL of a JSON document listing release tags
#   CONTEXTVEIL_RELEASE_BASE   URL prefix that holds the release assets
set -euo pipefail

REPOSITORY="daniel-sc/contextveil"
DEFAULT_INSTALL_DIR="${HOME}/.local/bin"
RELEASE_INDEX="${CONTEXTVEIL_RELEASE_INDEX:-https://api.github.com/repos/${REPOSITORY}/releases?per_page=100}"
RELEASE_BASE="${CONTEXTVEIL_RELEASE_BASE:-https://github.com/${REPOSITORY}/releases/download}"

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
  --version VERSION       Install exactly this release, for example 1.2.3, or a
                          prerelease such as 1.0.0-alpha.1
  --allow-major-upgrade   Permit crossing the installed major version
  -h, --help              Show this help

The installer only installs or upgrades the binary. It never runs setup and never
changes ContextVeil or coding-agent configuration.
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
  "${binary}" --version 2>/dev/null | awk '/^contextveil /{print $2; exit}'
}

major_of() {
  printf '%s' "${1%%.*}"
}

# Every published release tag, newest first.
#
# `stable` keeps only the three-numeric-component tags that automatic selection
# may choose. `any` also lists prereleases such as 1.0.0-alpha.1, which only an
# exact `--version` request can install (`REL-002`).
list_versions() {
  local scope="$1" index pattern
  index="$(mktemp)"
  fetch "${RELEASE_INDEX}" "${index}" || fail "the release list could not be downloaded"
  # Tags look like "tag_name": "v1.2.3" or "tag_name": "v1.0.0-alpha.1".
  pattern='"tag_name"[[:space:]]*:[[:space:]]*"v\{0,1\}[0-9]\{1,\}\.[0-9]\{1,\}\.[0-9]\{1,\}"'
  if [ "${scope}" = "any" ]; then
    pattern='"tag_name"[[:space:]]*:[[:space:]]*"v\{0,1\}[0-9]\{1,\}\.[0-9]\{1,\}\.[0-9]\{1,\}[-+.0-9A-Za-z]*"'
  fi
  grep -o "${pattern}" "${index}" |
    sed 's/.*"v\{0,1\}\([^"]*\)"/\1/' |
    sort -t. -k1,1nr -k2,2nr -k3,3nr |
    awk '!seen[$0]++'
  rm -f "${index}"
}

select_version() {
  local current="$1" versions latest

  if [ -n "${requested_version}" ]; then
    # `REL-002`: an exact request may name a prerelease.
    versions="$(list_versions any)"
    printf '%s\n' "${versions}" | grep -Fqx "${requested_version}" ||
      fail "release ${requested_version} was not found"
    printf '%s' "${requested_version}"
    return
  fi

  # A prerelease is never chosen for the user.
  versions="$(list_versions stable)"
  [ -n "${versions}" ] || fail "no stable release was found"

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
binary_path="${install_dir}/contextveil"
current_version="$(installed_version "${binary_path}" || true)"
version="$(select_version "${current_version}")"

if [ -n "${current_version}" ]; then
  printf 'install.sh: found contextveil %s in %s\n' "${current_version}" "${install_dir}"
  if [ "$(major_of "${version}")" != "$(major_of "${current_version}")" ] &&
    [ "${allow_major_upgrade}" != "yes" ]; then
    fail "installing ${version} would cross major version $(major_of "${current_version}"); rerun with --allow-major-upgrade"
  fi
  if [ "${version}" = "${current_version}" ]; then
    printf 'install.sh: contextveil %s is already installed\n' "${version}"
    exit 0
  fi
fi

archive="contextveil-${version}-${target}.tar.gz"
checksums="contextveil-${version}-SHA256SUMS"
work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT

printf 'install.sh: downloading contextveil %s for %s\n' "${version}" "${target}"
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

# Extract only the one member that is installed. A hostile archive therefore
# cannot write anywhere else, and a member that is a symlink rather than a
# regular file is refused instead of followed.
tar -xzf "${work}/${archive}" -C "${work}" contextveil ||
  fail "the release archive does not contain an extractable \`contextveil\`"
if [ -L "${work}/contextveil" ] || [ ! -f "${work}/contextveil" ]; then
  fail "the release archive does not contain \`contextveil\` as a regular file"
fi
chmod 755 "${work}/contextveil"

mkdir -p "${install_dir}" || fail "${install_dir} could not be created"
# Replace atomically: stage next to the target so the rename cannot cross a
# filesystem boundary, then rename over it.
staged="${install_dir}/.contextveil.install.$$"
cp "${work}/contextveil" "${staged}" || fail "${install_dir} is not writable"
chmod 755 "${staged}"
mv -f "${staged}" "${binary_path}" || {
  rm -f "${staged}"
  fail "the binary could not be moved into place"
}

printf 'install.sh: installed contextveil %s to %s\n' "${version}" "${binary_path}"
case ":${PATH}:" in
  *":${install_dir}:"*) ;;
  *) printf 'install.sh: add %s to your PATH to run `contextveil`\n' "${install_dir}" ;;
esac
printf 'install.sh: nothing else was changed. Run `contextveil setup` when you are ready.\n'

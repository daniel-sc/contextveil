#!/usr/bin/env bash
# Release artifact, checksum, and installer verification (`REL-001` - `REL-004`,
# `TST-007`).
#
# It packages a real release artifact for the host target, then drives the real
# `install.sh` against those artifacts over `file://` URLs. No network access and
# no published release are required, so this runs in CI and locally.
set -euo pipefail

version="$(awk -F'"' '/^version = /{print $2; exit}' Cargo.toml)"
target="$(rustc -vV | awk '/^host:/{print $2}')"
root="$(pwd)"
work="$(mktemp -d)"
trap 'rm -rf "${work}"' EXIT

pass=0

# Writes a stand-in binary that reports `stub_version`, for upgrade checks.
write_stub() {
  cat >"$1" <<STUB
#!/bin/sh
# Stands in for another release during the release check.
[ "\$1" = "--version" ] && echo "secretsieve ${stub_version}"
exit 0
STUB
  chmod 755 "$1"
}

check() {
  printf '  [ok] %s\n' "$1"
  pass=$((pass + 1))
}
fail() {
  printf '  [FAIL] %s\n' "$1" >&2
  exit 1
}

printf 'SecretSieve release check\n'
printf '  version     %s\n' "${version}"
printf '  target      %s\n' "${target}"

# --- artifacts ---------------------------------------------------------------

releases="${work}/releases/v${version}"
mkdir -p "${releases}"
bash scripts/package.sh "${target}" "${releases}" >/dev/null
archive="${releases}/secretsieve-${version}-${target}.tar.gz"
checksums="${releases}/secretsieve-${version}-SHA256SUMS"

[ -f "${archive}" ] || fail "the release archive was not produced"
[ -f "${checksums}" ] || fail "the checksum file was not produced"
check "release archive and checksum file exist"

# `REL-001`: the checksum must match the artifact.
if command -v sha256sum >/dev/null 2>&1; then
  (cd "${releases}" && sha256sum --check --status "$(basename "${checksums}")") ||
    fail "the published checksum does not match the artifact"
else
  (cd "${releases}" && shasum -a 256 --check --status "$(basename "${checksums}")") ||
    fail "the published checksum does not match the artifact"
fi
check "checksum matches the artifact"

# Deterministic packaging: the same commit and platform produce the same bytes.
second="${work}/second"
mkdir -p "${second}"
bash scripts/package.sh "${target}" "${second}" >/dev/null
if ! cmp -s "${archive}" "${second}/secretsieve-${version}-${target}.tar.gz"; then
  fail "packaging is not reproducible on this platform"
fi
check "packaging the same commit twice produces identical bytes"

# A release index in the shape `install.sh` parses, newest first.
index="${work}/index.json"
printf '[{"tag_name": "v%s"}]\n' "${version}" >"${index}"

installer() {
  env \
    HOME="${work}/home" \
    PATH="${PATH}" \
    SECRETSIEVE_RELEASE_INDEX="file://${index}" \
    SECRETSIEVE_RELEASE_BASE="file://${work}/releases" \
    bash "${root}/install.sh" "$@"
}

# --- clean install (`REL-002`) ----------------------------------------------

mkdir -p "${work}/home"
installer >"${work}/clean.log" 2>&1 || fail "a clean install failed"
binary="${work}/home/.local/bin/secretsieve"
[ -x "${binary}" ] || fail "the binary was not installed to the default location"
check "clean install placed the binary in ~/.local/bin"

reported="$("${binary}" --version | awk '{print $2}')"
[ "${reported}" = "${version}" ] || fail "the installed binary reports ${reported}"
"${binary}" --help >/dev/null || fail "the installed binary cannot print help"
check "the installed artifact runs --version and --help"

grep -q "checksum verified" "${work}/clean.log" || fail "the installer did not verify the checksum"
check "the installer verified the checksum before installing"

# `REL-003`: the installer must not run setup or touch any configuration.
if [ -e "${work}/home/.config/secretsieve" ] ||
  [ -e "${work}/home/.claude" ] ||
  [ -e "${work}/home/.codex" ] ||
  [ -e "${work}/home/.copilot" ] ||
  [ -e "${work}/home/.config/opencode" ]; then
  fail "the installer created configuration or harness files"
fi
check "no configuration or harness file was created"

# --- repeat install ---------------------------------------------------------

installer >"${work}/repeat.log" 2>&1 || fail "a repeat install failed"
grep -q "already installed" "${work}/repeat.log" || fail "a repeat install did not detect the current version"
check "a repeat install is a no-op"

# --- upgrade from an older release in the same major (`REL-004`, `REL-007`) --

major="${version%%.*}"
older_version="${major}.0.0"
if [ "${older_version}" = "${version}" ]; then
  printf '  [skip] no older release exists inside major %s to upgrade from\n' "${major}"
else
  stub_version="${older_version}" write_stub "${work}/home/.local/bin/secretsieve"
fi

# A V1 configuration written before the upgrade must still be readable after it.
mkdir -p "${work}/home/.config/secretsieve" "${work}/home/project"
cat >"${work}/home/.config/secretsieve/config.toml" <<'CONFIG'
version = 1

[[secret]]
source = "env"
name = "RELEASE_CHECK_TOKEN"
CONFIG
before="$(cat "${work}/home/.config/secretsieve/config.toml")"

installer >"${work}/upgrade.log" 2>&1 || fail "an upgrade from ${older_version} failed"
reported="$("${binary}" --version | awk '{print $2}')"
[ "${reported}" = "${version}" ] || fail "the upgrade did not replace the older binary"
check "an older install upgrades within the same major version"

after="$(cat "${work}/home/.config/secretsieve/config.toml")"
[ "${before}" = "${after}" ] || fail "the upgrade changed the configuration file"
if ! env HOME="${work}/home" RELEASE_CHECK_TOKEN="release-check-value" \
  sh -c "cd '${work}/home/project' && '${binary}' status" | grep -q "active          1 value"; then
  fail "the existing V1 configuration is not runtime-readable after the upgrade"
fi
check "an existing V1 configuration stays runtime-readable after the upgrade"

# --- corrupt download -------------------------------------------------------

cp "${archive}" "${work}/archive.good"
printf 'corrupted' >>"${archive}"
# Remove the installed binary first, so the installer actually attempts the
# download instead of reporting that this version is already installed.
rm -f "${binary}"
if installer >"${work}/corrupt.log" 2>&1; then
  fail "a corrupt download was installed"
fi
grep -q "checksum mismatch" "${work}/corrupt.log" ||
  fail "the corrupt download was rejected for the wrong reason"
[ ! -e "${binary}" ] || fail "the corrupt download left a binary behind"
# No staged temporary file may be left behind either.
if [ -n "$(find "${work}/home/.local/bin" -name '.secretsieve.install.*' 2>/dev/null)" ]; then
  fail "the corrupt download left a staged temporary file behind"
fi
cp "${work}/archive.good" "${archive}"
installer >/dev/null 2>&1 || fail "reinstalling after a corrupt download failed"
check "a corrupt download is rejected, installs nothing, and leaves no temporary file"

# --- hostile archive contents ------------------------------------------------

# An archive whose `secretsieve` member is a symlink must be refused rather than
# followed, and an archive carrying extra paths must not have them extracted.
hostile="${work}/hostile"
mkdir -p "${hostile}/v${version}"
staging="$(mktemp -d)"
ln -s /etc/passwd "${staging}/secretsieve"
mkdir -p "${staging}/extra"
printf 'unwanted\n' >"${staging}/extra/payload"
# No --dereference, so the symlink is archived as a symlink.
tar --create --gzip \
  --directory "${staging}" \
  --file "${hostile}/v${version}/secretsieve-${version}-${target}.tar.gz" \
  secretsieve extra
(
  cd "${hostile}/v${version}"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "secretsieve-${version}-${target}.tar.gz" >"secretsieve-${version}-SHA256SUMS"
  else
    shasum -a 256 "secretsieve-${version}-${target}.tar.gz" >"secretsieve-${version}-SHA256SUMS"
  fi
)
rm -rf "${staging}"

rm -f "${binary}"
if env HOME="${work}/home" \
  SECRETSIEVE_RELEASE_INDEX="file://${index}" \
  SECRETSIEVE_RELEASE_BASE="file://${hostile}" \
  bash "${root}/install.sh" >"${work}/hostile.log" 2>&1; then
  fail "an archive whose binary member is a symlink was installed"
fi
[ ! -e "${binary}" ] || fail "the hostile archive left a binary behind"
[ ! -e "${work}/home/.local/bin/extra" ] || fail "the hostile archive wrote an extra path"
installer >/dev/null 2>&1 || fail "reinstalling after the hostile archive failed"
check "an archive with a symlinked or extra member is refused"

# --- major upgrade gating (`REL-004`) --------------------------------------

# An installed binary from the next major version, so the packaged release is
# across a major boundary from it.
other_major="$((major + 1)).0.0"
stub_version="${other_major}" write_stub "${binary}"
majorindex="${work}/major.json"
printf '[{"tag_name": "v%s"}]\n' "${version}" >"${majorindex}"

if env HOME="${work}/home" \
  SECRETSIEVE_RELEASE_INDEX="file://${majorindex}" \
  SECRETSIEVE_RELEASE_BASE="file://${work}/releases" \
  bash "${root}/install.sh" --version "${version}" >"${work}/major.log" 2>&1; then
  fail "crossing a major version was allowed without the flag"
fi
grep -q "allow-major-upgrade" "${work}/major.log" ||
  fail "the major-version refusal did not name the flag"
check "crossing a major version requires an explicit opt-in"

env HOME="${work}/home" \
  SECRETSIEVE_RELEASE_INDEX="file://${majorindex}" \
  SECRETSIEVE_RELEASE_BASE="file://${work}/releases" \
  bash "${root}/install.sh" --allow-major-upgrade >"${work}/major-ok.log" 2>&1 ||
  fail "an explicit major upgrade failed"
reported="$("${binary}" --version | awk '{print $2}')"
[ "${reported}" = "${version}" ] || fail "the explicit major upgrade did not install"
check "an explicit major upgrade installs the latest stable release"

# --- alternative install directory -----------------------------------------

installer --install-dir "${work}/custom/bin" >/dev/null 2>&1 ||
  fail "--install-dir failed"
[ -x "${work}/custom/bin/secretsieve" ] || fail "--install-dir did not place the binary"
check "--install-dir installs elsewhere"

# --- unknown usage ---------------------------------------------------------

if installer --not-a-flag >/dev/null 2>&1; then
  fail "an unknown option was accepted"
fi
check "an unknown option is rejected"

printf '  result      %d checks passed\n' "${pass}"

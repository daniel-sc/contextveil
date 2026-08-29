# Release Process

`CHANGELOG.md` is the canonical release history. Keep entries concise and
focused on user-visible changes; link issues, pull requests, and contributors
when useful. Product detail belongs in the README and linked documentation.

Pre-release entries describe changes since the previous pre-release in the same
release train. The first pre-release falls back to the previous stable release,
or an explicit project baseline when no stable release exists.

Stable entries describe changes since the previous stable release. Carry forward
features from the pre-release cycle and include fixes for bugs that existed in
that stable release. Do not present bugs introduced and fixed only during the
pre-release cycle as stable fixes.

Pre-release-only fixes advance the pre-release identifier (`beta.1` to
`beta.2`), not the SemVer patch version. The release creator or agent chooses the
appropriate comparison baseline and curates the final wording.

The release workflow publishes the changelog section matching the tag, adds the
version-specific install guidance, and uploads the verified artifacts.

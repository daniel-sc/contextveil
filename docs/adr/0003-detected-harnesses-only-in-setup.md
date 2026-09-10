# Offer Detected Harnesses Only During Setup

## Status

Accepted

## Context

The setup integration phase previously displayed every supported harness and
allowed a user to install one even when ContextVeil could not detect its
executable or configuration directory. This made a first-run screen show
options unrelated to the user's machine and made the resulting installation
hard to verify.

The alternative is useful for non-standard, remote, or not-yet-initialized
host environments, but it adds an opaque choice to the onboarding path and can
produce an integration that the host never loads.

## Decision

Setup presents harnesses that are detected, plus harnesses with an existing
ContextVeil-managed integration so those installations remain maintainable.
Undetected harnesses without an existing managed integration are not offered
and there is no secondary reveal action in V1.

## Consequences

- First-run integration selection is limited to plausible local choices.
- A setup run with no detected harnesses explains that no supported installation
  was found and does not write an integration.
- Existing managed integrations remain visible even if detection later fails,
  so users can inspect or remove them.
- Users with a non-standard or remote host must make the host detectable in the
  setup environment before installing an integration.

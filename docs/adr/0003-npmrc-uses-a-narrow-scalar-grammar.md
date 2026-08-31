# npmrc Uses A Narrow Scalar Grammar

## Status

Accepted

## Decision

ContextVeil parses npmrc sources with a small top-level scalar grammar rather
than claiming compatibility with npm or a generic INI parser. It preserves exact
keys, supports common comments and quoting, isolates recoverable syntax issues
to their keys, and deliberately treats `${NAME}` as literal text. This keeps
runtime resolution deterministic and non-executing, but users must enroll the
underlying environment source when npm substitutes a credential at use time.

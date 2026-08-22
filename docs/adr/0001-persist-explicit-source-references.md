# Persist Explicit Source References

Known Source Rules are setup-time candidate-admission knowledge only. Enrollment
persists an explicit resolver type, path, and selector rather than a Known Source
Rule identity,
so runtime behavior remains inspectable and cannot change when a maintained
rule definition changes. Persisting a Known Source Rule identity would follow
path overrides and schema updates automatically, but it would also make an
existing configuration read new locations or fields after an upgrade. Path
overrides are therefore resolved during setup, and users rerun setup when those
overrides change.

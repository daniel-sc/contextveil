# Requirement-To-Test Traceability Audit

This document maps every normative requirement ID in
[specification.md](specification.md) to its implementation and its test or
check evidence. `specification.md` is authoritative for observable behavior;
this audit does not change it and records no new requirements.

Each row gives one requirement ID, a one-line paraphrase (not a substitute for
the normative text), where the behavior lives in the source tree, the test or
check that would fail if the behavior regressed, and a status:

- `covered` — implemented, with a test or check that would fail on regression.
- `covered-by-design` — nothing to implement (typically a prohibition satisfied
  by the absence of code); the reason is given in the same clause.
- `manual` — verifiable only by a human or a paid/networked run.
- `accepted-limitation` — implemented behavior with an explicitly accepted gap.
- `gap` — no implementation or evidence was found.

Findings were produced by reading the cited source and test files directly,
not by trusting requirement-ID comments alone. To regenerate this mapping,
re-run `grep -rn "<ID>"` across `src/` and `tests/` for each ID in
specification.md, read the matched code and its nearest test, and re-classify.
Every row was checked against the repository as of the point this document was
written; the verification result for this documentation update is reported at
the end of the audit.

## 1. Security Claim (`SEC-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| SEC-001 | Help prevent resolved values reaching model context via covered adapter paths | src/matcher.rs (`Redactor`), src/redact.rs, src/adapter/{claude,codex,copilot,opencode}.rs | tests/process_boundaries.rs and tests/opencode/plugin.test.ts generated-canary boundaries | covered |
| SEC-002 | Must not claim protection beyond the support matrix | Public documentation; limitations.md LIM-001/LIM-002 | Manual release review compares public claims with the requirement; prose meaning is not inferred from keywords | manual |
| SEC-003 | Runtime resolution/redaction make no network calls; install and the Claude live canary are the only network-capable workflows | Cargo.toml has no HTTP client dependency; src/integration/claude.rs (`live_canary`) is the one exception | tests/diagnose.rs (comment-anchored assertion that nothing else reaches the network, ~line 298); tests/diagnose.rs::doctor_is_not_offered_the_live_canary_without_a_terminal | covered |
| SEC-004 | Must not persist, configure, or diagnose with resolved values | src/setup/write.rs; src/matcher.rs intervention metadata; src/source.rs malfunction reasons | tests/leaks.rs setup/diagnostic fixtures and generated-canary boundary tests | covered |
| SEC-005 | No telemetry, crash upload, analytics, or persistent runtime logging | Absence of any telemetry, crash-upload, analytics, or logging dependency or code path | tests/leaks.rs::runtime_writes_no_log_or_telemetry_file (walks the isolated home before/after every adapter, status, and doctor run and asserts no new file appears) | covered |
| SEC-006 | Untrusted terminal strings render as one visible-escaped logical line; non-UTF-8 path bytes render as `\xNN` | src/sanitize.rs::text, ::path, ::bytes | src/sanitize.rs unit tests (`control_characters_become_visible_escapes`, `escape_sequences_cannot_reach_the_terminal`, `bidi_and_separator_controls_are_escaped`, `every_rendering_occupies_one_logical_line`, `invalid_utf8_bytes_are_escaped`, `non_utf8_paths_are_rendered_without_raw_bytes`); tests/leaks.rs::terminal_hostile_names_and_paths_are_escaped_in_diagnostics; tests/setup.rs::terminal_escapes_in_names_and_paths_are_neutralized | covered |

## 2. Supported Platforms And Integrations (`SUP-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| SUP-001 | Support Linux and macOS on x86_64 and arm64 | .github/workflows/release.yml (package matrix: x86_64/aarch64-linux-gnu, aarch64/x86_64-apple-darwin); install.sh platform detection | scripts/release-check.sh (native artifact + checksum verification); .github/workflows/ci.yml matrix (ubuntu-latest x86_64, macos-latest arm64) | covered |
| SUP-002 | Claude is production; Codex, Copilot, OpenCode are experimental | src/integration/mod.rs (`Tier` enum) | src/integration/mod.rs::only_claude_is_production; tests/documentation.rs::public_support_matrices_have_the_required_tiers | covered |
| SUP-003 | Experimental integrations labeled EXPERIMENTAL everywhere; opt-in only; not counted as production health | src/diagnose.rs:534 (`Tier::Experimental => " (EXPERIMENTAL)"`); src/setup/integrations.rs:171-174 (affirmative installation only) | tests/diagnose.rs (asserts output contains "EXPERIMENTAL", ~line 884); tests/setup.rs::an_experimental_integration_requires_an_affirmative_choice; tests/documentation.rs::public_support_matrices_have_the_required_tiers | covered |
| SUP-004 | No host version checks; health from configuration and synthetic checks | No version-detection code exists anywhere in src/; doc comment in src/integration/mod.rs states this explicitly | covered-by-design — a prohibition satisfied by the absence of any version-comparison code; DIA-003/DIA-006 evidence the config+synthetic-check alternative | covered-by-design |
| SUP-005 | Coverage applies to local harness modes honoring the integration; cloud/remote/container modes need separate install | README.md support matrix | Manual release review compares the coverage statements with the requirement | manual |

## 3. CLI (`CLI-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| CLI-001 | `setup` is the only configuration workflow; no `init`/install/remove/slash commands | src/cli.rs::parse (rejects `init`, `install`, `uninstall`, `enroll`; only `setup`/`status`/`doctor` plus the hidden `hook` entry point) | src/cli.rs::v1_rejects_removed_and_unknown_commands, ::help_lists_only_public_commands, tests/cli.rs::help_hides_harness_protocol_entry_points | covered |
| CLI-002 | `setup` requires an interactive TTY; fails clearly without changing files otherwise | src/cli.rs::run_setup (`is_terminal()` check on stdin/stdout) | tests/cli.rs::setup_refuses_to_run_without_a_terminal | covered |
| CLI-003 | Public commands are human-readable only; no stable JSON contract | src/cli.rs::parse (only `-h/--help`, `-V/--version` accepted; any other flag is `UnknownOption`); limitations.md LIM-021 | src/cli.rs (flag-rejection tests); output is asserted as plain text throughout tests/diagnose.rs and tests/setup.rs | covered |
| CLI-004 | `setup` returns zero only when every write/action/verification completes | src/setup/mod.rs (cancellation and phase-failure handling) | tests/setup.rs::cancelling_the_first_phase_writes_nothing, ::a_project_phase_failure_keeps_the_committed_global_phase, ::a_malformed_settings_file_fails_the_integration_phase_without_changing_it | covered |
| CLI-005 | `status` returns zero whenever inspection completes, nonzero only if inspection itself fails | src/diagnose.rs (status exit logic) | tests/diagnose.rs::a_healthy_machine_exits_zero_for_both_commands, ::a_partially_unresolved_registry_is_healthy, ::an_inspection_that_cannot_complete_exits_two | covered |
| CLI-006 | `doctor` exit codes 0/1/2 per the documented health-failure rules | src/diagnose.rs (exit-code derivation) | tests/diagnose.rs::a_fully_inactive_registry_is_a_health_failure, ::a_partially_unresolved_registry_is_healthy, ::an_approved_conflict_stays_healthy_but_visible, ::an_inspection_that_cannot_complete_exits_two | covered |
| CLI-007 | Diagnosed process-hook failures exit zero after emitting valid host protocol output | src/cli.rs process-hook entry points; adapter malformed-input handling | Adapter malformed-input decision tables assert host-specific exits and safe output | covered |

## 4. Configuration Locations And Selection (`CFG-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| CFG-001 | Global config path is XDG-based; directory/file are user-only | src/config.rs (`global_config_path`); src/setup/write.rs (0o700/0o600 permissions) | config.rs unit test on the XDG path; write.rs permission-mode test | covered |
| CFG-002 | Project config filename is `.contextveil.toml` | src/paths.rs (`PROJECT_CONFIG_FILENAME`) | tests/setup.rs (literal filename used throughout) | covered |
| CFG-003 | Setup project root: nearest `.contextveil.toml`, else Git worktree root, else cwd | src/paths.rs (`setup_project_root`) | paths.rs::project_root_selection_prefers_the_nearest_config, ::project_root_falls_back_to_the_git_worktree_then_the_directory, ::a_git_file_marks_a_worktree_root; tests/setup.rs::the_project_root_is_selected_from_the_working_directory | covered |
| CFG-004 | Runtime uses at most one, nearest-ancestor project registry; no merging | src/paths.rs (`runtime_project_config`); src/registry.rs (`build`) | registry.rs fixture asserting exactly one project registry is used | covered |
| CFG-005 | Per-adapter project root selection (Claude/OpenCode stable root; Codex/Copilot may use cwd) | src/adapter/{claude,codex,copilot,opencode}.rs | Per-adapter project-root unit fixtures, including Codex and Copilot event-cwd tests | covered |
| CFG-006 | `version = 1` required; unknown fields/types/malformed entries/duplicate identities invalidate the file, including JSON identities | src/config.rs (`parse`, `parse_entry`); src/source.rs (`SourceRef::id`) | config.rs strict-field and normalized-identity tests for env, dotenv, and JSON | covered |
| CFG-007 | An env entry needs `source = "env"` plus non-empty `name`, no dotenv fields | src/config.rs (`parse_entry`, "env" arm) | config.rs::environment_entries_reject_dotenv_fields | covered |
| CFG-008 | A dotenv entry needs `file` plus exactly one of `key`/`all` | src/config.rs (`parse_entry`, "dotenv" arm) | config.rs::dotenv_entries_require_exactly_one_of_key_or_all | covered |
| CFG-009 | Global/project may share identity; project may reference external files/env names | src/config.rs (no cross-file identity check); src/registry.rs (`build`) | config.rs::project_config_may_reference_external_paths_and_environment_names; registry.rs::cross_scope_duplicate_identities_are_allowed | covered |
| CFG-010 | Paths stored as entered; `~/` expands to home; relative paths resolve against the config file; no env/glob/shell expansion | src/paths.rs (`expand`); src/config.rs (stores the entered string) | paths.rs::relative_paths_resolve_against_the_config_directory, ::a_leading_tilde_expands_to_the_home_directory, ::other_expansions_never_happen; config.rs::paths_are_stored_as_entered | covered |
| CFG-011 | Effective enrollment is additive (global + project); no negation/override | src/registry.rs (`build`) | registry.rs fixtures combining global and project registries | covered |
| CFG-012 | Invalid/unreadable config disables the entire effective registry, all-or-nothing | src/registry.rs (`Outcome::Malfunction`); src/config.rs (`Load::Invalid`) | registry.rs::an_invalid_project_config_disables_global_redaction, ::an_invalid_global_config_disables_project_redaction | covered |
| CFG-013 | Missing global config warns but keeps valid project redaction; missing project config is normal | src/registry.rs (`Warning::GlobalConfigMissing`); src/config.rs (`Load::Missing`) | registry.rs::a_missing_global_config_warns_but_keeps_project_redaction, ::a_missing_project_config_leaves_project_enrollment_empty | covered |
| CFG-014 | Setup must not overwrite invalid existing config; shows sanitized path/reason | src/setup/mod.rs (preflight, invalid-config reporting) | tests/setup.rs::an_invalid_existing_config_is_preserved_byte_for_byte, ::an_invalid_project_config_stops_setup_before_the_global_phase | covered |
| CFG-015 | Setup preserves existing valid enrollment by default; permits deliberate removal; never auto-removes unresolved entries | src/setup/mod.rs (enrollment-preservation logic) | tests/setup.rs::existing_enrollment_survives_a_rerun_even_when_unresolved, ::an_enrolled_entry_can_be_removed_deliberately | covered |
| CFG-016 | JSON entries require an explicit file and non-empty plain RFC 6901 pointer, with no wildcards or cross-source fields | src/config.rs (`parse_entry`, JSON arm); src/json.rs (`final_token`) | config.rs::json_entries_are_strict_and_require_a_supported_pointer; json.rs pointer-validation tests | covered |
| CFG-017 | Properties entries require an explicit file and exact decoded key, with no wildcard or inferred resolver | src/config.rs (`parse_entry`, properties arm) | config.rs::properties_entries_require_only_an_exact_file_and_decoded_key | covered |

## 5. Configuration Schema

Schema requirements are numbered under `CFG-*` above (`CFG-006` through
`CFG-010` and `CFG-016` cover the schema itself); there is no separate ID range
for section 5.

## 6. Source Resolution (`SRC-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| SRC-001 | Env reference resolves case-sensitively from the inherited environment | src/source.rs (`resolve_environment`) | source.rs::environment_names_are_case_sensitive_and_empty_values_are_unresolved | covered |
| SRC-002 | Unset/empty/non-UTF-8 env value is unresolved, never enters the matcher | src/source.rs (`Unresolved::NonUtf8`) | source.rs::non_utf8_environment_values_never_enter_the_matcher | covered |
| SRC-003 | Deterministic dotenv grammar (export, quoting, CRLF, comments, escapes) | src/dotenv.rs (`parse`, `Parser`) | src/dotenv.rs unit tests covering every grammar clause; fuzz/regressions/dotenv/{bom-and-trailing-comment,crlf-inside-quotes,export-space-is-malformed,unterminated-single-quote} | covered |
| SRC-004 | Last dotenv assignment wins; setup/doctor warn without showing values | src/dotenv.rs (last-key-wins); src/setup/mod.rs (duplicate warning) | dotenv.rs::the_last_assignment_wins_and_duplicates_are_reported; tests/setup.rs::duplicate_dotenv_keys_are_warned_about_without_values | covered |
| SRC-005 | Absent file/key or empty value is unresolved, not a malfunction | src/source.rs (`Resolution::Unresolved`) | source.rs::absent_files_keys_and_empty_values_are_unresolved | covered |
| SRC-006 | Permission denial, malformed dotenv, invalid UTF-8, or I/O failure disables the whole effective registry | src/source.rs (`SourceMalfunction`) | source.rs::malformed_and_invalid_utf8_files_are_malfunctions, ::an_unreadable_file_is_a_malfunction; src/registry.rs all-or-nothing tests | covered |
| SRC-007 | A wildcard entry resolves every current non-empty key without another setup run | src/source.rs (`Resolver::resolve`, `DotenvAll` arm) | source.rs::a_wildcard_entry_resolves_every_current_non_empty_key | covered |
| SRC-008 | No ContextVeil-specific dotenv size cap | src/dotenv.rs (linear parser, no size check) | tests/limits.rs (large-dotenv-file case); covered-by-design: absence of any cap-checking code | covered |
| SRC-009 | Sources resolved afresh per event; no cross-process cache or rotation history | src/source.rs (`Resolver` constructed per event; a file is read once per event only) | source.rs::a_file_is_read_once_per_event_and_duplicates_are_recorded | covered |
| SRC-010 | Dotenv changes observable next event; env changes need a harness restart | src/source.rs (doc comment tying this to SRC-009's per-event `Resolver`) | covered-by-design: an architectural consequence of a fresh `Resolver` per event plus process-immutable `Environment::from_process()`; not independently testable in-process (would require spawning a new harness process) | covered-by-design |
| SRC-011 | JSON source resolver accepts full JSON5, uses one exact pointer, rejects duplicate members and more than 128 nested containers, and performs no transformation | src/json.rs (`preflight`, `json5::from_str`, duplicate-aware visitor, `select`) | json.rs::full_json5_forms_parse_and_exact_pointers_still_select, ::json5_duplicate_members_at_every_depth_are_rejected, ::excessive_nesting_is_rejected_before_deserialization; tests/limits.rs::a_deeply_nested_json_source_is_a_malfunction_not_a_stack_overflow | covered |
| SRC-012 | Non-empty selected JSON strings resolve; missing/empty/non-string targets are unresolved | src/source.rs (`SourceRef::Json` resolution) | source.rs::a_json_pointer_resolves_only_a_non_empty_string | covered |
| SRC-013 | Missing JSON source files are unresolved; malformed JSON5/unreadable/non-UTF-8/duplicate-member files malfunction | src/source.rs (`read_json`) using the JSON5 source parser | source.rs::missing_malformed_non_utf8_and_duplicate_json_are_classified and JSON5 resolver fixtures | covered |
| SRC-014 | JSON files are parsed once per event where practical and never cached across hook processes | src/source.rs (`Resolver::json_files`, constructed per event) | source.rs::a_json_file_is_parsed_once_per_event_and_fresh_next_event | covered |
| SRC-015 | JSON5 applies only to persisted JSON sources; protocols and integration files remain strict unless separately specified | Source parsing is isolated in src/json.rs; adapters and integration editors continue using strict `serde_json` protocol/config parsing | Existing malformed-protocol fixtures and integration parsing tests reject non-strict input | covered |
| SRC-016 | Every decoded source value is trimmed before resolution and all downstream semantics use it | src/source.rs (`resolve_text` and wildcard resolution) | source.rs::every_source_family_trims_values_before_resolution; registry.rs::properties_values_are_active_and_malformed_files_disable_the_whole_registry; tests/process_boundaries.rs::enrolled_values_are_absent_at_every_process_boundary_after_intervention | covered |
| SRC-017 | Exact-key properties resolution follows transactional java-properties 2.0.0 behavior with last-key-wins duplicates | src/properties.rs; src/source.rs (`PropertiesFileState`) | properties.rs parser fixtures; source.rs::properties_resolve_exact_decoded_keys_and_report_duplicates, ::unreadable_file_sources_are_malfunctions; registry.rs::properties_values_are_active_and_malformed_files_disable_the_whole_registry | covered |

## 7. Setup Discovery And Enrollment (`SET-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| SET-001 | Setup presents four phases in order after preflight parse of both config files | src/setup/mod.rs (`run`, preflight) | tests/setup.rs::an_invalid_project_config_stops_setup_before_the_global_phase, ::a_project_phase_failure_keeps_the_committed_global_phase | covered |
| SET-002 | Setup applies every applicable Known Source Rule independently of adapter selection | Candidate discovery runs before integration selection in src/setup/mod.rs; environment and credential-document discovery do not inspect selected adapters | Existing name, URL, additive-location, and bounded-probe setup fixtures | covered |
| SET-003 | One recursive project walk supplies dotenv, anchored JSON, and eligible properties inputs with documented exclusions; no symlinks/special files | src/setup/discovery.rs (`project_files`, `walk`) | discovery.rs project-walk, exclusion, symlink, FIFO, non-UTF-8, and properties fixtures | covered |
| SET-004 | Global dotenv probing bounded to home + harness config directories, non-recursive | src/setup/discovery.rs (`global_dotenv_files`) | discovery.rs::global_probing_is_bounded_to_the_documented_locations; tests/setup.rs::global_dotenv_probing_covers_the_documented_locations | covered |
| SET-005 | Manual paths/keys/wildcard/env names, JSON pointers, and properties decoded keys allowed; absent manual sources savable after confirmation | src/setup/mod.rs (`add_manual`) | tests/setup.rs::an_unresolved_manual_source_requires_confirmation and manual env, JSON, and exact properties fixtures | covered |
| SET-006 | Secret-like name Known Source Rule uses the exact maintained vocabulary without candidate ranking, weights, confidence, or value-shape annotations | src/setup/vocabulary.rs; src/setup/mod.rs (`build_items`, `item_for`) | vocabulary.rs gating tests; tests/setup.rs::rule_count_does_not_change_candidate_order, ::value_shape_does_not_change_candidate_order, ::setup_shows_a_masked_preview_and_rules_without_shape_details | covered |
| SET-007 | Selection defaults apply to each Candidate Group or standalone source; existing or manual membership wins over collision defaults | src/setup/enrollment.rs; src/setup/mod.rs (`automatic_item_for`, `annotate_collisions`) | tests/setup.rs::an_enrolled_alias_keeps_its_colliding_group_selected, ::a_known_source_group_with_an_external_collision_defaults_unselected | covered |
| SET-008 | User is authoritative: enrollment allowed after a collision warning; no minimum length | src/setup/mod.rs (no length gate anywhere in setup or matcher) | tests/setup.rs::a_collision_can_be_overridden_by_the_user; absence of a length check corroborated by REG-001 | covered |
| SET-009 | Wildcard enrollment requires an additional explicit confirmation | src/setup/mod.rs (`add_manual`, wildcard branch) | tests/setup.rs::wildcard_enrollment_requires_an_extra_confirmation | covered |
| SET-010 | Preview masking table by Unicode scalar length; no fingerprint shown | src/setup/preview.rs (`mask`, `describe`) | preview.rs unit tests, including `boundaries_follow_the_specified_table`, `length_is_counted_in_unicode_scalar_values`, `no_fingerprint_is_derived_from_the_value` | covered |
| SET-011 | Collision analysis is byte-exact and excludes every whole equal-value alias file | src/setup/collision.rs (`Subject::source_files`, `analyze`); src/setup/mod.rs (`alias_inventory`) | collision.rs::every_equal_value_alias_file_is_excluded; tests/setup.rs::every_alias_file_is_excluded_but_an_unrelated_collision_remains, ::equal_values_in_different_phases_remain_separate_choices, ::properties_are_discovered_and_enrolled_as_exact_keys | covered |
| SET-012 | Collision output shows counts and sanitized filenames only, never values/snippets | src/setup/collision.rs (`Collisions::describe`) | collision.rs::reports_contain_filenames_and_counts_but_never_values, ::filenames_are_sanitized_for_the_terminal | covered |
| SET-013 | Unavailable non-enrolled files excluded without aborting discovery; enrolled malformed sources must be repaired or removed | src/setup/discovery.rs (`inspect`); src/setup/mod.rs (blocking on enrolled malformed sources) | discovery.rs::malformed_and_unreadable_files_are_marked_unavailable; tests/setup.rs::an_enrolled_malformed_source_must_be_repaired_or_removed, ::an_unavailable_discovered_file_does_not_stop_discovery | covered |
| SET-014 | Atomic phase commits and exact per-integration rollback | src/setup/write.rs; src/setup/integrations.rs (`ArtifactSnapshot`) | tests/setup.rs::verification_failure_removes_new_artifacts, ::verification_failure_restores_previous_state_byte_for_byte, ::later_failure_keeps_completed_earlier_actions | covered |
| SET-015 | Required source/integration actions remain available and every continuing interaction rerenders | src/setup/mod.rs; src/setup/render.rs | tests/setup.rs::selection_screen_rerenders_after_every_continuing_interaction | covered |
| SET-016 | Equal-value aliases form one selectable enrollment unit and selected groups persist every represented source | src/setup/enrollment.rs; src/setup/mod.rs; src/setup/render.rs | tests/setup.rs::equal_value_group_toggle_applies_to_every_alias, ::aliases_split_into_separate_rows_after_their_values_diverge, ::resolvable_manual_sources_merge_into_an_existing_group, ::manual_addition_preserves_existing_display_order | covered |
| SET-017 | Credential-bearing URL values already surfaced by bounded discovery become whole-value candidates without recursive scanning | src/setup/credential_url.rs; src/setup/mod.rs; src/setup/known_source.rs | credential_url.rs fixtures; setup env/dotenv/properties URL fixtures | covered |
| SET-018 | Known Source Rules are automatic admission only, run independently of adapters, inspect additive bounded locations, and persist explicit references | src/setup/mod.rs, src/setup/known_source.rs, and src/setup/discovery.rs; no runtime rule variant exists in src/source.rs | known_source.rs probe fixtures; tests/setup.rs::known_sources_persist_explicit_refs_and_bypass_name_gating, ::known_source_override_reruns_are_idempotent_and_pick_up_changes, ::non_utf8_known_source_override_keeps_default_discovery | covered |
| SET-019 | Recognized credential documents use JSON5 and bounded permissive probes; non-empty listed strings admit independently, while malformed documents are unavailable | src/setup/known_source.rs declarative probes and shared evaluator delegate parsing to src/json.rs | known_source.rs::probes_are_bounded_and_accept_only_non_empty_strings; tests/setup.rs::known_sources_persist_explicit_refs_and_bypass_name_gating | covered |
| SET-020 | Credential document rules cover only the additive host locations, bounded containers, and inventory leaves; keychains, helpers, sidecars, and unlisted fields remain outside | src/setup/known_source.rs data descriptors and src/setup/discovery.rs anchored project traversal | known_source.rs exact-path safety tests; discovery.rs::one_project_walk_collects_only_anchored_known_source_json; tests/setup.rs::known_sources_persist_explicit_refs_and_bypass_name_gating | covered |
| SET-021 | Properties Known Source discovery covers eligible project files and additive Gradle roots with localization exclusions | src/setup/discovery.rs properties predicate; src/setup/known_source.rs properties admission | discovery.rs::properties_discovery_handles_monorepos_and_localization_exclusions; tests/setup.rs properties and Gradle fixtures | covered |

## 8. Effective Registry (`REG-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| REG-001 | Every non-empty normalized resolved value is an exact match pattern; no heuristics apply at runtime | src/source.rs; src/matcher.rs (`Redactor::new`) | source normalization and matcher exactness fixtures | covered |
| REG-002 | Duplicate resolved values collapse to one canonical pattern (first project entry, else first global entry, in file order) | src/matcher.rs (value dedup); src/registry.rs (canonical ordering) | src/matcher.rs::duplicate_values_collapse_to_the_canonical_source; src/registry.rs::equal_values_canonicalize_to_the_first_project_entry; src/diagnose.rs alias-warning test | covered |
| REG-003 | Source/key names are case-sensitive; labels derive from env name, dotenv/properties key, or final JSON pointer token, never a file path | src/secret.rs (`SourceId::label`); src/json.rs (`final_token`) | secret.rs label tests; source properties/JSON label tests | covered |
| REG-004 | Labels keep ASCII word characters, collapse other runs to `_` | src/secret.rs (`safe_label`) | src/secret.rs::labels_keep_only_the_allowed_character_set, ::labels_collapse_control_and_escape_sequences | covered |

## 9. Redaction Semantics (`RED-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| RED-001 | Matching is case-sensitive UTF-8 byte comparison; no normalization | src/matcher.rs (`match_at`, `redact`) | src/matcher.rs::matching_is_case_sensitive_and_byte_exact, ::utf8_values_match_without_normalization; tests/matcher_property.rs | covered |
| RED-002 | Matching operates independently per selected string value; fields are never joined | src/redact.rs (`redact_in_place`) | src/redact.rs::values_are_matched_independently_across_fields; Claude adapter shape units | covered |
| RED-003 | Leftmost-longest match selection with canonical-source tie-break | src/matcher.rs (`build_index`, `match_at`) | src/matcher.rs::same_start_overlap_prefers_the_longest_value, ::different_start_overlap_prefers_the_earliest_start; tests/matcher_property.rs::the_matcher_agrees_with_the_reference_model | covered |
| RED-004 | Substring matching; no token/word boundaries | src/matcher.rs (`redact`) | src/matcher.rs::matching_is_substring_matching | covered |
| RED-005 | Only decoded string values are transformed; keys, numbers, booleans, nulls untouched | src/redact.rs (`redact_in_place`) | src/redact.rs::object_keys_are_never_transformed, ::numbers_and_booleans_that_look_like_values_are_left_alone | covered |
| RED-006 | Placeholder fallback: `<SECRET:LABEL>` then `<SECRET>` then empty string | src/matcher.rs (`Redactor::new` decision logic) | src/matcher.rs::a_value_inside_the_named_placeholder_forces_the_generic_form, ::a_value_inside_every_placeholder_forces_deletion, ::a_placeholder_that_reproduces_a_value_is_rejected_before_insertion | covered |
| RED-007 | Generated placeholders are never rescanned/fed back through the matcher | src/matcher.rs (`redact`, cursor advances past a replacement) | src/matcher.rs::replacements_are_never_rescanned; tests/matcher_property.rs::no_active_value_survives_when_placeholders_cannot_be_reconstructed | covered |
| RED-008 | Intervention metadata carries counts/labels only, never values/hashes/content | src/matcher.rs (`Intervention`, `intervention`) | Matcher metadata units, generated-canary boundaries, and diagnostics leak fixture | covered |
| RED-009 | Clean events are silent; unresolved sources produce no runtime UI | src/matcher.rs (`redact` returns `None` on no match); adapter call sites | Matcher and per-adapter clean/unresolved decision fixtures | covered |
| RED-010 | No path replaces a placeholder with a source value later | No reverse-mapping code exists anywhere in src/; documented in src/redact.rs and README.md | covered-by-design — satisfied by the absence of any placeholder-to-source lookup path | covered-by-design |

## 10. Runtime Failure Policy (`RUN-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| RUN-001 | Malfunction/invalid config yields no partial redaction; original content passed with a warning | src/registry.rs (`Outcome::Malfunction`); process adapter call sites | Registry all-or-nothing tests and adapter malfunction decision fixtures | covered |
| RUN-002 | Claude, Codex, and Copilot are documented as fail-open | README.md support matrix; limitations.md LIM-012 | Manual release review verifies the documented failure behavior | manual |
| RUN-003 | The OpenCode plugin aborts covered operations on subprocess failure or timeout; notify failure preserves mutation | assets/opencode/plugin.ts | tests/opencode/plugin.test.ts timeout-abort, failure, and notification fixtures | covered |
| RUN-004 | Every installed hook/subprocess invocation uses a 5-second timeout | src/integration/{claude,codex,copilot}.rs (`TIMEOUT_SECONDS = 5`); assets/opencode/plugin.ts (`TIMEOUT_MS = 5000`) | src/integration/{claude,codex,copilot}.rs timeout-in-config tests; tests/{claude,codex,copilot}_hook.rs timeout-mapping tests | covered |
| RUN-005 | Runtime should target p95 below 100 ms for the documented workload (engineering benchmark, not a pass/fail gate) | benches/redaction.rs | `mise run bench` (`cargo bench --bench redaction`); tests/limits.rs::many_enrolled_values_stay_inside_the_host_timeout (loose 5-second wiring check only) | manual |
| RUN-006 | Malformed envelope/unknown event is diagnosed safely while valid uncovered content is preserved | src/adapter/{claude,codex,copilot,opencode}.rs | Adapter malformed/unknown decision fixtures and OpenCode invalid-protocol plugin fixture | covered |

## 11. Integration Installation (`INT-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| INT-001 | Detect all four harnesses; Claude selected by default; experimental integrations unselected unless already ContextVeil-installed | src/setup/integrations.rs (default-selection logic); src/integration/mod.rs (`Tier`) | src/integration/mod.rs::only_claude_is_production; tests/setup.rs::an_experimental_integration_requires_an_affirmative_choice | covered |
| INT-002 | A user may install an undetected harness; setup discloses limited verification | src/setup/integrations.rs (undetected-harness path) | tests/setup.rs::an_undetected_harness_discloses_limited_verification | covered |
| INT-003 | Absolute binary path, direct argument arrays, stdin/stdout, no shell interpolation | src/integration/mod.rs; src/integration/hooks_json.rs | Installer command-shape units and tests/process_boundaries.rs real stdin/stdout wiring | covered |
| INT-004 | No duplicate managed entries; removal only when ownership/identity is established; modified/user-owned entries preserved with a warning | src/integration/hooks_json.rs (`Installed::Modified`, classification); src/integration/opencode.rs, src/integration/copilot.rs (`classify`) | src/integration/claude.rs::malformed_settings_are_never_overwritten, ::removal_by_deselection_removes_only_the_managed_entry; tests/setup.rs::rerunning_setup_leaves_an_installed_integration_byte_identical, ::deselecting_the_integration_removes_only_the_managed_hook | covered |
| INT-005 | Competing mutating hooks shown for individual approval; an approved conflict is not a health failure but stays visible | src/integration/hooks_json.rs (`Conflict`); src/setup/integrations.rs (`approve_conflicts`) | tests/setup.rs::a_competing_mutating_hook_is_offered_for_approval; src/integration/claude.rs::other_post_tool_use_command_hooks_are_reported_as_conflicts; tests/diagnose.rs::an_approved_conflict_stays_healthy_but_visible, ::an_unapproved_conflict_is_a_health_failure | covered |
| INT-006 | Installation success is not permanent proof; status/doctor derive current state from config/host artifacts | src/integration/mod.rs (`inspect` re-derives state on every call; no cached "installed" flag) | tests/diagnose.rs::a_missing_integration_is_a_health_failure, ::an_uninstalled_integration_reports_no_timeout | covered |

## 12. Claude Code Adapter (`CLA-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| CLA-001 | One managed synchronous wildcard `PostToolUse` hook in `~/.claude/settings.json`, 5-second timeout | src/integration/claude.rs (`SPEC`, `settings_path`, `install`) | src/integration/claude.rs (installation test asserting timeout=5, single hook group) | covered |
| CLA-002 | Recursively redact `tool_response` strings, preserve keys/non-strings/shape, return via `hookSpecificOutput.updatedToolOutput` | src/adapter/claude.rs (`handle`, `finish`) | Claude adapter shape-preservation units and tests/process_boundaries.rs | covered |
| CLA-003 | On intervention, one safe `systemMessage` with count/labels; never `additionalContext` | src/adapter/claude.rs (`finish`) | src/adapter/claude.rs (intervention test asserting `systemMessage` present, no `additionalContext`) | covered |
| CLA-004 | Must not claim coverage for failed results, prompts, outgoing args, telemetry, local artifacts, or non-replaceable successes | Absence of such handling in src/adapter/claude.rs; limitations.md LIM-013 | Claude adapter failed-event negative fixture | covered |
| CLA-005 | Other matching `PostToolUse` hooks trigger INT-005 approval; once approved, they do not block healthy status | Shared conflict logic in src/integration/hooks_json.rs (see INT-005) | tests/diagnose.rs::an_approved_conflict_stays_healthy_but_visible | covered |

## 13. Codex CLI Adapter (`COD-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| COD-001 | One managed synchronous wildcard `PostToolUse` hook in `~/.codex/hooks.json`, 5-second timeout, host trust workflow | src/integration/codex.rs (`SPEC`, `hooks_path`, `install`, trust-note) | src/integration/codex.rs::installation_writes_the_documented_codex_shape, ::codex_is_experimental_and_carries_a_trust_note | covered |
| COD-002 | On a match, redact strings, block the original, provide sanitized text via the blocking mechanism | src/adapter/codex.rs (`handle`, `render`, `finish`) | src/adapter/codex.rs::a_match_blocks_the_original_and_supplies_sanitized_text, ::a_string_result_is_rendered_directly, ::structured_results_keep_their_shape_inside_the_rendering | covered |
| COD-003 | Disclose that intervention may turn a successful/structured result into error-like text and lose structure/images/types | src/adapter/codex.rs (`render` embeds the disclosure text); limitations.md LIM-014 | src/adapter/codex.rs::a_match_blocks_the_original_and_supplies_sanitized_text (asserts the disclosure wording) | covered |
| COD-004 | Must not claim every tool emits the event, that MCP results are shape-preserving, or full failed-result coverage | limitations.md LIM-014 documents all three explicitly | src/adapter/codex.rs::a_non_zero_exit_result_is_still_covered (documents the boundary) | covered |

## 14. GitHub Copilot CLI Adapter (`COP-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| COP-001 | Dedicated ContextVeil hook file under `~/.copilot/hooks/`, 5-second timeout, unrelated files untouched | src/integration/copilot.rs (`hook_file`, `install`, `managed_file`) | src/integration/copilot.rs::copilot_installs_one_dedicated_file_and_leaves_others_alone | covered |
| COP-002 | Redact `userPromptTransformed` and successful `postToolUse.toolResult.textResultForLlm`, preserve host result shape | src/adapter/copilot.rs (`handle`) | src/adapter/copilot.rs::a_transformed_prompt_is_redacted_with_one_progress_line, ::a_successful_tool_result_keeps_its_shape, ::extra_result_fields_are_preserved | covered |
| COP-003 | On intervention, one safe persistent progress summary before the final mutation object | src/adapter/copilot.rs (`redact_one` pushes the progress line) | src/adapter/copilot.rs::a_transformed_prompt_is_redacted_with_one_progress_line (asserts exactly one progress line) | covered |
| COP-004 | Must not claim coverage for failed tool errors, non-text attachments, other injection paths, or the local timeline prompt | limitations.md LIM-015 documents the gaps explicitly | src/adapter/copilot.rs::a_failed_tool_result_is_not_covered | covered |

## 15. OpenCode Adapter (`OCO-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| OCO-001 | One ContextVeil-owned TypeScript plugin file under `~/.config/opencode/plugins/`; JSON stdin/stdout to the absolute Rust binary | src/integration/opencode.rs (`plugin_file`, `install`, `render`) | src/integration/opencode.rs::installation_writes_one_owned_plugin_file; tests/opencode/plugin.test.ts (spawns the real plugin) | covered |
| OCO-002 | Use `chat.message` for new textual user parts and `tool.execute.after` for successful standard textual tool output | assets/opencode/plugin.ts (both handlers); src/adapter/opencode.rs (`Event`) | tests/opencode/plugin.test.ts ("new user text is redacted in place and announced", "successful standard tool output is redacted in place") | covered |
| OCO-003 | One safe named/count TUI notification when redaction occurs and the API is available | assets/opencode/plugin.ts (`announce`/`notify`) | tests/opencode/plugin.test.ts ("new user text is redacted...", "a notification failure does not undo the mutation") | covered |
| OCO-004 | Must not implement V2 APIs, provider wrappers, full-history/system transforms, tool-definition rewriting, or claim wider coverage | assets/opencode/plugin.ts (only the two documented hooks, no matcher logic); limitations.md LIM-016 | src/integration/opencode.rs::the_plugin_carries_no_matcher_or_resolver_logic; tests/opencode/plugin.test.ts ("explicitly unsupported paths are left alone without spawning") | covered |

## 16. Status And Doctor (`DIA-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| DIA-001 | Status inspects config, resolves sources, reports active/unresolved counts without adapter protocol tests; both select project root via CFG-003 from cwd | src/diagnose.rs (status implementation, project-root selection) | tests/diagnose.rs::status_runs_no_adapter_protocol_test, ::the_project_root_follows_the_working_directory | covered |
| DIA-002 | Registry and integration health are independent facets; zero active values shown as `INACTIVE` | src/diagnose.rs (registry/integration facets kept separate) | tests/diagnose.rs::a_partially_unresolved_registry_is_healthy, ::a_fully_inactive_registry_is_a_health_failure | covered |
| DIA-003 | Doctor additionally checks permissions, source errors, aliases, collisions, ownership, disabled hooks, conflicts, executables, timeouts, synthetic protocol behavior | src/diagnose.rs (`inspect` and permission/timeout/synthetic-check helpers, ~line 502, 720) | tests/diagnose.rs::malformed_configuration_fails_doctor_but_not_status, ::status_recognizes_a_hook_that_points_at_the_running_binary, ::an_uninstalled_integration_reports_no_timeout | covered |
| DIA-004 | Collision findings remain advisory and doctor applies grouped alias-file exclusions | src/diagnose.rs (`collision_findings` grouped subjects); src/setup/collision.rs | tests/diagnose.rs::doctor_groups_aliases_and_excludes_all_of_their_source_files | covered |
| DIA-005 | Optional paid/networked Claude live canary, disabled by default, requires confirmation, uses a random non-credential value, and passes only on a present placeholder | src/diagnose.rs (`LiveCanary` enum, `run_live_canary`); src/cli.rs (`run_doctor` gating); src/integration/claude.rs (`classify_canary`) | tests/diagnose.rs::doctor_is_not_offered_the_live_canary_without_a_terminal; src/integration/claude.rs::tests (reply classification: placeholder, inconclusive, disclosure, bytes, empty value); only the network request itself is exercised by a human (see limitations.md DEV-001) | manual |
| DIA-006 | Codex, Copilot, OpenCode have offline synthetic verification only; passing it does not remove the experimental label | src/diagnose.rs (`verify_offline` call, ~line 611-628) | src/integration/{codex,copilot,opencode}.rs offline-verification tests; tests/diagnose.rs (experimental label persists after a passing check) | covered |
| DIA-007 | A previous successful verification is never a permanent certificate | src/integration/state.rs (`Managed` stores only command + approved conflicts, no pass/fail history); src/diagnose.rs (re-derives every check on every run) | covered-by-design — no field anywhere persists a "verified" or "last passed" state, so a stale pass cannot be represented; doctor re-runs synthetic checks from scratch on every invocation | covered-by-design |
| DIA-008 | Doctor returns one for any diagnosed protection-preventing condition, two only for usage/internal failures | src/diagnose.rs (exit-code derivation) | tests/diagnose.rs::a_fully_inactive_registry_is_a_health_failure, ::a_missing_integration_is_a_health_failure, ::an_unapproved_conflict_is_a_health_failure, ::an_inspection_that_cannot_complete_exits_two | covered |

## 17. Installation And Release (`REL-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| REL-001 | Standalone checksummed GitHub Release artifacts for all four platform/arch targets | .github/workflows/release.yml (`package` job matrix: 4 targets); scripts/package.sh | scripts/release-check.sh (checksum-match assertion); release.yml `publish` job (merges and verifies `SHA256SUMS`) | covered |
| REL-002 | Maintained install script: detects platform/arch, downloads, verifies checksum, atomically installs, overridable default destination; a prerelease is never selected automatically and only `--version` may name one | install.sh (`--install-dir`, `--version`, `--allow-major-upgrade`; platform detection, checksum verification; `list_versions stable\|any`) | scripts/release-check.sh (clean-install, `--install-dir`, unknown-option rejection, and prerelease-selection cases, the last asserting a default run refuses a prerelease-only index and that naming the version installs it) | covered |
| REL-003 | Install script installs/upgrades the binary only; never runs setup, edits config, installs adapters, or accepts enrollment defaults | install.sh (doc comment: "never runs setup... never touches coding-agent configuration") | scripts/release-check.sh ("no configuration or harness file created" case) | covered |
| REL-004 | Rerunning the script upgrades within the installed major; crossing a major needs explicit opt-in | install.sh (version-selection logic) | scripts/release-check.sh (upgrade-same-major, major-gating, explicit-major-upgrade cases) | covered |
| REL-005 | Hooks and plugins never download/install/update the Rust binary | src/integration/mod.rs (doc comment: "no installer, hook, or plugin downloads or updates the binary. The only component that fetches anything is `install.sh`") | covered-by-design — no networking dependency exists in the hook/adapter code paths (Cargo.toml has no HTTP client); install.sh is the sole fetcher | covered-by-design |
| REL-006 | MIT OR Apache-2.0 license plus a public security-reporting policy | Cargo.toml (`license = "MIT OR Apache-2.0"`); LICENSE-MIT, LICENSE-APACHE | SECURITY.md (reporting instructions, response expectations) | covered |
| REL-007 | Every V1 release reads earlier V1 config/managed state without requiring setup to run first | scripts/release-check.sh (installs an older release, writes a V1 config, upgrades, reads the config with the new binary) | scripts/release-check.sh (upgrade case, ~line 118-151: "an existing V1 configuration still runtime-readable afterwards") | covered |
| REL-008 | Release qualification includes a manual live Claude test proving redaction survives session resume | Not automatable by design; run per release and recorded in docs/qualification.md | Run and passed 2026-08-17 against Claude Code 2.1.233 by an automated session: placeholder survived `claude -r`, value absent from the reply and the stored transcript. Human sign-off remains outstanding per docs/qualification.md. No automated test exists or should exist (`TST-008`); limitations.md DEV-001 records the automation gap | manual |

## 18. Testing And Acceptance (`TST-*`)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| TST-001 | Matcher tests cover empty/UTF-8/case/substrings/adjacent/overlap/duplicates/canonical labels/multiline/placeholder-fallback/no-recursion | src/matcher.rs unit tests (the named vectors); tests/matcher_property.rs (the same rules over generated input) | src/matcher.rs test module; tests/matcher_property.rs::the_matcher_agrees_with_the_reference_model | covered |
| TST-002 | Config/source tests cover full JSON5 source grammar while retaining pointer, duplicate-member, wrong-type, and failure coverage | src/json.rs and src/source.rs test modules | Full JSON5 forms, duplicate-member, exact-pointer, wrong-type, malformed, non-UTF-8, and freshness fixtures | covered |
| TST-003 | Filesystem tests cover additive locations, bounded permissive probes without sibling gating, exact/anchored paths, malformed and symlink boundaries, pointers, grouping, collisions, and leaks | src/setup/discovery.rs and src/setup/known_source.rs shared filesystem/probe tests; tests/setup.rs end-to-end setup fixtures | known_source.rs::probes_are_bounded_and_accept_only_non_empty_strings, exact FIFO/symlink/directory safety tests; tests/setup.rs::known_sources_persist_explicit_refs_and_bypass_name_gating, ::known_source_override_reruns_are_idempotent_and_pick_up_changes, ::non_utf8_known_source_override_keeps_default_discovery, ::project_known_source_aliases_form_one_candidate_group, ::a_known_source_group_with_an_external_collision_defaults_unselected | covered |
| TST-004 | Adapter decisions are tested at unit level and each covered path retains one real boundary fixture | src/adapter; tests/process_boundaries.rs; tests/opencode/plugin.test.ts | Four Rust process cases and two real-binary OpenCode cases | covered |
| TST-005 | Intervention fixtures prove input presence, intervention, and absence from every observable channel | src/testing.rs | tests/process_boundaries.rs and tests/opencode/plugin.test.ts | covered |
| TST-006 | Routine committed-corpus replay is separate from bounded mutation | src/fuzz.rs; src/bin/fuzz_smoke.rs | mise `fuzz-regressions` and `fuzz-smoke` tasks | covered |
| TST-007 | Routine CI uses mise and release verification consumes exact package artifacts | mise.toml; .github/workflows/release.yml | package-dependent native verification matrix | covered |
| TST-008 | Optional paid/networked tests do not gate routine CI; REL-008 gates a release only | Routine workflows invoke offline mise tasks; the live qualification is a documented manual release step | Covered by workflow design and review, not a keyword blacklist | covered-by-design |

## Gaps and manual items

## Non-Normative Design Baseline

| Baseline | Automated Aid | Required Review | Status |
| --- | --- | --- | --- |
| Setup rendering | One broad golden snapshot | Manual visual review | manual |

## Accepted Limitations

| Behavior | Record | Status |
| --- | --- | --- |
| Setup save may change the canonical placeholder alias | LIM-024 and `save_order_can_change_the_canonical_alias` | accepted-limitation |

No implementation or evidence gap remains in the confirmed Known Source Rule and
JSON5 source-document requirements. Strict harness protocols and integration
files remain separate from JSON sources. Planned npmrc and recognized INI store
rows are non-contract and require no current implementation evidence.

**Manual (verifiable only by a human or a paid/networked run):**

- **SEC-002, SUP-005, RUN-002** — release review checks the meaning of public
  security-boundary and failure-policy claims; keyword tests cannot establish it.
- **RUN-005** — the p95-latency benchmark is an engineering target, not a
  pass/fail gate; a human must run `mise run bench` and read the result.
- **DIA-005** — the gating, confirmation, random-value, and reply-classification
  logic around the Claude live canary is tested, but the live network request
  itself is made only when a human runs `contextveil doctor` and opts in
  (`limitations.md` DEV-001).
- **REL-008** — release qualification requires a manual live Claude test proving
  redaction survives session resume. It is deliberately outside automated CI
  (`TST-008`). It was run and passed on 2026-08-17 against Claude Code 2.1.233,
  but by an automated session rather than by a human at the terminal, so a
  release manager must still repeat or confirm it. `docs/qualification.md`
  records the procedure, the host's own transcript records, the result, and the
  scope of what one run proves, and must be rerun for each release.

**Covered-by-design (nothing to implement; satisfied by an absence):**

- **SUP-004** — no host-version-comparison code exists anywhere in the tree.
- **SRC-010** — the environment-restart half follows structurally from
  process-immutable `Environment::from_process()`; not independently
  testable without spawning a new harness process.
- **RED-010** — no placeholder-to-source reverse mapping exists anywhere.
- **REL-005** — no hook or adapter path has network capability; only
  `install.sh` fetches anything.
- **DIA-007** — no code path persists a "previously verified" flag, so a
  stale pass cannot be shown as a certificate.

**Noted caveat on otherwise-covered rows:**

- **SUP-001, REL-001** — implemented for all four targets, and the release
  workflow builds and packages each of them, but only
  `x86_64-unknown-linux-gnu` has been built and exercised in the development
  environment used so far. The other three need their CI runners.

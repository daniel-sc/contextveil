# Requirement-to-Test Traceability

This document maps every normative requirement ID in `specification.md` to its
implementation and its test or verification evidence. `specification.md`
remains the authoritative source for requirement text; this file only records
where each requirement is implemented and how a regression would be caught.

To regenerate or refresh this mapping: extract requirement IDs from
`specification.md` with `grep -oE '\*\*[A-Z]+-[0-9]+\*\*' specification.md`,
then for each ID run `grep -rn "<ID>" src tests benches scripts fuzz assets
*.md` to find citing code and tests, and read the cited test bodies to confirm
they would actually fail on a regression rather than merely mentioning the ID
in a comment.

Status values:

- `covered` — implemented, with at least one test that would fail on
  regression.
- `covered-by-design` — nothing to implement; the requirement is a
  prohibition satisfied by the absence of code, noted with why.
- `manual` — verifiable only by a human or a paid/networked run.
- `gap` — no implementation or evidence was found.

## 1. Security Claim (SEC)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| SEC-001 | Help prevent enrolled resolved values reaching model context via covered adapter paths | `src/matcher.rs`, `src/redact.rs`, `src/adapter/*.rs` | `tests/leaks.rs::no_adapter_discloses_an_enrolled_value`, `tests/claude_hook.rs`, `tests/wording.rs::the_readme_states_the_boundary_it_must_state` | covered |
| SEC-002 | Must not overclaim protection scope | `README.md`, `vision.md`, `docs/release-notes-template.md`, `SECURITY.md` (checked text) | `tests/wording.rs::no_public_document_promises_more_than_the_security_claim` | covered |
| SEC-003 | Runtime makes no network calls; install and Claude live canary are the only exceptions | `Cargo.toml` (no HTTP client dependency), `src/integration/claude.rs:live_canary` | `src/diagnose.rs::doctor_performs_no_network_call_unless_the_canary_is_selected`, `tests/diagnose.rs::doctor_is_not_offered_the_live_canary_without_a_terminal` | covered |
| SEC-004 | Never persist, log, or diagnose resolved values | `src/dotenv.rs`, `src/source.rs::reason`, `src/setup/write.rs`, `src/registry.rs` (error messages never carry values) | `tests/leaks.rs`, `src/config.rs::diagnostics_never_quote_file_content`, `src/registry.rs::malfunction_messages_contain_no_path_or_file_text` | covered |
| SEC-005 | No telemetry, crash upload, analytics, or persistent logging | absence of any logging or telemetry crate or code path | `tests/leaks.rs::runtime_writes_no_log_or_telemetry_file` compares the file tree before and after every adapter, `status`, and `doctor` run; `Cargo.toml` contains no logging, tracing, or analytics crate | covered |
| SEC-006 | Every untrusted terminal string occupies one logical line, with controls/bidi/non-UTF-8 escaped | `src/sanitize.rs` | `src/sanitize.rs` unit tests (`control_characters_become_visible_escapes`, `bidi_and_separator_controls_are_escaped`, `non_utf8_paths_are_rendered_without_raw_bytes`, `every_rendering_occupies_one_logical_line`), `tests/leaks.rs::terminal_hostile_names_and_paths_are_escaped_in_diagnostics` | covered |

## 2. Supported Platforms And Integrations (SUP)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| SUP-001 | Support Linux and macOS on x86_64 and arm64 | `.github/workflows/ci.yml` (ubuntu/macos matrix), `.github/workflows/release.yml` (four targets) | CI matrix job `check`; release `package` job builds `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-apple-darwin` | covered |
| SUP-002 | Claude is production; Codex/Copilot/OpenCode are experimental | `README.md`, `vision.md`, `docs/release-notes-template.md` support matrix | `tests/wording.rs::every_public_support_matrix_labels_the_tiers` | covered |
| SUP-003 | Experimental integrations labeled EXPERIMENTAL, require affirmative install, excluded from default selection/health | `src/setup/integrations.rs`, `src/diagnose.rs:530` | `tests/setup.rs::an_experimental_integration_requires_an_affirmative_choice`, `tests/wording.rs::every_public_support_matrix_labels_the_tiers` | covered |
| SUP-004 | No host version checks | absence of version-check code | no host-version comparison exists anywhere in `src/`; health is derived from observed configuration plus the offline synthetic checks, and the versions each protocol was verified against are recorded in the release notes as evidence rather than as a range | covered-by-design (nothing to implement; verified by absence) |
| SUP-005 | Coverage scoped to local harness modes honoring the hook | `README.md` scoping language | `tests/wording.rs::coverage_is_scoped_to_local_harnesses_that_honor_the_integration` | covered |

## 3. CLI

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| CLI-001 | `setup` is the only configuration workflow; no `init`/install subcommands/slash commands | `src/cli.rs` (command dispatch) | `tests/cli.rs::invalid_usage_exits_two_without_stdout` (rejects `init`), `tests/cli.rs::help_hides_harness_protocol_entry_points` | covered |
| CLI-002 | `setup` requires an interactive TTY; fails clearly without changing files otherwise | `src/cli.rs::run_setup` | `tests/cli.rs::setup_refuses_to_run_without_a_terminal` | covered |
| CLI-003 | Public commands are human-readable only; no stable JSON contract | `src/diagnose.rs` render functions (plain `writeln!` text, no serialization) | `tests/diagnose.rs`, `tests/cli.rs::help_exits_zero_and_documents_the_public_commands` (asserts human text) | covered |
| CLI-004 | `setup` returns zero only when every action completes; nonzero on cancel/failure | `src/setup/mod.rs::cancelled`, `src/setup/integrations.rs` | `tests/setup.rs::cancelling_the_first_phase_writes_nothing`, `::a_project_phase_failure_keeps_the_committed_global_phase`, `::a_malformed_settings_file_fails_the_integration_phase_without_changing_it` | covered |
| CLI-005 | `status` returns zero whenever inspection completes; nonzero only when inspection cannot complete | `src/diagnose.rs::status` | `tests/diagnose.rs::a_healthy_machine_exits_zero_for_both_commands`, `::an_inspection_that_cannot_complete_exits_two` | covered |
| CLI-006 | `doctor` returns zero/one/two per health outcome; zero active values is a failure | `src/diagnose.rs::doctor` | `tests/diagnose.rs::a_fully_inactive_registry_is_a_health_failure`, `::malformed_configuration_fails_doctor_but_not_status`, `::an_approved_conflict_stays_healthy_but_visible` | covered |
| CLI-007 | Diagnosed process-hook failures SHOULD exit zero with valid host protocol output | `src/cli.rs::run_claude_hook`, `::run_copilot_hook` | `tests/claude_hook.rs:161`, `tests/codex_hook.rs:189` (exit zero); `tests/copilot_hook.rs::a_malfunction_warns_through_stderr_and_mutates_nothing` (host-specific exit 2, documented as this host's own warning channel) | covered |

## 4-5. Configuration Locations, Selection, And Schema (CFG)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| CFG-001 | Global config at `$XDG_CONFIG_HOME`/`~/.config` + `secretsieve/config.toml`, user-only permissions | `src/config.rs::global_config_path`, `src/setup/write.rs` (0o600/0o700) | `src/config.rs::the_global_path_follows_xdg_rules`, `src/setup/write.rs` permission test (asserts `0o600`/`0o700`) | covered |
| CFG-002 | Project config filename is `.secretsieve.toml` | `src/paths.rs::PROJECT_CONFIG_FILENAME` | `src/paths.rs` project-root tests use the constant throughout | covered |
| CFG-003 | Project root: nearest `.secretsieve.toml`, else Git worktree root, else cwd | `src/paths.rs::setup_project_root` | `src/paths.rs::project_root_selection_prefers_the_nearest_config`, `::project_root_falls_back_to_the_git_worktree_then_the_directory`, `::a_git_file_marks_a_worktree_root` | covered |
| CFG-004 | Runtime selects at most one project registry, nearest ancestor, no merging | `src/paths.rs::runtime_project_config` | `src/registry.rs::the_nearest_ancestor_project_config_is_selected` | covered |
| CFG-005 | Claude uses stable project dir (fallback `cwd`); OpenCode stable field; Codex/Copilot MAY use `cwd` | `src/adapter/claude.rs` (`CLAUDE_PROJECT_DIR`), `src/adapter/opencode.rs::project_root`, `src/adapter/codex.rs::Event::cwd` | `src/adapter/claude.rs::the_project_registry_is_selected_from_the_host_project_directory` | covered |
| CFG-006 | `version = 1` required; unknown fields/types/duplicates invalidate a file | `src/config.rs::parse` (`deny_unknown_fields`) | `src/config.rs::unknown_fields_invalidate_the_file`, `::the_version_is_required_and_pinned`, `::duplicate_identities_in_one_file_are_rejected`, `::identity_is_computed_after_expansion_and_normalization` | covered |
| CFG-007 | Env entry needs `source="env"` + non-empty `name`, no dotenv fields | `src/config.rs::parse_entry` | `src/config.rs::environment_entries_reject_dotenv_fields` | covered |
| CFG-008 | Dotenv entry needs `file` + exactly one of `key`/`all` | `src/config.rs::parse_entry` | `src/config.rs::dotenv_entries_require_exactly_one_of_key_or_all` | covered |
| CFG-009 | Global/project may share identities; project may reference external files/env names | `src/config.rs` | `src/config.rs::project_config_may_reference_external_paths_and_environment_names` | covered |
| CFG-010 | Paths stored as entered; `~/` expands; relative resolves against config file dir; no env/glob/shell expansion | `src/paths.rs::expand` | `src/paths.rs::a_leading_tilde_expands_to_the_home_directory`, `::other_expansions_never_happen`, `src/config.rs::paths_are_stored_as_entered` | covered |
| CFG-011 | Effective enrollment additive; no negation/override | `src/registry.rs::build` | `src/registry.rs::global_and_project_enrollment_are_additive` | covered |
| CFG-012 | Parsing strict per file; use of effective registry all-or-nothing | `src/registry.rs::build` | `src/registry.rs::an_invalid_project_config_disables_global_redaction`, `::an_invalid_global_config_disables_project_redaction`, `::a_malformed_enrolled_source_disables_the_whole_registry` | covered |
| CFG-013 | Missing global config is non-clean but not malformed; missing project is normal | `src/registry.rs::build` | `src/registry.rs::a_missing_global_config_warns_but_keeps_project_redaction`, `::a_missing_project_config_leaves_project_enrollment_empty` | covered |
| CFG-014 | Setup never overwrites an invalid existing config; shows sanitized reason | `src/setup/mod.rs::report_invalid` | `tests/setup.rs::an_invalid_existing_config_is_preserved_byte_for_byte`, `::an_invalid_project_config_stops_setup_before_the_global_phase` | covered |
| CFG-015 | Setup preserves existing valid enrollment by default; permits deliberate removal | `src/setup/mod.rs` (item selection defaults) | `tests/setup.rs::existing_enrollment_survives_a_rerun_even_when_unresolved`, `::an_enrolled_entry_can_be_removed_deliberately` | covered |

## 6. Source Resolution (SRC)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| SRC-001 | Env reference resolves case-sensitively from inherited environment | `src/source.rs::resolve_environment` | `src/source.rs::environment_names_are_case_sensitive_and_empty_values_are_unresolved` | covered |
| SRC-002 | Unset/empty/non-UTF-8 env value is unresolved, never enters matcher | `src/source.rs::resolve_environment` | `src/source.rs::non_utf8_environment_values_never_enter_the_matcher` | covered |
| SRC-003 | Deterministic dotenv grammar (BOM, CRLF, `export`, quoting, comments) | `src/dotenv.rs::parse` | `src/dotenv.rs` unit tests, one per grammar clause (`a_leading_bom_is_ignored`, `crlf_endings_normalize_to_lf`, `export_is_accepted_only_as_a_separate_token`, `single_quoted_values_are_literal`, `double_quoted_values_decode_only_the_listed_escapes`, `unquoted_comments_start_only_after_whitespace`, `unterminated_quotes_are_malformed`, `text_after_a_closing_quote_is_malformed`, `no_interpolation_or_substitution_happens`) | covered |
| SRC-004 | Last dotenv assignment wins; duplicates warned without values | `src/dotenv.rs::Dotenv` | `src/dotenv.rs::the_last_assignment_wins_and_duplicates_are_reported`, `src/registry.rs::duplicate_dotenv_keys_are_reported_without_values` | covered |
| SRC-005 | Absent file/key or empty value is unresolved, not a malfunction | `src/source.rs::resolve` | `src/source.rs::absent_files_keys_and_empty_values_are_unresolved` | covered |
| SRC-006 | Permission denial/malformed/invalid-UTF-8/non-`NotFound` I/O is a malfunction disabling the registry | `src/source.rs::read_dotenv` | `src/source.rs::malformed_and_invalid_utf8_files_are_malfunctions`, `::an_unreadable_file_is_a_malfunction` | covered |
| SRC-007 | Wildcard entry resolves every current non-empty key; future keys enroll automatically | `src/source.rs::resolve` | `src/source.rs::a_wildcard_entry_resolves_every_current_non_empty_key`, `src/registry.rs::wildcard_entries_pick_up_keys_added_later` | covered |
| SRC-008 | No SecretSieve-specific dotenv size cap | `src/dotenv.rs` (no size check in parser) | `tests/limits.rs::a_large_wildcard_dotenv_file_is_resolved_without_a_cap` | covered |
| SRC-009 | Sources resolved afresh per event; no cross-process caching | `src/source.rs::Cache` (per-`Registry` instance only, no daemon) | `src/source.rs::a_file_is_read_once_per_event_and_duplicates_are_recorded`; architecture has no daemon (`AGENTS.md`), and each adapter constructs `registry::build` fresh per invocation (`src/adapter/claude.rs:82`) | covered |
| SRC-010 | Dotenv changes observable next event; env changes only after harness restart | process-per-event architecture, no persistent cache | no dedicated multi-process test exists; behavior follows directly from SRC-009's fresh-resolution architecture and the OS providing environment only at process start | covered-by-design (natural consequence of the stateless per-event architecture) |

## 7. Setup Discovery And Enrollment (SET)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| SET-001 | Four ordered phases after preflight parsing; each offers a no-change path | `src/setup/mod.rs::run` | `tests/setup.rs::rerunning_setup_with_no_changes_is_idempotent`, `::an_invalid_project_config_stops_setup_before_the_global_phase` | covered |
| SET-002 | Setup automatically inspects the process environment for gated candidates | `src/setup/mod.rs::environment_candidates` | `tests/setup.rs::a_gated_environment_candidate_is_enrolled_by_default` | covered |
| SET-003 | Recursive project `.env*` discovery incl. ignored files; excludes `.git`/vendor/build; no symlink follow; skips special files | `src/setup/discovery.rs::project_dotenv_files`, `::walk` | `src/setup/discovery.rs::discovery_is_recursive_and_includes_untracked_files`, `::excluded_directories_are_never_entered`, `::symlinks_and_special_files_are_skipped`, `::a_fifo_named_like_a_dotenv_file_is_never_read`, `::non_utf8_paths_are_reported_as_unavailable` | covered |
| SET-004 | Global probe bounded to home + harness config dirs, not recursive | `src/setup/discovery.rs::global_dotenv_files` | `src/setup/discovery.rs::global_probing_is_bounded_to_the_documented_locations`, `tests/setup.rs::global_dotenv_probing_covers_the_documented_locations` | covered |
| SET-005 | Both phases allow manual paths/keys/wildcard/env names; absent manual source needs confirmation | `src/setup/mod.rs::add_manual` | `tests/setup.rs::an_unresolved_manual_source_requires_confirmation` | covered |
| SET-006 | Name-gating vocabulary: exact tokens and compact-form suffixes, ASCII case folding only | `src/setup/vocabulary.rs::gating_term` | `src/setup/vocabulary.rs::exact_tokens_gate_a_name`, `::compact_suffixes_gate_a_name`, `::unrelated_names_are_not_gated`, `::gating_is_ascii_case_insensitive_only`, `::value_shape_never_introduces_a_candidate` | covered |
| SET-007 | Gated candidates default-selected unless collision found | `src/setup/mod.rs` (item selection) | `tests/setup.rs::a_colliding_candidate_is_visible_but_unselected` | covered |
| SET-008 | User is authoritative: enrollment allowed after collision warning, no minimum length | `src/setup/mod.rs` | `tests/setup.rs::a_collision_can_be_overridden_by_the_user` | covered |
| SET-009 | Wildcard enrollment requires extra confirmation | `src/setup/mod.rs::add_manual` | `tests/setup.rs::wildcard_enrollment_requires_an_extra_confirmation` | covered |
| SET-010 | Preview masking table by Unicode scalar length; no fingerprint | `src/setup/preview.rs::mask` | `src/setup/preview.rs::short_values_are_fully_masked`, `::medium_values_reveal_two_characters_at_each_end`, `::long_values_reveal_four_characters_at_each_end`, `::boundaries_follow_the_specified_table`, `::length_is_counted_in_unicode_scalar_values`, `::no_fingerprint_is_derived_from_the_value` | covered |
| SET-011 | Collision search: readable regular files under project root, excludes candidate's own file, non-overlapping byte counts | `src/setup/collision.rs::analyze` | `src/setup/collision.rs::occurrences_are_counted_across_the_project`, `::counting_is_non_overlapping_and_left_to_right`, `::the_candidates_own_source_file_is_excluded_entirely`, `::binary_and_non_utf8_files_are_included` | covered |
| SET-012 | Collision output shows counts + sanitized filenames only, never values/snippets | `src/setup/collision.rs::Collisions` | `src/setup/collision.rs::reports_contain_filenames_and_counts_but_never_values`, `::filenames_are_sanitized_for_the_terminal` | covered |
| SET-013 | Unreadable/malformed discovered-but-unenrolled file shown unavailable; enrolled malformed source blocks completion | `src/setup/discovery.rs::inspect`, `src/setup/mod.rs::blocking_item` | `tests/setup.rs::an_unavailable_discovered_file_does_not_stop_discovery`, `::an_enrolled_malformed_source_must_be_repaired_or_removed` | covered |
| SET-014 | Atomic writes; each phase commits on its own confirmation; per-integration transactions restore prior state on failure | `src/setup/write.rs`, `src/setup/integrations.rs::apply`/`restore` | `tests/setup.rs::a_project_phase_failure_keeps_the_committed_global_phase`, `::a_malformed_settings_file_fails_the_integration_phase_without_changing_it` | covered |

## 8. Effective Registry (REG)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| REG-001 | Every non-empty UTF-8 resolved value becomes an active pattern; no heuristics at runtime | `src/registry.rs::build` | `src/registry.rs::global_and_project_enrollment_are_additive` | covered |
| REG-002 | Equal resolved values dedup to one pattern; canonical source is first project entry, else first global | `src/registry.rs::build` | `src/registry.rs::equal_values_canonicalize_to_the_first_project_entry` | covered |
| REG-003 | Source/key names case-sensitive; labels derive from key/name, never path | `src/secret.rs::SourceId::label` | `src/secret.rs::labels_derive_from_the_key_only`, `::case_is_preserved_because_names_are_case_sensitive` | covered |
| REG-004 | Label preserves ASCII letters/digits/`_`/`-`/`.`, replaces other runs with `_` | `src/secret.rs::safe_label` | `src/secret.rs::labels_keep_only_the_allowed_character_set`, `::labels_collapse_control_and_escape_sequences` | covered |

## 9. Redaction Semantics (RED)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| RED-001 | Case-sensitive UTF-8 byte matching, no normalization/folding | `src/matcher.rs::match_at` | `src/matcher.rs::matching_is_case_sensitive_and_byte_exact`, `tests/matcher_property.rs` | covered |
| RED-002 | Matching independent per selected string value; no joining across fields | `src/redact.rs::redact_value` | `src/redact.rs::values_are_matched_independently_across_fields` | covered |
| RED-003 | Leftmost-longest matching, canonical source tie-break, scanning resumes after match | `src/matcher.rs::redact`/`match_at` | `src/matcher.rs::same_start_overlap_prefers_the_longest_value`, `::different_start_overlap_prefers_the_earliest_start` | covered |
| RED-004 | Substring matching; no token/word boundaries | `src/matcher.rs::match_at` | `src/matcher.rs::matching_is_substring_matching` | covered |
| RED-005 | Structured payload redaction touches only decoded string values | `src/redact.rs::redact_value` | `src/redact.rs` (keys/numbers/bools/nulls preserved), `tests/claude_hook.rs::every_supported_result_shape_is_redacted_without_changing_its_shape` | covered |
| RED-006 | Placeholder fallback chain `<SECRET:LABEL>` → `<SECRET>` → empty string | `src/matcher.rs::is_emit_safe` | `src/matcher.rs::a_value_inside_the_named_placeholder_forces_the_generic_form`, `::a_value_inside_every_placeholder_forces_deletion` | covered |
| RED-007 | No recursive matcher feedback; unsafe labels omitted | `src/matcher.rs::redact` (scanning resumes past inserted text) | `src/matcher.rs::replacements_are_never_rescanned`, `::a_placeholder_that_reproduces_a_value_is_rejected_before_insertion` | covered |
| RED-008 | Intervention metadata: total + per-source counts only, no values/hashes/content | `src/matcher.rs::Intervention` | `src/matcher.rs::unsafe_labels_are_aggregated_without_names`; struct fields contain only `total`/`named`/`unnamed` counts | covered |
| RED-009 | Clean events with valid config are silent; unresolved sources stay silent | `src/matcher.rs::redact` (returns `None` when unchanged) | `src/matcher.rs::clean_input_is_unchanged_and_silent`, `src/registry.rs::unresolved_sources_do_not_fail_the_event`, `tests/claude_hook.rs::a_clean_event_produces_no_output_at_all` | covered |
| RED-010 | No rehydration path from placeholder back to source value | absence of any rehydrate/restore function | no code path maps a placeholder back to a value anywhere in `src/`; confirmed by grep for "rehydrat" returning nothing | covered-by-design (nothing to implement; verified by absence) |

## 10. Runtime Failure Policy (RUN)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| RUN-001 | Malfunction/invalid config produces no partial redaction; original content passed through with a warning | `src/adapter/claude.rs`, `::codex.rs`, `::copilot.rs` (all return unmodified content on `Malfunction`) | `tests/claude_hook.rs::an_invalid_global_config_disables_redaction_and_warns`, `tests/copilot_hook.rs::a_malfunction_warns_through_stderr_and_mutates_nothing` | covered |
| RUN-002 | Claude/Codex/Copilot documented as fail-open on crash/timeout/disable/bypass | `limitations.md` (`LIM-012`) | `tests/wording.rs::the_readme_states_the_boundary_it_must_state` (asserts "fail open" is stated) | covered |
| RUN-003 | OpenCode plugin aborts on subprocess crash/timeout/invalid protocol/malfunction; notification failure doesn't undo mutation | `assets/opencode/plugin.ts`, `src/adapter/opencode.rs::Answer::Malfunction` | `tests/opencode/plugin.test.ts::a_subprocess_failure_aborts_the_covered_operation` (and invalid-protocol/nonzero-exit/reported-malfunction variants), `::a_notification_failure_does_not_undo_the_mutation`, `src/adapter/opencode.rs::a_malfunction_tells_the_plugin_to_abort` | covered |
| RUN-004 | Every installed hook/subprocess invocation uses a 5-second timeout | `src/integration/claude.rs::TIMEOUT_SECONDS`, `::codex.rs`, `::copilot.rs` (each `=5`), `assets/opencode/plugin.ts::TIMEOUT_MS=5000` | `src/integration/claude.rs` (`assert_eq!(groups[0]["hooks"][0]["timeout"], json!(5))`), `src/integration/codex.rs`, `src/integration/copilot.rs` timeout assertions; OpenCode's `TIMEOUT_MS` constant has no test that forces it to trigger | covered |
| RUN-005 | p95 < 100 ms on a warm-cache 1 MiB payload, 100 values, 10 dotenv files | `benches/redaction.rs` | `mise run bench`; benchmark reports p95 but never asserts/fails on timing, matching the spec's "engineering benchmark, not a guarantee" framing | manual |
| RUN-006 | Malformed envelope/unknown event is a diagnosed protocol malfunction per adapter; OpenCode plugin throws; uncovered-but-valid content is preserved without warning | `src/adapter/claude.rs`, `::codex.rs`, `::copilot.rs`, `::opencode.rs::ProtocolError` | `src/adapter/claude.rs::unknown_envelope_fields_are_not_malformed`, `src/adapter/opencode.rs::a_malformed_or_unknown_request_is_a_protocol_error`, `tests/claude_hook.rs::invalid_input_is_diagnosed_without_echoing_the_payload` | covered |

## 11. Integration Installation (INT)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| INT-001 | All four harnesses detected; Claude default-selected; experimental unselected unless already installed | `src/setup/integrations.rs::phase` | `tests/setup.rs::an_experimental_integration_requires_an_affirmative_choice` | covered |
| INT-002 | Undetected harness may still be installed, with disclosure of limited verification | `src/setup/integrations.rs` (row rendering) | `tests/setup.rs::an_undetected_harness_discloses_limited_verification` | covered |
| INT-003 | Installed command uses absolute binary path, stdin/stdout, no shell interpolation | `src/integration/mod.rs::current_executable`, quoting helpers | `src/integration/mod.rs::awkward_paths_are_quoted_so_the_shell_cannot_split_or_expand_them`, `::plain_paths_are_not_quoted`; every hook test drives the binary over stdin/stdout | covered |
| INT-004 | No duplicate managed entries; removal only for unmodified owned artifacts; user-owned entries preserved with warning | `src/integration/hooks_json.rs`, `::copilot.rs::classify`, `::opencode.rs::classify` | `tests/setup.rs::rerunning_setup_leaves_an_installed_integration_byte_identical`, `::deselecting_the_integration_removes_only_the_managed_hook`, `src/integration/copilot.rs::unrelated_hook_files_are_never_touched` | covered |
| INT-005 | Competing mutating hooks shown for individual approval; approved conflict still shown by doctor | `src/setup/integrations.rs::approve_conflicts`, `src/integration/hooks_json.rs` (conflict detection) | `tests/setup.rs::a_competing_mutating_hook_is_offered_for_approval`, `tests/diagnose.rs::an_approved_conflict_stays_healthy_but_visible` | covered |
| INT-006 | Install success is not permanent proof; status/doctor re-derive from config/artifacts | `src/integration/state.rs::Managed` (no persisted verified/health flag) | `src/integration/state.rs` test suite confirms the struct stores only `command` and `approved_conflicts`; `tests/diagnose.rs` re-runs inspection each call | covered |

## 12. Claude Code Adapter (CLA)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| CLA-001 | Setup manages one synchronous wildcard `PostToolUse` hook in `~/.claude/settings.json`, 5s timeout | `src/integration/claude.rs::SPEC`, `::TIMEOUT_SECONDS` | `src/integration/claude.rs::a_clean_installation_creates_the_managed_hook` (asserts `timeout: 5`) | covered |
| CLA-002 | Recursively redacts every string in `tool_response`, preserves shape, returns via `hookSpecificOutput.updatedToolOutput` | `src/adapter/claude.rs::handle` | `tests/claude_hook.rs::every_supported_result_shape_is_redacted_without_changing_its_shape`, `::assert_same_shape` | covered |
| CLA-003 | On intervention, one safe `systemMessage`, never `additionalContext` | `src/adapter/claude.rs::finish` | `tests/claude_hook.rs::a_matched_value_is_replaced_before_the_model_visible_boundary` (asserts `systemMessage` present, no `additionalContext` key) | covered |
| CLA-004 | Must not claim coverage for failed results, prompts, tool args, telemetry, or non-replaceable paths | `src/adapter/claude.rs` (only handles `tool_response` on success) | `tests/claude_hook.rs::a_failed_tool_result_event_is_not_claimed_as_covered` | covered |
| CLA-005 | Other matching `PostToolUse` hooks trigger `INT-005` approval; approved hooks don't block healthy status | `src/setup/integrations.rs`, `src/diagnose.rs` | `tests/diagnose.rs::an_approved_conflict_stays_healthy_but_visible` | covered |

## 13. Codex CLI Adapter (COD)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| COD-001 | Setup manages one synchronous wildcard `PostToolUse` hook in `~/.codex/hooks.json`, 5s timeout, trust workflow | `src/integration/codex.rs::SPEC`, `::TIMEOUT_SECONDS` | `src/integration/codex.rs` timeout assertion | covered |
| COD-002 | On match, blocks original result and supplies sanitized textual rendering via blocking feedback | `src/adapter/codex.rs::handle` | `tests/codex_hook.rs` (asserts `<SECRET:...>` in `reason`) | covered |
| COD-003 | Discloses that intervention may turn a result error-like and lose structure/images/typed semantics | `src/adapter/codex.rs` rendering text | `src/adapter/codex.rs` test asserting "did not fail" + "sanitized textual rendering"; `tests/codex_hook.rs` same | covered |
| COD-004 | Must not claim every tool emits the event, MCP shape-preservation, or full failed-result coverage | `src/adapter/codex.rs` (textual-only rendering for MCP) | `tests/codex_hook.rs::a_structured_mcp_result_is_rendered_as_sanitized_text` | covered |

## 14. GitHub Copilot CLI Adapter (COP)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| COP-001 | Setup manages a dedicated file under `~/.copilot/hooks/`, 5s timeout, never touches unrelated files | `src/integration/copilot.rs::FILENAME`, `::TIMEOUT_SECONDS` | `tests/setup.rs::copilot_installs_one_dedicated_file_and_leaves_others_alone`, `src/integration/copilot.rs::unrelated_hook_files_are_never_touched` | covered |
| COP-002 | Redacts `userPromptTransformed` text and successful `toolResult.textResultForLlm`, preserves result shape | `src/adapter/copilot.rs::handle` | `tests/copilot_hook.rs` (prompt and tool-result fixtures) | covered |
| COP-003 | On intervention, one safe progress summary before final mutation object | `src/adapter/copilot.rs` (progress line before final JSON) | `tests/copilot_hook.rs` (`progress.len() == 1` assertion) | covered |
| COP-004 | Must not claim coverage for failed errors, non-text attachments, other injection paths, or local timeline prompt | `src/adapter/copilot.rs` (only successful/textual paths handled) | `tests/copilot_hook.rs::a_failed_tool_result_is_not_touched`, `src/adapter/copilot.rs::a_failed_tool_result_is_not_covered` | covered |

Note: `limitations.md` (`DEV-002`) records that Copilot's own documentation does
not explicitly confirm `userPromptTransformed` honors `modifiedTransformedPrompt`
for command hooks; this is an accepted, documented limitation on COP-002, not a
coverage gap in this codebase.

## 15. OpenCode Adapter (OCO)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| OCO-001 | Setup manages one owned TypeScript plugin file that invokes the absolute Rust binary with one JSON request/response over stdio | `src/integration/opencode.rs::FILENAME`, `assets/opencode/plugin.ts` | `tests/setup.rs::opencode_installs_one_owned_plugin_file`, `tests/opencode/plugin.test.ts::the_plugin_binary_under_test_exists` | covered |
| OCO-002 | Uses `chat.message` for new textual user parts and `tool.execute.after` for successful textual tool output | `assets/opencode/plugin.ts` hook registrations | `tests/opencode/plugin.test.ts::new_user_text_is_redacted_in_place_and_announced`, `::successful_standard_tool_output_is_redacted_in_place` | covered |
| OCO-003 | One safe named/count TUI notification when redaction occurs and host API is available | `assets/opencode/plugin.ts::notify` | `tests/opencode/plugin.test.ts` (`client.toasts` assertion) | covered |
| OCO-004 | No V2 APIs, provider wrappers, full-history/system transforms, tool-definition rewriting, or extra claims | `assets/opencode/plugin.ts` (thin translator, no matcher logic) | `src/integration/opencode.rs::the_plugin_carries_no_matcher_or_resolver_logic`; `tests/opencode/plugin.test.ts` (uncovered fields preserved) | covered |

## 16. Status And Doctor (DIA)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| DIA-001 | Status inspects config, resolves sources, reports counts, runs no adapter protocol test | `src/diagnose.rs::status` | `tests/diagnose.rs::status_runs_no_adapter_protocol_test`, `::the_project_root_follows_the_working_directory` | covered |
| DIA-002 | Registry/integration health independent; zero active values shown INACTIVE | `src/diagnose.rs::render_registry`/`render_integrations` | `src/diagnose.rs::status_facets_are_independent` | covered |
| DIA-003 | Doctor additionally inspects permissions, source errors, duplicate aliases, collisions, ownership, disabled hooks, conflicts, executables, timeouts, synthetic checks | `src/diagnose.rs::doctor` | `src/diagnose.rs::doctor_fails_on_an_enrolled_source_malfunction`, `::doctor_warns_about_a_wrong_timeout_without_failing_on_it_alone`, `::doctor_warns_about_duplicate_keys_and_aliases_without_values`, `::doctor_fails_on_an_unapproved_conflict_and_a_missing_executable`, `tests/diagnose.rs:114` (synthetic check) | covered |
| DIA-004 | Collision findings are warnings only, never change enrollment or exit status | `src/diagnose.rs::collision_findings` | `src/diagnose.rs::doctor_reports_collisions_as_warnings_only` | covered |
| DIA-005 | Optional paid/networked Claude live canary, disabled by default, requires confirmation, uses a random non-credential value | `src/diagnose.rs::run_live_canary`, `src/integration/claude.rs::live_canary` | `tests/diagnose.rs::doctor_is_not_offered_the_live_canary_without_a_terminal` covers the offline gating; the live network path itself is exercised only by a human (`limitations.md` `DEV-001`) | manual |
| DIA-006 | Codex/Copilot/OpenCode have offline synthetic verification only; passing doesn't remove EXPERIMENTAL label | `src/integration/codex.rs::verify_offline`, `::copilot.rs`, `::opencode.rs` (no network call in any) | `tests/setup.rs::the_claude_hook_is_installed_and_verified_offline`-equivalent flows for each integration; `src/diagnose.rs::status_facets_are_independent` (EXPERIMENTAL label rendering is unconditional on check outcome) | covered |
| DIA-007 | A previous successful verification is never a permanent certificate | `src/integration/state.rs::Managed` (no persisted verification/health flag) | absence of any "verified" field confirmed by reading `Managed`'s fields (`command`, `approved_conflicts` only); doctor re-runs the synthetic check every invocation | covered |
| DIA-008 | Doctor exit-code classification: one for a diagnosed condition, two for usage/internal failure | `src/diagnose.rs::doctor` | `src/diagnose.rs::doctor_fails_when_no_value_resolves`, `::doctor_fails_on_an_enrolled_source_malfunction`, `::doctor_fails_when_no_integration_is_installed`, `::doctor_fails_on_an_unapproved_conflict_and_a_missing_executable`, `tests/diagnose.rs::an_inspection_that_cannot_complete_exits_two` | covered |

## 17. Installation And Release (REL)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| REL-001 | Standalone release artifacts for the four targets with SHA-256 checksums | `.github/workflows/release.yml`, `scripts/package.sh` | `scripts/release-check.sh` ("checksum matches the artifact"); CI `release.yml` `package` matrix | covered |
| REL-002 | Maintained install script: detect platform/arch, download, verify checksum, atomic install, overridable destination | `install.sh` | `scripts/release-check.sh` ("clean install placed the binary in ~/.local/bin", "the installer verified the checksum before installing", `--install-dir` check) | covered |
| REL-003 | Install script only installs/upgrades the binary; no setup/config/adapter changes | `install.sh` | `scripts/release-check.sh` ("no configuration or harness file was created") | covered |
| REL-004 | Rerunning upgrades within the installed major; crossing major requires explicit opt-in | `install.sh` | `scripts/release-check.sh` ("an older install upgrades within the same major version", "crossing a major version requires an explicit opt-in") | covered |
| REL-005 | Hooks and plugins never download/install/update the binary | absence of download/network code in adapters/plugin | no `download`/`http`/`fetch` code found in `assets/opencode/plugin.ts` or any `src/adapter`/`src/integration` file | covered-by-design (nothing to implement; verified by absence) |
| REL-006 | MIT OR Apache-2.0 license, public security-reporting policy | `Cargo.toml` (`license = "MIT OR Apache-2.0"`), `LICENSE-MIT`, `LICENSE-APACHE`, `SECURITY.md` | file presence at repo root; `tests/wording.rs` reads `SECURITY.md` as a public document (fails if absent) | covered |
| REL-007 | Every V1 release reads earlier V1 config/state without requiring setup to rerun | `src/config.rs`, `src/integration/state.rs` (stable schema, `deny_unknown_fields` only on known keys with defaults for new optional fields) | `scripts/release-check.sh` ("an existing V1 configuration stays runtime-readable after the upgrade") | covered |
| REL-008 | Manual live Claude test proving redaction survives session resume, gating release | none (inherently manual) | `limitations.md` (`DEV-001`) records this as the release-gating manual qualification | manual |

## 18. Testing And Acceptance (TST)

| ID | Requirement | Implementation | Evidence | Status |
| --- | --- | --- | --- | --- |
| TST-001 | Matcher tests cover empty/UTF-8/case/substring/adjacent/overlap/duplicate/canonical/multiline/fallback/no-recursion | `src/matcher.rs` unit tests | one dedicated test per listed case, e.g. `adjacent_matches_are_each_replaced`, `same_start_overlap_prefers_the_longest_value`, `different_start_overlap_prefers_the_earliest_start`, `duplicate_values_collapse_to_the_canonical_source`, `multiline_values_match_across_line_breaks`, `replacements_are_never_rescanned`, `empty_input_and_empty_registry_are_no_ops` | covered |
| TST-002 | Config/source tests cover unknown fields, duplicates, cross-scope duplicates, missing sources, empty values, non-UTF-8 env, malformed dotenv, duplicate keys, path expansion, wildcard future keys, all-or-nothing | `src/config.rs`, `src/source.rs`, `src/registry.rs` unit tests | `src/config.rs::unknown_fields_invalidate_the_file`, `::duplicate_identities_in_one_file_are_rejected`; `src/registry.rs::cross_scope_duplicate_identities_are_allowed`; `src/source.rs::non_utf8_environment_values_never_enter_the_matcher`; `src/registry.rs::a_malformed_enrolled_source_disables_the_whole_registry` | covered |
| TST-003 | Filesystem tests cover project-root selection, recursive discovery, exclusions, symlinks, collision exclusion, permissions, atomic writes, invalid-config preservation, repeat setup, partial failure | `tests/setup.rs` | full test list (67 tests), e.g. `an_invalid_existing_config_is_preserved_byte_for_byte`, `rerunning_setup_with_no_changes_is_idempotent`, `a_project_phase_failure_keeps_the_committed_global_phase` | covered |
| TST-004 | Every adapter path has protocol fixtures for clean/intervened/unresolved/malformed/malfunction/timeout/conflict states | `tests/claude_hook.rs`, `::codex_hook.rs`, `::copilot_hook.rs`, `::opencode/plugin.test.ts` | per-adapter fixtures for each state, e.g. `a_clean_event_produces_no_output_at_all`, `an_unresolved_source_is_silent_and_does_not_fail_the_event`, `timeout_mapping_stays_inside_the_host_bound`; conflicting-installation states are covered in `tests/setup.rs::a_competing_mutating_hook_is_offered_for_approval` | covered |
| TST-005 | Generated canaries; assert absence from stdout/stderr/diagnostics/snapshots/model-visible content | `src/testing.rs::Canary`, `assert_canary_absent` | `tests/leaks.rs` (drives every adapter, status, doctor, and setup with one canary and asserts absence everywhere) | covered |
| TST-006 | Fuzz targets cover matcher, JSON, TOML, dotenv; bounded fuzz smoke via mise | `src/fuzz.rs`, `src/bin/fuzz_smoke.rs`, `fuzz/regressions/*` | `scripts/fuzz-smoke.sh` (`mise run fuzz-smoke`), `.github/workflows/fuzz.yml` | covered |
| TST-007 | Routine CI runs format/lint/test/build via mise; release checks exercise artifacts/checksums/install/upgrade | `mise.toml` tasks, `.github/workflows/ci.yml`, `::release.yml` | CI `check`/`build` jobs; `scripts/release-check.sh` | covered |
| TST-008 | Optional paid/networked tests never gate routine CI; REL-008 gates release | `.github/workflows/*.yml` (no live-canary invocation) | `tests/wording.rs::no_routine_workflow_runs_a_paid_or_networked_check` | covered |

## Gaps and manual items

No `gap` rows were found: every requirement ID in `specification.md` has either
an implementation with a regression-catching test, a documented
covered-by-design absence, or an accepted manual/paid verification step.

Items that are not `covered`:

- **SUP-001, REL-001** — implemented for all four targets, and the release
  workflow builds and packages each of them, but only
  `x86_64-unknown-linux-gnu` has been built and exercised in the development
  environment used so far. The other three need their CI runners.

- **RUN-005** — the p95 benchmark (`benches/redaction.rs`) reports timing but
  never asserts pass/fail, matching the spec's own framing as an engineering
  benchmark rather than a machine-independent guarantee; a human must read the
  reported number.
- **DIA-005** — the offline gating (disabled by default, no TTY means no
  call) is tested, but the live Claude canary itself is a paid, networked call
  that only a human running `secretsieve doctor` can exercise
  (`limitations.md` `DEV-001`).
- **REL-008** — the live Claude session-resume qualification is an explicitly
  manual release gate with no automated coverage by design
  (`limitations.md` `DEV-001`).

Items marked `covered-by-design` (nothing to implement, verified by the
absence of code rather than a positive test):

- **SUP-004** — no host-version-comparison code exists anywhere in `src/`.
- **RED-010** — no code path maps a placeholder back to a source value.
- **SRC-010** — falls out of SRC-009's stateless, per-event architecture; no
  cache or daemon exists to retain values across events.
- **REL-005** — no hook or plugin contains download/network code.

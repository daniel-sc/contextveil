//! Exact-value matching and placeholder selection.
//!
//! This module owns `RED-001` through `RED-008`: case-sensitive UTF-8 byte
//! matching, leftmost-longest selection, safe placeholder fallback, and
//! intervention metadata that never carries a matched value.
//!
//! Matching is deliberately literal. No name, entropy, provider-format, length,
//! or collision heuristic may influence runtime behavior (`REG-001`).
//!
//! `RED-010` is satisfied by construction: nothing here, and no other module, maps
//! a placeholder back to a source value. Replacement is one way.

use std::collections::HashMap;

use crate::secret::{ResolvedSecret, SourceId};

/// A resolved value compiled into a match pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pattern {
    value: String,
    /// Safe label of the canonical source (`REG-002`), if the label itself can
    /// be emitted without reproducing an active value.
    label: Option<String>,
    canonical: SourceId,
    /// Sources that resolved to the same value and were deduplicated.
    aliases: Vec<SourceId>,
    /// Replacement text chosen once per registry by `RED-006`.
    replacement: String,
}

/// The ordered set of active values used for one runtime event.
///
/// Construction canonicalizes duplicates, so the matcher never stores the same
/// value twice (`REG-002`).
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    patterns: Vec<Pattern>,
    /// Pattern indexes bucketed by first byte, each ordered longest first, so
    /// leftmost-longest selection is a short scan instead of a full sweep.
    by_first_byte: Vec<Vec<u32>>,
}

impl Redactor {
    /// Compiles resolved secrets into a matcher.
    ///
    /// `secrets` must already be in canonical order: project entries in file
    /// order followed by global entries in file order (`REG-002`).
    pub fn new(secrets: Vec<ResolvedSecret>) -> Self {
        let mut patterns: Vec<Pattern> = Vec::with_capacity(secrets.len());
        // Deduplication is keyed rather than scanned, so a very large wildcard
        // file stays linear in its key count (`SRC-008` allows any size).
        let mut positions: HashMap<String, usize> = HashMap::with_capacity(secrets.len());

        for secret in secrets {
            // Resolvers classify an empty value as unresolved (`SRC-002`,
            // `SRC-005`), so this should be unreachable. It is still filtered
            // here rather than asserted: an empty pattern would match at every
            // position, and a debug assertion disappears from a release build.
            if secret.value.is_empty() {
                continue;
            }
            if let Some(position) = positions.get(&secret.value) {
                patterns[*position].aliases.push(secret.source);
                continue;
            }
            positions.insert(secret.value.clone(), patterns.len());
            patterns.push(Pattern {
                value: secret.value,
                // An identity without a key has no label, so it can only ever
                // be reported as an unnamed count (`RED-008`).
                label: (!secret.label.is_empty()).then_some(secret.label),
                canonical: secret.source,
                aliases: Vec::new(),
                replacement: String::new(),
            });
        }

        let mut redactor = Self {
            patterns,
            by_first_byte: Vec::new(),
        };
        redactor.build_index();

        // A label or placeholder is emit-safe only when it cannot reproduce any
        // active value (`RED-006`, `RED-008`). The check runs through the index
        // that was just built, so it costs the length of the candidate rather
        // than the size of the registry.
        let generic_is_safe = redactor.is_emit_safe(GENERIC_PLACEHOLDER);
        let decisions: Vec<(String, Option<String>)> = redactor
            .patterns
            .iter()
            .map(|pattern| {
                let named = pattern
                    .label
                    .as_deref()
                    .map(|label| format!("<SECRET:{label}>"));
                let replacement = if let Some(named) =
                    named.filter(|candidate| redactor.is_emit_safe(candidate))
                {
                    named
                } else if generic_is_safe {
                    GENERIC_PLACEHOLDER.to_string()
                } else {
                    // `RED-006` final fallback: delete the match rather than
                    // emit anything that reproduces an active value.
                    String::new()
                };
                let safe_label = pattern
                    .label
                    .clone()
                    .filter(|label| redactor.is_emit_safe(label));
                (replacement, safe_label)
            })
            .collect();

        for (pattern, (replacement, safe_label)) in redactor.patterns.iter_mut().zip(decisions) {
            pattern.replacement = replacement;
            pattern.label = safe_label;
        }
        redactor
    }

    /// Buckets pattern indexes by first byte, longest first.
    ///
    /// The first hit at a position is then the leftmost-longest match
    /// (`RED-003`), and registry order breaks a length tie so the canonical
    /// source wins (`REG-002`).
    fn build_index(&mut self) {
        let mut by_first_byte = vec![Vec::new(); 256];
        let mut order: Vec<u32> = (0..self.patterns.len() as u32).collect();
        order.sort_by(|left, right| {
            let left_len = self.patterns[*left as usize].value.len();
            let right_len = self.patterns[*right as usize].value.len();
            right_len.cmp(&left_len).then(left.cmp(right))
        });
        for index in order {
            let first = self.patterns[index as usize].value.as_bytes()[0];
            by_first_byte[first as usize].push(index);
        }
        self.by_first_byte = by_first_byte;
    }

    /// True when `candidate` reproduces no active value.
    fn is_emit_safe(&self, candidate: &str) -> bool {
        let bytes = candidate.as_bytes();
        (0..bytes.len()).all(|position| self.match_at(bytes, position).is_none())
    }

    /// Number of active values.
    pub fn active_count(&self) -> usize {
        self.patterns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Canonical source identities in registry order.
    pub fn canonical_sources(&self) -> impl Iterator<Item = &SourceId> {
        self.patterns.iter().map(|pattern| &pattern.canonical)
    }

    /// Sources that were deduplicated into another pattern (`REG-002`).
    pub fn aliases(&self) -> impl Iterator<Item = (&SourceId, &[SourceId])> {
        self.patterns
            .iter()
            .filter(|pattern| !pattern.aliases.is_empty())
            .map(|pattern| (&pattern.canonical, pattern.aliases.as_slice()))
    }

    /// Replaces every active value in one string value.
    ///
    /// Returns `None` when the input is unchanged so callers can preserve the
    /// original allocation and stay silent on clean events (`RED-009`).
    /// Replacement text is never rescanned (`RED-007`).
    pub fn redact(&self, input: &str, tally: &mut Tally) -> Option<String> {
        if self.patterns.is_empty() || input.is_empty() {
            return None;
        }

        let bytes = input.as_bytes();
        let mut output: Option<String> = None;
        let mut copied = 0usize;
        let mut cursor = 0usize;

        while cursor < bytes.len() {
            let Some(index) = self.match_at(bytes, cursor) else {
                cursor += 1;
                continue;
            };
            let pattern = &self.patterns[index as usize];
            let out = output.get_or_insert_with(|| String::with_capacity(input.len()));
            out.push_str(&input[copied..cursor]);
            out.push_str(&pattern.replacement);
            cursor += pattern.value.len();
            copied = cursor;
            tally.record(index as usize);
        }

        output.map(|mut out| {
            out.push_str(&input[copied..]);
            out
        })
    }

    /// Returns the leftmost-longest pattern starting exactly at `position`.
    fn match_at(&self, haystack: &[u8], position: usize) -> Option<u32> {
        let bucket = &self.by_first_byte[haystack[position] as usize];
        for index in bucket {
            let value = self.patterns[*index as usize].value.as_bytes();
            if haystack.len() - position >= value.len()
                && &haystack[position..position + value.len()] == value
            {
                return Some(*index);
            }
        }
        None
    }

    /// Builds emit-safe intervention metadata from a tally (`RED-008`).
    pub fn intervention(&self, tally: &Tally) -> Option<Intervention> {
        if tally.total == 0 {
            return None;
        }
        let mut named: Vec<LabeledCount> = Vec::new();
        let mut unnamed = 0usize;
        for (index, count) in tally.per_pattern.iter().enumerate() {
            if *count == 0 {
                continue;
            }
            match &self.patterns[index].label {
                Some(label) => named.push(LabeledCount {
                    label: label.clone(),
                    count: *count,
                }),
                None => unnamed += *count,
            }
        }
        Some(Intervention {
            total: tally.total,
            named,
            unnamed,
        })
    }

    /// Creates a tally sized for this registry.
    pub fn tally(&self) -> Tally {
        Tally {
            total: 0,
            per_pattern: vec![0; self.patterns.len()],
        }
    }
}

const GENERIC_PLACEHOLDER: &str = "<SECRET>";

/// Replacement counts accumulated while redacting one payload.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    total: usize,
    per_pattern: Vec<usize>,
}

impl Tally {
    fn record(&mut self, index: usize) {
        self.total += 1;
        self.per_pattern[index] += 1;
    }

    pub fn total(&self) -> usize {
        self.total
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }
}

/// Emit-safe result of one or more redactions.
///
/// It carries counts and safe labels only. Matched values, deterministic
/// hashes, source content, and value-derived previews are forbidden
/// (`SEC-004`, `RED-008`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intervention {
    pub total: usize,
    /// Per-source counts whose canonical label is emit-safe.
    pub named: Vec<LabeledCount>,
    /// Aggregated count for sources whose label could reproduce a value.
    pub unnamed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabeledCount {
    pub label: String,
    pub count: usize,
}

impl Intervention {
    /// One-line, emit-safe summary for host UI, for example
    /// `SecretSieve replaced 3 values (GITHUB_TOKEN x2, 1 unnamed)`.
    pub fn summary(&self) -> String {
        let plural = if self.total == 1 { "value" } else { "values" };
        let mut detail: Vec<String> = self
            .named
            .iter()
            .map(|entry| {
                if entry.count == 1 {
                    entry.label.clone()
                } else {
                    format!("{} x{}", entry.label, entry.count)
                }
            })
            .collect();
        if self.unnamed > 0 {
            detail.push(format!("{} unnamed", self.unnamed));
        }
        format!(
            "SecretSieve replaced {} {plural} ({})",
            self.total,
            detail.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::SourceId;
    use crate::testing::Canary;

    fn secret(label: &str, value: &str) -> ResolvedSecret {
        ResolvedSecret {
            value: value.to_string(),
            label: label.to_string(),
            source: SourceId::env(label),
        }
    }

    fn redact(redactor: &Redactor, input: &str) -> (Option<String>, Tally) {
        let mut tally = redactor.tally();
        let output = redactor.redact(input, &mut tally);
        (output, tally)
    }

    #[test]
    fn clean_input_is_unchanged_and_silent() {
        let redactor = Redactor::new(vec![secret("TOKEN", "abc")]);
        let (output, tally) = redact(&redactor, "nothing here");
        assert_eq!(output, None);
        assert!(tally.is_empty());
        assert_eq!(redactor.intervention(&tally), None);
    }

    #[test]
    fn exact_values_are_replaced_with_named_placeholders() {
        let redactor = Redactor::new(vec![
            secret("GITHUB_TOKEN", "ghp_CANARY_123456"),
            secret("SHORT_TOKEN", "CANARY_123"),
        ]);
        let (output, tally) = redact(
            &redactor,
            "Authorization: ghp_CANARY_123456; fallback=CANARY_123",
        );
        assert_eq!(
            output.as_deref(),
            Some("Authorization: <SECRET:GITHUB_TOKEN>; fallback=<SECRET:SHORT_TOKEN>")
        );
        assert_eq!(tally.total(), 2);
    }

    #[test]
    fn matching_is_case_sensitive_and_byte_exact() {
        let redactor = Redactor::new(vec![secret("TOKEN", "Secret")]);
        assert_eq!(redact(&redactor, "secret SECRET").0, None);
        assert_eq!(
            redact(&redactor, "Secret").0.as_deref(),
            Some("<SECRET:TOKEN>")
        );
    }

    #[test]
    fn matching_is_substring_matching() {
        // `RED-004`: token and word boundaries have no meaning.
        let redactor = Redactor::new(vec![secret("TOKEN", "abc")]);
        assert_eq!(
            redact(&redactor, "xxabcyy").0.as_deref(),
            Some("xx<SECRET:TOKEN>yy")
        );
    }

    #[test]
    fn adjacent_matches_are_each_replaced() {
        let redactor = Redactor::new(vec![secret("TOKEN", "ab")]);
        let (output, tally) = redact(&redactor, "abab");
        assert_eq!(output.as_deref(), Some("<SECRET:TOKEN><SECRET:TOKEN>"));
        assert_eq!(tally.total(), 2);
    }

    #[test]
    fn same_start_overlap_prefers_the_longest_value() {
        let redactor = Redactor::new(vec![secret("SHORT", "abc"), secret("LONG", "abcd")]);
        assert_eq!(
            redact(&redactor, "zabcd").0.as_deref(),
            Some("z<SECRET:LONG>")
        );
    }

    #[test]
    fn different_start_overlap_prefers_the_earliest_start() {
        // `RED-003`: earliest start wins even when a later match is longer.
        let redactor = Redactor::new(vec![secret("EARLY", "abc"), secret("LATE", "bcdef")]);
        assert_eq!(
            redact(&redactor, "abcdef").0.as_deref(),
            Some("<SECRET:EARLY>def")
        );
    }

    #[test]
    fn duplicate_values_collapse_to_the_canonical_source() {
        let redactor = Redactor::new(vec![secret("PROJECT", "same"), secret("GLOBAL", "same")]);
        assert_eq!(redactor.active_count(), 1);
        let (output, tally) = redact(&redactor, "same");
        assert_eq!(output.as_deref(), Some("<SECRET:PROJECT>"));
        let intervention = redactor.intervention(&tally).expect("intervention");
        assert_eq!(intervention.named[0].label, "PROJECT");
        let aliases: Vec<_> = redactor.aliases().collect();
        assert_eq!(aliases.len(), 1);
        assert_eq!(aliases[0].1.len(), 1);
    }

    #[test]
    fn multiline_values_match_across_line_breaks() {
        let redactor = Redactor::new(vec![secret("KEY", "line1\nline2")]);
        assert_eq!(
            redact(&redactor, "a\nline1\nline2\nb").0.as_deref(),
            Some("a\n<SECRET:KEY>\nb")
        );
    }

    #[test]
    fn utf8_values_match_without_normalization() {
        let redactor = Redactor::new(vec![secret("UNICODE", "pässwörd✓")]);
        assert_eq!(
            redact(&redactor, "x pässwörd✓ y").0.as_deref(),
            Some("x <SECRET:UNICODE> y")
        );
        // Decomposed form is a different byte sequence and must not match.
        assert_eq!(redact(&redactor, "pa\u{0308}sswo\u{0308}rd✓").0, None);
    }

    #[test]
    fn a_value_inside_the_named_placeholder_forces_the_generic_form() {
        let redactor = Redactor::new(vec![secret("TOKEN", "TOKEN")]);
        assert_eq!(redact(&redactor, "TOKEN").0.as_deref(), Some("<SECRET>"));
    }

    #[test]
    fn a_value_inside_every_placeholder_forces_deletion() {
        let redactor = Redactor::new(vec![secret("TOKEN", "TOKEN"), secret("MARKER", "SECRET")]);
        assert_eq!(redact(&redactor, "[TOKEN]").0.as_deref(), Some("[]"));
        assert_eq!(redact(&redactor, "[SECRET]").0.as_deref(), Some("[]"));
    }

    #[test]
    fn unsafe_labels_are_aggregated_without_names() {
        // The label reproduces another active value, so it must not be emitted.
        let redactor = Redactor::new(vec![
            secret("PREFIX_TOKEN", "value-a"),
            secret("X", "TOKEN"),
        ]);
        let (_, tally) = redact(&redactor, "value-a");
        let intervention = redactor.intervention(&tally).expect("intervention");
        assert_eq!(intervention.total, 1);
        assert!(intervention.named.is_empty());
        assert_eq!(intervention.unnamed, 1);
        assert!(!intervention.summary().contains("PREFIX_TOKEN"));
    }

    #[test]
    fn replacements_are_never_rescanned() {
        // `RED-007`: scanning resumes after the inserted placeholder, so a value
        // that only appears because of the insertion is not matched. Here "T>c"
        // exists only across the boundary of `<SECRET:T>` and the trailing "c".
        let redactor = Redactor::new(vec![secret("T", "ab"), secret("B", "T>c")]);
        let (output, tally) = redact(&redactor, "abc");
        assert_eq!(output.as_deref(), Some("<SECRET:T>c"));
        assert_eq!(tally.total(), 1);
    }

    #[test]
    fn a_placeholder_that_reproduces_a_value_is_rejected_before_insertion() {
        // `RED-006`: the named form is checked against every active value first.
        let redactor = Redactor::new(vec![secret("A", "xy"), secret("B", "<SECRET:A>")]);
        assert_eq!(redact(&redactor, "xy").0.as_deref(), Some("<SECRET>"));
    }

    #[test]
    fn intervention_summary_is_canary_free() {
        let canary = Canary::generate("GITHUB_TOKEN");
        let redactor = Redactor::new(vec![secret(canary.label(), canary.value())]);
        let (output, tally) = redact(&redactor, &format!("token={} again", canary.value()));
        let output = output.expect("intervention occurred");
        crate::testing::assert_canary_absent("redacted output", output.as_bytes(), &canary);
        let intervention = redactor.intervention(&tally).expect("intervention");
        crate::testing::assert_canary_absent(
            "intervention summary",
            intervention.summary().as_bytes(),
            &canary,
        );
        assert_eq!(intervention.total, 1);
    }

    #[test]
    fn an_empty_value_never_becomes_a_pattern() {
        // An empty pattern would match at every position. Resolvers already
        // treat empty values as unresolved; this is the release-build backstop.
        let redactor = Redactor::new(vec![secret("EMPTY", ""), secret("REAL", "abc")]);
        assert_eq!(redactor.active_count(), 1);
        assert_eq!(
            redact(&redactor, "xabcx").0.as_deref(),
            Some("x<SECRET:REAL>x")
        );
    }

    #[test]
    fn empty_input_and_empty_registry_are_no_ops() {
        let empty = Redactor::new(Vec::new());
        assert!(empty.is_empty());
        assert_eq!(redact(&empty, "anything").0, None);
        let redactor = Redactor::new(vec![secret("TOKEN", "abc")]);
        assert_eq!(redact(&redactor, "").0, None);
    }
}

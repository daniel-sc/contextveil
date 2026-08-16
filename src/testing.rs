//! Generated canaries and secret-safe assertions for tests.
//!
//! Tests must never embed a real credential (`AGENTS.md` security rules) and
//! must assert that a matched canary is absent from every output channel after
//! intervention (`TST-005`). Failure messages here therefore report a channel,
//! a label, and an offset, never the value that leaked.
//!
//! This module is compiled only for tests or behind the `testing` feature.

use std::hash::{BuildHasher, Hasher, RandomState};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A conspicuous generated value that stands in for a credential.
///
/// The value carries a fixed prefix so an accidental disclosure is obvious in a
/// diff or transcript, plus a per-instance random token so two canaries in one
/// test are never confused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canary {
    label: String,
    token: String,
    value: String,
}

impl Canary {
    /// Generates a unique canary for `label`.
    ///
    /// `label` is normalized to the characters a safe placeholder label may
    /// contain so that a canary can be used directly as an environment or
    /// dotenv key name.
    pub fn generate(label: &str) -> Self {
        let label = normalize_label(label);
        let token = random_token();
        let value = format!("SSCANARY_{label}_{token}");
        Self {
            label,
            token,
            value,
        }
    }

    /// Generates a canary whose value is exactly `length` ASCII characters.
    ///
    /// Used by preview-masking and short-value tests, which depend on length.
    pub fn generate_with_length(label: &str, length: usize) -> Self {
        let mut canary = Self::generate(label);
        let mut value = canary.value.clone();
        while value.len() < length {
            value.push_str(&random_token());
        }
        value.truncate(length);
        canary.value = value;
        canary
    }

    /// The stand-in credential value. Never write this to a durable artifact.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// A display name safe to appear in diagnostics and placeholders.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The random component. Its presence alone proves partial disclosure.
    pub fn token(&self) -> &str {
        &self.token
    }
}

/// Asserts that `haystack` discloses neither the canary value nor its unique
/// random component.
///
/// `channel` names the inspected output, for example `stdout`, `stderr`,
/// `updatedToolOutput`, or a config path.
#[track_caller]
pub fn assert_canary_absent(channel: &str, haystack: &[u8], canary: &Canary) {
    if let Some(offset) = find(haystack, canary.value.as_bytes()) {
        panic!(
            "canary `{}` disclosed in {channel} at byte offset {offset} (value withheld)",
            canary.label
        );
    }
    if let Some(offset) = find(haystack, canary.token.as_bytes()) {
        panic!(
            "canary `{}` partially disclosed in {channel} at byte offset {offset} (value withheld)",
            canary.label
        );
    }
}

/// Asserts that none of `canaries` is disclosed in `haystack`.
#[track_caller]
pub fn assert_canaries_absent(channel: &str, haystack: &[u8], canaries: &[&Canary]) {
    for canary in canaries {
        assert_canary_absent(channel, haystack, canary);
    }
}

/// Asserts that `haystack` still contains the canary.
///
/// Used to prove a fixture actually exercises a redaction path before the
/// absence assertions become meaningful.
#[track_caller]
pub fn assert_canary_present(channel: &str, haystack: &[u8], canary: &Canary) {
    if find(haystack, canary.value.as_bytes()).is_none() {
        panic!(
            "canary `{}` is missing from {channel}; the fixture no longer exercises this path",
            canary.label
        );
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn normalize_label(label: &str) -> String {
    // `REG-004` label shape: keep ASCII word characters, collapse the rest.
    let mut normalized = String::with_capacity(label.len());
    let mut pending_separator = false;
    for character in label.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.') {
            if pending_separator {
                normalized.push('_');
                pending_separator = false;
            }
            normalized.push(character);
        } else if !normalized.is_empty() {
            pending_separator = true;
        }
    }
    if normalized.is_empty() {
        normalized.push_str("CANARY");
    }
    normalized
}

fn random_token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();

    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(sequence);
    hasher.write_u128(nanos);
    hasher.write_u32(std::process::id());
    format!("{:016x}{:04x}", hasher.finish(), sequence & 0xffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canaries_are_unique_and_conspicuous() {
        let first = Canary::generate("GITHUB_TOKEN");
        let second = Canary::generate("GITHUB_TOKEN");
        assert_ne!(first.value(), second.value());
        assert!(first.value().starts_with("SSCANARY_GITHUB_TOKEN_"));
        assert_eq!(first.label(), "GITHUB_TOKEN");
    }

    #[test]
    fn labels_are_normalized_to_safe_characters() {
        assert_eq!(normalize_label("api key/value"), "api_key_value");
        assert_eq!(normalize_label("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(normalize_label("  "), "CANARY");
    }

    #[test]
    fn generated_length_is_exact() {
        for length in [1usize, 4, 5, 15, 16, 40] {
            let canary = Canary::generate_with_length("SHORT", length);
            assert_eq!(canary.value().chars().count(), length);
        }
    }

    #[test]
    fn absence_assertions_accept_clean_output() {
        let canary = Canary::generate("TOKEN");
        assert_canary_absent("stdout", b"nothing to see", &canary);
        assert_canaries_absent("stdout", b"<SECRET:TOKEN>", &[&canary]);
    }

    #[test]
    #[should_panic(expected = "value withheld")]
    fn absence_assertions_reject_a_full_disclosure() {
        let canary = Canary::generate("TOKEN");
        let leaked = format!("output={}", canary.value());
        assert_canary_absent("stdout", leaked.as_bytes(), &canary);
    }

    #[test]
    #[should_panic(expected = "partially disclosed")]
    fn absence_assertions_reject_a_partial_disclosure() {
        let canary = Canary::generate("TOKEN");
        let leaked = format!("fragment={}", canary.token());
        assert_canary_absent("stderr", leaked.as_bytes(), &canary);
    }

    #[test]
    fn failure_messages_withhold_the_value() {
        let canary = Canary::generate("TOKEN");
        let leaked = format!("output={}", canary.value());
        let panic = std::panic::catch_unwind(|| {
            assert_canary_absent("stdout", leaked.as_bytes(), &canary);
        })
        .expect_err("the assertion must fail");
        let message = panic
            .downcast_ref::<String>()
            .expect("panic payload is a string");
        assert!(!message.contains(canary.token()));
        assert!(message.contains("TOKEN"));
    }

    #[test]
    fn presence_assertions_detect_an_unused_fixture() {
        let canary = Canary::generate("TOKEN");
        assert_canary_present("fixture", canary.value().as_bytes(), &canary);
        let panic = std::panic::catch_unwind(|| {
            assert_canary_present("fixture", b"clean", &canary);
        });
        assert!(panic.is_err());
    }
}

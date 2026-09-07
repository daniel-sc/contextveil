//! Admission vocabularies for setup candidates.
//!
//! `SET-006` and `SET-023` fix the V1 vocabularies exactly. Vocabulary changes
//! are observable setup behavior and must update the specification and its
//! fixtures in the same change.

/// Whole tokens that gate a name.
const EXACT_TOKENS: [&str; 8] = [
    "token",
    "secret",
    "password",
    "passwd",
    "passphrase",
    "key",
    "credential",
    "credentials",
];

/// Suffixes of the compact form that gate a name.
const COMPACT_SUFFIXES: [&str; 13] = [
    "token",
    "secret",
    "password",
    "passwd",
    "passphrase",
    "credential",
    "credentials",
    "apikey",
    "accesskey",
    "privatekey",
    "clientsecret",
    "authtoken",
    "refreshtoken",
];

/// Complete normalized values excluded from wholly new automatic candidates.
const COMMON_LITERALS: [&str; 17] = [
    "true",
    "false",
    "yes",
    "no",
    "on",
    "off",
    "0",
    "1",
    "enabled",
    "disabled",
    "null",
    "nil",
    "none",
    "undefined",
    "n/a",
    "default",
    "auto",
];

/// Returns the vocabulary term that gates `name`, if any.
///
/// Matching uses ASCII case folding. The name is split into tokens at every run
/// of non-ASCII-alphanumeric characters, and a compact form is built by removing
/// those separators. Characters outside ASCII are preserved for display but do
/// not match the vocabulary.
pub fn gating_term(name: &str) -> Option<&'static str> {
    let folded: Vec<char> = name.chars().map(|c| c.to_ascii_lowercase()).collect();

    let mut tokens: Vec<String> = Vec::new();
    let mut compact = String::new();
    let mut current = String::new();
    for character in folded {
        if character.is_ascii_alphanumeric() {
            current.push(character);
            compact.push(character);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    for term in EXACT_TOKENS {
        if tokens.iter().any(|token| token == term) {
            return Some(term);
        }
    }
    // Longest suffix first so `refreshtoken` explains itself rather than
    // reporting the shorter `token`.
    let mut suffixes = COMPACT_SUFFIXES;
    suffixes.sort_by_key(|suffix| std::cmp::Reverse(suffix.len()));
    suffixes
        .into_iter()
        .find(|suffix| compact.ends_with(suffix))
}

/// Whether a complete, already normalized source value is a Common Literal.
pub fn is_common_literal(value: &str) -> bool {
    COMMON_LITERALS
        .iter()
        .any(|literal| value.eq_ignore_ascii_case(literal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_tokens_gate_a_name() {
        for name in [
            "GITHUB_TOKEN",
            "api-key",
            "MY.SECRET.VALUE",
            "db password",
            "PASSWD_FILE",
            "user_passphrase_1",
            "AWS_CREDENTIAL",
            "credentials",
            "KEY",
        ] {
            assert!(gating_term(name).is_some(), "`{name}` should be gated");
        }
    }

    #[test]
    fn compact_suffixes_gate_a_name() {
        for (name, expected) in [
            ("GITHUBAPIKEY", "apikey"),
            ("myAccessKey", "accesskey"),
            ("service_private_key", "key"),
            ("SomePrivateKey", "privatekey"),
            ("client-secret", "secret"),
            ("OAUTH_REFRESH_TOKEN", "token"),
        ] {
            assert_eq!(gating_term(name), Some(expected), "for `{name}`");
        }
        // A compact suffix with no separate token still gates.
        assert_eq!(gating_term("stripeapikey"), Some("apikey"));
        assert_eq!(gating_term("myrefreshtoken"), Some("refreshtoken"));
        assert_eq!(gating_term("appclientsecret"), Some("clientsecret"));
    }

    #[test]
    fn unrelated_names_are_not_gated() {
        for name in [
            "PATH",
            "HOME",
            "EDITOR",
            "DATABASE_URL",
            "keyboard_layout",
            "TOKENIZER",
            "monkey",
            "keys",
        ] {
            assert_eq!(gating_term(name), None, "`{name}` should not be gated");
        }
    }

    #[test]
    fn gating_is_ascii_case_insensitive_only() {
        assert_eq!(gating_term("Github_Token"), Some("token"));
        assert_eq!(gating_term("gitHUB_tOkEn"), Some("token"));
        // Non-ASCII characters act as separators and never match themselves.
        assert_eq!(gating_term("TÖKEN"), None);
        assert_eq!(gating_term("secret✓"), Some("secret"));
        assert_eq!(gating_term("prefix✓token"), Some("token"));
    }

    #[test]
    fn common_literals_are_exact_and_ascii_case_insensitive() {
        for value in COMMON_LITERALS {
            assert!(is_common_literal(value), "`{value}` should be excluded");
            assert!(
                is_common_literal(&value.to_ascii_uppercase()),
                "`{value}` should be excluded regardless of ASCII case"
            );
        }
        for value in ["", " true ", "truex", "xtrue", "n\\a", "áuto"] {
            assert!(
                !is_common_literal(value),
                "`{value}` should remain eligible"
            );
        }
    }
}

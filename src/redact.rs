//! Structured redaction over decoded JSON values.
//!
//! `RED-005`: only decoded string values are transformed. Object keys, numbers,
//! booleans, nulls, and structure are preserved exactly, so a host that
//! validates the returned shape still accepts it (`CLA-002`).
//!
//! Each string value is matched independently (`RED-002`); adjacent fields are
//! never joined.

use serde_json::Value;

use crate::matcher::{Redactor, Tally};

/// Redacts every string value in `value` in place.
///
/// Returns `true` when at least one replacement occurred.
pub fn redact_json(value: &mut Value, redactor: &Redactor, tally: &mut Tally) -> bool {
    let before = tally.total();
    redact_in_place(value, redactor, tally);
    tally.total() > before
}

fn redact_in_place(value: &mut Value, redactor: &Redactor, tally: &mut Tally) {
    match value {
        Value::String(text) => {
            if let Some(replaced) = redactor.redact(text, tally) {
                *text = replaced;
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_in_place(item, redactor, tally);
            }
        }
        Value::Object(entries) => {
            // Keys are never transformed (`RED-005`, `LIM-003`).
            for (_key, entry) in entries.iter_mut() {
                redact_in_place(entry, redactor, tally);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::{ResolvedSecret, SourceId};
    use crate::testing::{Canary, assert_canary_absent};
    use serde_json::json;

    fn redactor_for(pairs: &[(&str, &str)]) -> Redactor {
        Redactor::new(
            pairs
                .iter()
                .map(|(label, value)| ResolvedSecret::new(SourceId::env(*label), value.to_string()))
                .collect(),
        )
    }

    #[test]
    fn nested_string_values_are_redacted_and_shape_is_preserved() {
        let canary = Canary::generate("GITHUB_TOKEN");
        let redactor = redactor_for(&[("GITHUB_TOKEN", canary.value())]);
        let mut value = json!({
            "stdout": format!("TOKEN={}\n", canary.value()),
            "stderr": "",
            "exitCode": 0,
            "ok": true,
            "nothing": null,
            "nested": {"items": [format!("x{}", canary.value()), "clean"]}
        });

        let mut tally = redactor.tally();
        assert!(redact_json(&mut value, &redactor, &mut tally));
        assert_eq!(tally.total(), 2);

        assert_eq!(value["stdout"], json!("TOKEN=<SECRET:GITHUB_TOKEN>\n"));
        assert_eq!(value["exitCode"], json!(0));
        assert_eq!(value["ok"], json!(true));
        assert_eq!(value["nothing"], json!(null));
        assert_eq!(value["nested"]["items"][1], json!("clean"));
        assert_canary_absent("structured output", value.to_string().as_bytes(), &canary);
    }

    #[test]
    fn object_keys_are_never_transformed() {
        // `LIM-003`: a secret used as an object key stays visible by design.
        let redactor = redactor_for(&[("TOKEN", "abc")]);
        let mut value = json!({"abc": "abc"});
        let mut tally = redactor.tally();
        assert!(redact_json(&mut value, &redactor, &mut tally));
        assert_eq!(value, json!({"abc": "<SECRET:TOKEN>"}));
        assert_eq!(tally.total(), 1);
    }

    #[test]
    fn values_are_matched_independently_across_fields() {
        // `RED-002`: adjacent fields must not be joined.
        let redactor = redactor_for(&[("TOKEN", "abcdef")]);
        let mut value = json!({"first": "abc", "second": "def"});
        let mut tally = redactor.tally();
        assert!(!redact_json(&mut value, &redactor, &mut tally));
        assert_eq!(value, json!({"first": "abc", "second": "def"}));
    }

    #[test]
    fn clean_payloads_are_untouched() {
        let redactor = redactor_for(&[("TOKEN", "abc")]);
        let original = json!({"stdout": "clean", "items": [1, 2, 3]});
        let mut value = original.clone();
        let mut tally = redactor.tally();
        assert!(!redact_json(&mut value, &redactor, &mut tally));
        assert_eq!(value, original);
    }

    #[test]
    fn numbers_and_booleans_that_look_like_values_are_left_alone() {
        let redactor = redactor_for(&[("TOKEN", "1234")]);
        let mut value = json!({"count": 1234, "flag": true, "text": "1234"});
        let mut tally = redactor.tally();
        assert!(redact_json(&mut value, &redactor, &mut tally));
        assert_eq!(value["count"], json!(1234));
        assert_eq!(value["text"], json!("<SECRET:TOKEN>"));
    }

    #[test]
    fn key_order_is_preserved() {
        let redactor = redactor_for(&[("TOKEN", "abc")]);
        let mut value: Value =
            serde_json::from_str(r#"{"zeta": "abc", "alpha": 1, "middle": "x"}"#).expect("json");
        let mut tally = redactor.tally();
        redact_json(&mut value, &redactor, &mut tally);
        assert_eq!(
            value.to_string(),
            r#"{"zeta":"<SECRET:TOKEN>","alpha":1,"middle":"x"}"#
        );
    }
}

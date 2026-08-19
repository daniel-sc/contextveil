//! Property tests comparing the production matcher with a reference model.
//!
//! The reference implements `RED-001` through `RED-007` in the most direct way
//! possible: rebuild the placeholder decision from scratch, then scan character
//! by character choosing the leftmost-longest active value. Any disagreement
//! with the optimized matcher is a defect in the optimization.
//!
//! Inputs are generated from a deterministic PRNG so a failure is reproducible
//! from its seed and CI never depends on a random schedule.
//!
//! The named `TST-001` vectors have focused tests in `src/matcher.rs`; this file
//! covers the same rules over generated input.

use contextveil::matcher::Redactor;
use contextveil::secret::{ResolvedSecret, SourceId};

/// A small xorshift generator. Deterministic and dependency-free.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut state = self.0;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        self.0 = state;
        state
    }

    fn below(&mut self, limit: usize) -> usize {
        (self.next() % limit as u64) as usize
    }

    fn pick<'a, T>(&mut self, values: &'a [T]) -> &'a T {
        &values[self.below(values.len())]
    }
}

/// Reference implementation of the specified semantics.
mod reference {
    /// Chooses one replacement per value exactly as `RED-006` describes.
    fn replacements(values: &[String], labels: &[String]) -> Vec<String> {
        let safe = |candidate: &str| {
            !values
                .iter()
                .any(|value| candidate.contains(value.as_str()))
        };
        values
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let named = format!("<SECRET:{}>", labels[index]);
                if safe(&named) {
                    named
                } else if safe("<SECRET>") {
                    "<SECRET>".to_string()
                } else {
                    String::new()
                }
            })
            .collect()
    }

    /// Leftmost-longest replacement without any index or bucket optimization.
    pub fn redact(input: &str, values: &[String], labels: &[String]) -> (String, usize) {
        let replacements = replacements(values, labels);
        let mut output = String::new();
        let mut count = 0;
        let mut rest = input;

        'outer: while !rest.is_empty() {
            let mut best: Option<usize> = None;
            for (index, value) in values.iter().enumerate() {
                if !rest.starts_with(value.as_str()) {
                    continue;
                }
                match best {
                    // Longest wins; registry order breaks a tie, which only
                    // matters for equal-length distinct values.
                    Some(current) if values[current].len() >= value.len() => {}
                    _ => best = Some(index),
                }
            }
            if let Some(index) = best {
                output.push_str(&replacements[index]);
                rest = &rest[values[index].len()..];
                count += 1;
                continue 'outer;
            }
            let character = rest.chars().next().expect("the input is not empty");
            output.push(character);
            rest = &rest[character.len_utf8()..];
        }

        (output, count)
    }
}

/// Builds both matchers from the same deduplicated value set.
fn case(values: &[String], labels: &[String]) -> Redactor {
    Redactor::new(
        values
            .iter()
            .zip(labels)
            .map(|(value, label)| ResolvedSecret::new(SourceId::env(label.clone()), value.clone()))
            .collect(),
    )
}

fn generate_values(rng: &mut Rng, alphabet: &[&str]) -> (Vec<String>, Vec<String>) {
    let count = 1 + rng.below(5);
    let mut values: Vec<String> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    while values.len() < count {
        let length = 1 + rng.below(5);
        let mut value = String::new();
        for _ in 0..length {
            value.push_str(rng.pick(alphabet));
        }
        // Duplicate values are deduplicated by the registry; the reference
        // models one pattern per distinct value.
        if values.contains(&value) {
            continue;
        }
        labels.push(format!("VALUE_{}", values.len()));
        values.push(value);
    }
    (values, labels)
}

fn generate_input(rng: &mut Rng, alphabet: &[&str]) -> String {
    let length = rng.below(40);
    let mut input = String::new();
    for _ in 0..length {
        input.push_str(rng.pick(alphabet));
    }
    input
}

#[test]
fn the_matcher_agrees_with_the_reference_model() {
    // The alphabet is tiny on purpose: short values over few symbols produce
    // overlaps, adjacency, and same-start collisions in almost every case.
    let alphabet = ["a", "b", "c", "ä", "✓", "\n", " "];
    let mut rng = Rng(0x5ec2_5133_0000_0001);

    for iteration in 0..4000 {
        let (values, labels) = generate_values(&mut rng, &alphabet);
        let input = generate_input(&mut rng, &alphabet);

        let redactor = case(&values, &labels);
        let mut tally = redactor.tally();
        let produced = redactor
            .redact(&input, &mut tally)
            .unwrap_or_else(|| input.clone());
        let (expected, expected_count) = reference::redact(&input, &values, &labels);

        assert_eq!(
            produced, expected,
            "iteration {iteration}: values {values:?} input {input:?}"
        );
        assert_eq!(
            tally.total(),
            expected_count,
            "iteration {iteration}: values {values:?} input {input:?}"
        );
    }
}

#[test]
fn no_active_value_survives_when_placeholders_cannot_be_reconstructed() {
    // With an alphabet that excludes the placeholder syntax, no concatenation of
    // surrounding text and an inserted placeholder can recreate an active value,
    // so redaction must leave none behind.
    let alphabet = ["a", "b", "c", "ä", "✓", " "];
    let mut rng = Rng(0x5ec2_5133_0000_0002);

    for iteration in 0..2000 {
        let (values, labels) = generate_values(&mut rng, &alphabet);
        let input = generate_input(&mut rng, &alphabet);

        let redactor = case(&values, &labels);
        let mut tally = redactor.tally();
        let produced = redactor
            .redact(&input, &mut tally)
            .unwrap_or_else(|| input.clone());

        for value in &values {
            assert!(
                !produced.contains(value.as_str()),
                "iteration {iteration}: an active value survived; values {values:?} input {input:?}"
            );
        }
    }
}

#[test]
fn intervention_counts_match_the_number_of_replacements() {
    let alphabet = ["a", "b", "ab"];
    let mut rng = Rng(0x5ec2_5133_0000_0003);

    for _ in 0..1000 {
        let (values, labels) = generate_values(&mut rng, &alphabet);
        let input = generate_input(&mut rng, &alphabet);

        let redactor = case(&values, &labels);
        let mut tally = redactor.tally();
        redactor.redact(&input, &mut tally);

        match redactor.intervention(&tally) {
            None => assert_eq!(tally.total(), 0),
            Some(intervention) => {
                assert_eq!(intervention.total, tally.total());
                let reported: usize = intervention
                    .named
                    .iter()
                    .map(|entry| entry.count)
                    .sum::<usize>()
                    + intervention.unnamed;
                assert_eq!(reported, intervention.total);
            }
        }
    }
}

#[test]
fn redaction_is_idempotent_for_ordinary_values() {
    // Running the matcher again over its own output changes nothing, because
    // placeholders are never fed back through the matcher (`RED-007`).
    let alphabet = ["a", "b", "c", " "];
    let mut rng = Rng(0x5ec2_5133_0000_0004);

    for _ in 0..1000 {
        let (values, labels) = generate_values(&mut rng, &alphabet);
        let input = generate_input(&mut rng, &alphabet);

        let redactor = case(&values, &labels);
        let mut first_tally = redactor.tally();
        let once = redactor
            .redact(&input, &mut first_tally)
            .unwrap_or_else(|| input.clone());
        let mut second_tally = redactor.tally();
        assert_eq!(redactor.redact(&once, &mut second_tally), None);
    }
}

//! Public wording checks (`SEC-002`, `SUP-002`, `SUP-003`, `SUP-005`, `TST-008`).
//!
//! `SEC-002` is a requirement about what SecretSieve must never claim, and
//! `LIM-001` states that public wording is verified by test. These read the
//! shipped documents and fail on an overclaim or a missing boundary statement, so
//! an edit that quietly promises too much cannot pass review unnoticed.

use std::path::Path;

/// Documents that make public claims about what SecretSieve does.
const PUBLIC_DOCUMENTS: [&str; 4] = [
    "README.md",
    "vision.md",
    "docs/release-notes-template.md",
    "SECURITY.md",
];

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("{} could not be read: {error}", path.display());
    })
}

#[test]
fn no_public_document_promises_more_than_the_security_claim() {
    // `SEC-002`: SecretSieve must not claim to prevent direct local use, network
    // exfiltration, unknown or transformed values, host bypass, or anything
    // outside the support matrix.
    // Phrases that overclaim in any context.
    let forbidden = [
        "prevents exfiltration",
        "prevents leaks",
        "no secret can",
        "fully protected",
        "complete protection",
        "guaranteed",
        "guarantees that",
        "100%",
        "bulletproof",
        "airtight",
    ];
    // Phrases that are only acceptable inside an explicit disclaimer, such as
    // "not a claim that credentials can never leave the machine".
    let requires_disclaimer = ["never leave", "cannot leave", "can't leave", "fail closed"];

    for document in PUBLIC_DOCUMENTS {
        let text = read(document).to_lowercase();
        for phrase in forbidden {
            assert!(
                !text.contains(phrase),
                "{document} contains the overclaiming phrase `{phrase}`"
            );
        }
        // Line wrapping must not split a sentence, or a disclaimer could land on
        // the other side of the break and look like a bare claim.
        let unwrapped = text.replace('\n', " ");
        for phrase in requires_disclaimer {
            for sentence in unwrapped.split('.') {
                if !sentence.contains(phrase) {
                    continue;
                }
                let disclaimed = ["not ", "no ", "cannot be made", "never a claim"]
                    .iter()
                    .any(|marker| sentence.contains(marker));
                assert!(
                    disclaimed,
                    "{document} uses `{phrase}` without a disclaimer: {sentence}"
                );
            }
        }
    }
}

#[test]
fn the_readme_states_the_boundary_it_must_state() {
    let text = read("README.md");
    for required in [
        // The product is not a different kind of control.
        "not a vault, sandbox, network firewall, DLP system",
        // Transformed values are out of scope (`LIM-002`).
        "transformed values are not",
        // Process hooks fail open (`LIM-012`).
        "fail open",
        // Only enrolled, exact values are covered (`SEC-001`).
        "current, exact values from sources you enroll",
    ] {
        assert!(
            text.to_lowercase().contains(&required.to_lowercase()),
            "README.md no longer states: {required}"
        );
    }
}

#[test]
fn every_public_support_matrix_labels_the_tiers() {
    // `SUP-002` and `SUP-003`: Claude is the production integration, the other
    // three are experimental and must be labeled wherever they appear.
    for document in ["README.md", "vision.md", "docs/release-notes-template.md"] {
        let text = read(document);
        assert!(
            text.contains("Claude Code"),
            "{document} does not name Claude Code"
        );
        for experimental in ["Codex", "Copilot", "OpenCode"] {
            let line = text
                .lines()
                .find(|line| line.contains(experimental) && line.contains('|'))
                .unwrap_or_else(|| {
                    panic!("{document} has no support-matrix row for {experimental}")
                });
            let labeled = line.contains("EXPERIMENTAL") || line.contains("Experimental");
            assert!(
                labeled,
                "{document} does not label {experimental} as experimental: {line}"
            );
        }
    }
}

#[test]
fn coverage_is_scoped_to_local_harnesses_that_honor_the_integration() {
    // `SUP-005`: cloud, remote, container, and managed-policy modes are covered
    // only where SecretSieve is separately installed and the hook is honored.
    let readme = read("README.md").to_lowercase();
    assert!(
        readme.contains("local harness") || readme.contains("locally"),
        "README.md does not scope coverage to local harness modes"
    );
    for mode in ["cloud", "remote", "container"] {
        assert!(
            readme.contains(mode),
            "README.md does not mention {mode} modes"
        );
    }
}

#[test]
fn no_routine_workflow_runs_a_paid_or_networked_check() {
    // `TST-008`: optional paid or networked tests must not gate routine CI.
    for workflow in ["ci.yml", "fuzz.yml", "release.yml"] {
        let text = read(&format!(".github/workflows/{workflow}"));
        for forbidden in ["live canary", "live-canary", "ANTHROPIC_API_KEY"] {
            assert!(
                !text.contains(forbidden),
                "{workflow} references {forbidden}, which would make CI paid or networked"
            );
        }
    }
}

#[test]
fn the_release_notes_link_the_governing_limitations() {
    // Release notes must carry the support matrix and limitation links.
    let text = read("docs/release-notes-template.md");
    for limitation in ["LIM-001", "LIM-002", "LIM-003", "LIM-012", "LIM-013"] {
        assert!(
            text.contains(limitation),
            "the release notes no longer link {limitation}"
        );
    }
    assert!(
        text.contains("SECURITY.md"),
        "the release notes omit the reporting policy"
    );
}

#[test]
fn every_limitation_and_deviation_has_the_required_sections() {
    let text = read("limitations.md");
    let mut entries = 0;
    let mut deviations = 0;
    for block in text.split("\n### ").skip(1) {
        let heading = block.lines().next().unwrap_or_default().to_string();
        // The file ends with a fenced template for future entries; it is a
        // placeholder, not an entry.
        if heading.contains("NNN") {
            continue;
        }
        if heading.starts_with("LIM-") {
            entries += 1;
            for section in [
                "**Reality:**",
                "**Impact:**",
                "**Workaround:**",
                "**Verification:**",
            ] {
                assert!(block.contains(section), "{heading} is missing {section}");
            }
        } else if heading.starts_with("DEV-") {
            deviations += 1;
            for section in [
                "Contract:",
                "**Observed behavior:**",
                "**Impact:**",
                "**Verification:**",
            ] {
                assert!(block.contains(section), "{heading} is missing {section}");
            }
        }
    }
    assert!(entries >= 22, "expected every LIM entry, found {entries}");
    assert!(
        deviations >= 1,
        "expected the recorded deviations, found {deviations}"
    );
}

//! Structural checks for shipped documentation.

use std::collections::HashSet;
use std::path::Path;

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("{} could not be read: {error}", path.display());
    })
}

#[test]
fn public_support_matrices_have_the_required_tiers() {
    for document in ["README.md", "vision.md", "docs/release-notes-template.md"] {
        let text = read(document);
        for (integration, expected) in [
            ("Claude Code", "Production"),
            ("OpenAI Codex CLI", "Experimental"),
            ("GitHub Copilot CLI", "Experimental"),
            ("OpenCode", "Experimental"),
        ] {
            let row = text
                .lines()
                .filter(|line| line.trim_start().starts_with('|'))
                .find_map(|line| {
                    let mut columns = line
                        .split('|')
                        .skip(1)
                        .map(|column| column.trim().trim_matches('*'));
                    let name = columns.next()?;
                    let tier = columns.next()?;
                    (name == integration).then_some(tier)
                })
                .unwrap_or_else(|| panic!("{document} has no support row for {integration}"));

            assert!(
                row.to_ascii_lowercase()
                    .starts_with(&expected.to_ascii_lowercase()),
                "{document} labels {integration} as `{row}`, expected {expected}"
            );
        }
    }
}

#[test]
fn release_notes_link_the_boundary_and_reporting_documents() {
    let text = read("docs/release-notes-template.md");
    for link in ["(../limitations.md)", "(../SECURITY.md)"] {
        assert!(text.contains(link), "release notes omit the `{link}` link");
    }
}

#[test]
fn limitation_and_deviation_entries_are_well_formed() {
    let text = read("limitations.md");
    let mut identifiers = HashSet::new();

    for block in text.split("\n### ").skip(1) {
        let heading = block.lines().next().unwrap_or_default();
        let identifier = heading.split(':').next().unwrap_or_default();
        if identifier.ends_with("NNN") {
            continue;
        }

        let sections: &[&str] = if identifier.starts_with("LIM-") {
            &[
                "**Reality:**",
                "**Impact:**",
                "**Workaround:**",
                "**Verification:**",
            ]
        } else if identifier.starts_with("DEV-") {
            &[
                "Contract:",
                "**Observed behavior:**",
                "**Impact:**",
                "**Workaround:**",
                "**Verification:**",
            ]
        } else {
            continue;
        };

        let number = identifier
            .split_once('-')
            .map(|(_, number)| number)
            .unwrap_or_default();
        assert!(
            number.len() == 3 && number.bytes().all(|byte| byte.is_ascii_digit()),
            "invalid limitation identifier `{identifier}`"
        );
        assert!(
            identifiers.insert(identifier),
            "duplicate limitation identifier `{identifier}`"
        );
        for section in sections {
            assert!(block.contains(section), "{heading} is missing {section}");
        }
    }
}

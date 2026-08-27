//! Structural checks for shipped documentation.

use std::collections::HashSet;
use std::path::Path;

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("{} could not be read: {error}", path.display());
    })
}

fn section<'a>(text: &'a str, heading: &str) -> &'a str {
    let (_, rest) = text
        .split_once(heading)
        .unwrap_or_else(|| panic!("document has no `{heading}` section"));
    rest.split("\n## ").next().unwrap_or(rest)
}

#[test]
fn public_support_matrices_have_the_required_tiers() {
    for (document, heading) in [
        ("README.md", "## Support and Security Limits"),
        ("vision.md", "## V1 Support Posture"),
        ("docs/release-notes-template.md", "## Support matrix"),
    ] {
        let text = read(document);
        let matrix = section(&text, heading);
        for (integration, expected) in [
            ("Claude Code", "Production"),
            ("OpenAI Codex CLI", "Experimental"),
            ("GitHub Copilot CLI", "Experimental"),
            ("OpenCode", "Experimental"),
        ] {
            let row = matrix
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
fn public_known_source_documents_link_the_inventory() {
    let readme = read("README.md");
    let overview = section(&readme, "### Known Source Rules");
    assert!(
        overview.contains("(docs/known-sources.md)"),
        "README overview does not link the known source inventory"
    );

    let release_notes = read("docs/release-notes-template.md");
    let overview = section(&release_notes, "## Known Source Rules");
    let overview = overview.split_whitespace().collect::<Vec<_>>().join(" ");
    for marker in [
        "secret-like names",
        "credential-bearing URLs",
        "recognized credential document rules",
        "full JSON5 grammar",
        "bounded",
        "selected by default",
        "known-sources.md",
        "LIM-023",
    ] {
        assert!(
            overview.contains(marker),
            "release-note overview omits `{marker}`"
        );
    }
}

#[test]
fn known_source_inventory_describes_bounded_advisory_rules() {
    let text = read("docs/known-sources.md");
    for disclosure in [
        "bounded location",
        "non-empty string",
        "additively",
        "Default paths persist",
        "override paths",
        "JSON5",
        "Dynamic names",
        "keychains",
        "LIM-023",
    ] {
        assert!(
            text.contains(disclosure),
            "Known Source inventory omits `{disclosure}`"
        );
    }
}

#[test]
fn completed_setup_contract_work_has_no_temporary_gap_entries() {
    let limitations = read("limitations.md");
    assert!(!limitations.contains("### LIM-011:"));
    assert!(!limitations.contains("### DEV-003:"));
    assert!(!limitations.contains("### DEV-004:"));

    let traceability = read("docs/traceability.md");
    for requirement in [
        "SET-002", "SET-006", "SET-016", "SET-018", "SET-019", "SET-020", "TST-003",
    ] {
        let row = traceability
            .lines()
            .find(|line| line.starts_with(&format!("| {requirement} |")))
            .unwrap_or_else(|| panic!("traceability has no row for {requirement}"));
        assert!(
            row.ends_with("| covered |"),
            "{requirement} remains open: {row}"
        );
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

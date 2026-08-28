//! Pure setup rendering. Presentation is a maintained design baseline, not a
//! compatibility contract.

use super::describe;
use super::enrollment::Item;
use super::integrations::Row;
use crate::integration::Detection;
use crate::integration::hooks_json::Installed;
use crate::sanitize;

pub(super) fn enrollment(items: &[Item]) -> String {
    let mut lines = vec![String::new()];
    if items.iter().all(|item| !item.visible()) {
        lines.push("  (no candidates found)".to_string());
        return lines.join("\n");
    }

    for (row, item) in items.iter().filter(|item| item.visible()).enumerate() {
        let members: Vec<_> = item.visible_members().collect();
        let marker = match (&item.problem, item.selected) {
            (Some(_), _) => "!",
            (None, true) => "x",
            (None, false) => " ",
        };
        let enrolled = if item.enrolled { " (enrolled)" } else { "" };
        let description = if members.len() == 1 {
            describe(&members[0].source)
        } else {
            format!("Same current value ({} sources)", members.len())
        };
        lines.push(format!(
            "  {:>2} [{marker}] {description}{enrolled}",
            row + 1
        ));

        if members.len() > 1 {
            for member in members {
                let enrolled = if member.enrolled { " (enrolled)" } else { "" };
                lines.push(format!("        - {}{enrolled}", describe(&member.source)));
            }
        }
        if !item.detail.is_empty() {
            lines.push(format!("        {}", item.detail));
        }
        let rules = item.rules();
        if !rules.is_empty() {
            let names: Vec<_> = rules.iter().map(|rule| rule.display()).collect();
            lines.push(format!("        rules: {}", names.join(", ")));
        }
        if let Some(collisions) = &item.collisions {
            lines.push(format!("        collision: {}", collisions.describe()));
        }
    }
    lines.join("\n")
}

pub(super) fn enrollment_actions(row_count: usize) -> String {
    let mut lines = vec!["Choose an action:".to_string()];
    if row_count > 0 {
        lines.extend([
            "  [1 3]   toggle row(s)".to_string(),
            "  [a]     select all".to_string(),
            "  [n]     select none".to_string(),
        ]);
    }
    lines.extend([
        "  [e]     add env".to_string(),
        "  [k]     add dotenv key".to_string(),
        "  [w]     add wildcard file".to_string(),
        "  [j]     add JSON field".to_string(),
        "  [Enter] save".to_string(),
        "  [s]     skip".to_string(),
        "  [q]     quit".to_string(),
    ]);
    lines.join("\n")
}

pub(super) fn integrations(rows: &[Row]) -> String {
    let mut lines = vec![String::new()];
    for (index, row) in rows.iter().enumerate() {
        let harness = row.inspection.harness;
        lines.push(format!(
            "  {:>2} [{}] {} ({}) - {}, {}",
            index + 1,
            if row.selected { "x" } else { " " },
            harness.label(),
            harness.tier_label(),
            match row.inspection.detection {
                Detection::Detected => "detected",
                Detection::NotDetected => "not detected",
            },
            describe_installed(&row.inspection.installed)
        ));
        lines.push(format!(
            "        file: {}",
            sanitize::path(&row.inspection.artifact_path)
        ));
        for conflict in &row.inspection.conflicts {
            lines.push(format!(
                "        other hook on the same event: {} ({})",
                conflict.command,
                if conflict.approved {
                    "approved"
                } else {
                    "needs review"
                }
            ));
        }
    }
    lines.push(
        "  Installation is not proof of protection; run `contextveil doctor` to check it."
            .to_string(),
    );
    lines.join("\n")
}

pub(super) fn integration_actions(row_count: usize) -> String {
    let mut lines = vec!["Choose an action:".to_string()];
    if row_count > 0 {
        lines.push("  [1 3]   toggle row(s)".to_string());
    }
    lines.extend([
        "  [Enter] apply".to_string(),
        "  [s]     skip".to_string(),
        "  [q]     quit".to_string(),
    ]);
    lines.join("\n")
}

fn describe_installed(installed: &Installed) -> &'static str {
    match installed {
        Installed::Absent => "not installed",
        Installed::Current => "installed",
        Installed::Outdated { .. } => "installed, pointing at another binary",
        Installed::Modified { .. } => "installed entry was modified by hand",
        Installed::Unreadable => "host file is not valid JSON",
        Installed::Unexpected => "host file has an unexpected shape",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::integration::hooks_json::Installed;
    use crate::integration::{Harness, Inspection};
    use crate::setup::collision::Collisions;
    use crate::setup::enrollment::Member;
    use crate::setup::integrations::Row;
    use crate::setup::known_source::Rule;
    use crate::source::SourceRef;

    fn member(source: SourceRef, enrolled: bool, rules: Vec<Rule>) -> Member {
        Member {
            source,
            rules,
            enrolled,
            suppressed: false,
        }
    }

    fn item(members: Vec<Member>, detail: &str, selected: bool) -> Item {
        let enrolled = members.iter().any(|member| member.enrolled);
        Item {
            members,
            enrolled,
            selected,
            selection_touched: false,
            detail: detail.to_string(),
            problem: None,
            value: Some("SSCANARY-RENDER-MUST-NOT-APPEAR".to_string()),
            resolved: true,
            wildcard_values: Vec::new(),
            collisions: None,
        }
    }

    #[test]
    fn setup_rendering_matches_the_maintained_design_baseline() {
        let mut grouped = item(
            vec![
                member(
                    SourceRef::Env {
                        name: "PRIMARY_TOKEN".to_string(),
                    },
                    true,
                    vec![Rule::SecretLikeName],
                ),
                member(
                    SourceRef::DotenvKey {
                        entered: ".env.local".to_string(),
                        path: PathBuf::from("/project/.env.local"),
                        key: "SECONDARY_TOKEN".to_string(),
                    },
                    false,
                    vec![Rule::SecretLikeName, Rule::CredentialBearingUrl],
                ),
            ],
            "ab********yz (20 characters)",
            true,
        );
        grouped.collisions = Some(Collisions {
            total: 1,
            files: vec![("README\\nHOSTILE.md".to_string(), 1)],
        });
        let unavailable = Item {
            members: vec![member(
                SourceRef::Json {
                    entered: "broken\\efile.json".to_string(),
                    path: PathBuf::from("/project/broken.json"),
                    pointer: "/token".to_string(),
                },
                true,
                Vec::new(),
            )],
            enrolled: true,
            selected: true,
            selection_touched: false,
            detail: "unavailable: malformed JSON source".to_string(),
            problem: Some("malformed JSON source".to_string()),
            value: None,
            resolved: false,
            wildcard_values: Vec::new(),
            collisions: None,
        };
        let wildcard = Item {
            members: vec![member(
                SourceRef::DotenvAll {
                    entered: ".env.shared".to_string(),
                    path: PathBuf::from("/project/.env.shared"),
                },
                false,
                Vec::new(),
            )],
            enrolled: false,
            selected: true,
            selection_touched: true,
            detail: "3 current key(s)".to_string(),
            problem: None,
            value: None,
            resolved: true,
            wildcard_values: Vec::new(),
            collisions: None,
        };
        let integration_rows = [
            Row {
                inspection: Inspection {
                    harness: Harness::Claude,
                    artifact_path: PathBuf::from("/home/user/.claude/settings.json"),
                    detection: Detection::Detected,
                    installed: Installed::Current,
                    conflicts: Vec::new(),
                    hook_executable: None,
                    hook_timeout: Some(5),
                    disabled_by_policy: false,
                },
                selected: true,
                installed: true,
            },
            Row {
                inspection: Inspection {
                    harness: Harness::Codex,
                    artifact_path: PathBuf::from("/home/user/.codex/hooks.json"),
                    detection: Detection::NotDetected,
                    installed: Installed::Absent,
                    conflicts: Vec::new(),
                    hook_executable: None,
                    hook_timeout: None,
                    disabled_by_policy: false,
                },
                selected: false,
                installed: false,
            },
        ];

        let actual = format!(
            "Global sources (this machine)\n{}\n{}\n\nProject sources (this project)\n{}\n{}\n\nIntegrations\n{}\n{}\n\nNo-row action state\n{}\n{}\n",
            enrollment(&[grouped, unavailable, wildcard]),
            enrollment_actions(3),
            enrollment(&[]),
            enrollment_actions(0),
            integrations(&integration_rows),
            integration_actions(integration_rows.len()),
            enrollment(&[]),
            enrollment_actions(0),
        );
        assert!(!actual.contains("SSCANARY-RENDER-MUST-NOT-APPEAR"));
        assert_eq!(
            actual,
            include_str!("../../tests/snapshots/setup-rendering.txt")
        );
    }
}

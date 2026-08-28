//! Enrollment state kept separate from terminal orchestration and rendering.

use crate::source::SourceRef;

use super::collision::Collisions;
use super::known_source::Rule;

/// One source represented by a selectable equal-value group.
pub(super) struct Member {
    pub(super) source: SourceRef,
    pub(super) rules: Vec<Rule>,
    pub(super) enrolled: bool,
    pub(super) suppressed: bool,
}

/// One enrollment choice. Equal-value members are enrolled together so future
/// rotations of every alias remain protected.
pub(super) struct Item {
    pub(super) members: Vec<Member>,
    pub(super) enrolled: bool,
    pub(super) selected: bool,
    pub(super) selection_touched: bool,
    pub(super) detail: String,
    pub(super) problem: Option<String>,
    /// Secret-bearing grouping/collision state. Rendering must never inspect it.
    pub(super) value: Option<String>,
    pub(super) resolved: bool,
    pub(super) wildcard_values: Vec<String>,
    pub(super) collisions: Option<Collisions>,
}

impl Item {
    pub(super) fn is_wildcard(&self) -> bool {
        matches!(self.members[0].source, SourceRef::DotenvAll { .. })
    }

    pub(super) fn is_selected_wildcard(&self) -> bool {
        self.is_wildcard() && self.selected
    }

    pub(super) fn visible(&self) -> bool {
        self.members.iter().any(|member| !member.suppressed)
    }

    pub(super) fn visible_members(&self) -> impl Iterator<Item = &Member> {
        self.members.iter().filter(|member| !member.suppressed)
    }

    pub(super) fn any_enrolled(&self) -> bool {
        self.enrolled
    }

    pub(super) fn any_selected(&self) -> bool {
        self.selected && self.visible()
    }

    pub(super) fn rules(&self) -> Vec<Rule> {
        let mut rules: Vec<Rule> = self
            .visible_members()
            .flat_map(|member| member.rules.iter().copied())
            .collect();
        rules.sort_unstable();
        rules.dedup();
        rules
    }
}

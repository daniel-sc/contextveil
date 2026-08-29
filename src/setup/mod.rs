//! The unified interactive setup workflow.
//!
//! `CLI-001` makes `setup` the only configuration workflow and `SET-001` fixes
//! its phases: global enrollment, project enrollment, integration selection and
//! removal, then offline verification. Each configuration phase presents
//! existing entries as selected, offers a no-change path, and commits only after
//! its own explicit confirmation (`SET-014`).
//!
//! Nothing here prints a complete candidate value (`SET-010`) or persists one
//! (`SEC-004`), and every untrusted path, key, and preview is rendered through
//! `crate::sanitize` (`SEC-006`).

pub mod collision;
pub mod credential_url;
pub mod discovery;
pub mod enrollment;
pub mod integrations;
pub mod known_source;
pub mod preview;
pub mod render;
pub mod ui;
pub mod vocabulary;
pub mod write;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::cli::Exit;
use crate::config::{self, Config, ConfigError, Load};
use crate::paths::{self, PROJECT_CONFIG_FILENAME};
use crate::sanitize;
use crate::secret::SourceId;
use crate::source::{Environment, Resolution, Resolver, SourceRef, Unresolved};

use discovery::{Discovered, State};
use enrollment::{Item, Member};
use known_source::Rule;
use ui::{Cancelled, Terminal};

/// Which registry a phase edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Global,
    Project,
}

impl Scope {
    fn title(self) -> &'static str {
        match self {
            Scope::Global => "Global sources (this machine)",
            Scope::Project => "Project sources (this project)",
        }
    }
}

/// Runs the complete setup workflow.
///
/// `current_directory` is where the user invoked the command; the project root
/// is selected from it by `CFG-003`.
pub fn run(
    terminal: &mut Terminal<'_>,
    environment: &Environment,
    current_directory: &Path,
    executable: Option<&Path>,
) -> Exit {
    let home = environment.home();
    let Some(global_path) = config::global_config_path(environment) else {
        terminal.line(
            "contextveil: the configuration location could not be determined. Set HOME or \
             XDG_CONFIG_HOME.",
        );
        return Exit::Failure;
    };
    let project_root = paths::setup_project_root(current_directory);
    let project_path = project_root.join(PROJECT_CONFIG_FILENAME);

    terminal.line("ContextVeil setup");
    terminal.line("Complete values are never shown, stored, or sent anywhere.");
    terminal.blank();

    // `SET-001`: both files are parsed before any phase runs, so an invalid file
    // stops setup before it can change anything (`CFG-014`).
    let global = match preflight(terminal, &global_path, home.as_deref()) {
        Ok(config) => config,
        Err(exit) => return exit,
    };
    let project = match preflight(terminal, &project_path, home.as_deref()) {
        Ok(config) => config,
        Err(exit) => return exit,
    };

    let project_files = discovery::project_files(&project_root);

    // Discover both scopes before presenting either phase. Candidate Groups stay
    // phase-local, while collision exclusions can account for aliases in both
    // scopes (`SET-011`, `SET-016`).
    let (global_items, global_notices) = build_items(
        Scope::Global,
        &global,
        &project_root,
        environment,
        home.as_deref(),
        current_directory,
        &project_files,
    );
    let (project_items, project_notices) = build_items(
        Scope::Project,
        &project,
        &project_root,
        environment,
        home.as_deref(),
        current_directory,
        &project_files,
    );
    let mut aliases = alias_inventory([
        (Scope::Global, global_items.as_slice()),
        (Scope::Project, project_items.as_slice()),
    ]);

    let global_result = enrollment_phase(
        terminal,
        Scope::Global,
        &global,
        global_items,
        global_notices,
        EnrollmentContext {
            config_path: &global_path,
            project_root: &project_root,
            environment,
            home: home.as_deref(),
            aliases: &mut aliases,
        },
    );
    let global_sources = match global_result {
        PhaseResult::Kept(sources) | PhaseResult::Saved(sources) => sources,
        PhaseResult::Stopped(exit) => return exit,
    };

    let project_result = enrollment_phase(
        terminal,
        Scope::Project,
        &project,
        project_items,
        project_notices,
        EnrollmentContext {
            config_path: &project_path,
            project_root: &project_root,
            environment,
            home: home.as_deref(),
            aliases: &mut aliases,
        },
    );
    let project_sources = match project_result {
        PhaseResult::Kept(sources) | PhaseResult::Saved(sources) => sources,
        // `SET-014`: a completed global phase stays committed.
        PhaseResult::Stopped(exit) => return exit,
    };

    // `CFG-003`: the project file always exists after setup, even when empty.
    if !project_path.exists()
        && let Err(error) = write::write(&project_path, &project_sources, false)
    {
        terminal.line(&format!(
            "contextveil: `{}` could not be written because {}.",
            sanitize::path(&project_path),
            error.reason()
        ));
        return Exit::Failure;
    }

    match integrations::phase(
        terminal,
        environment,
        home.as_deref(),
        &global_path,
        executable,
    ) {
        Ok(()) => {}
        Err(exit) => return exit,
    }

    verification_phase(
        terminal,
        environment,
        &project_root,
        &global_sources,
        &project_sources,
    )
}

/// Loads one configuration file before any phase runs.
fn preflight(
    terminal: &mut Terminal<'_>,
    path: &Path,
    home: Option<&Path>,
) -> Result<Config, Exit> {
    match config::load(path, home) {
        Load::Valid(config) => Ok(config),
        Load::Missing => Ok(Config::default()),
        Load::Invalid(error) => {
            report_invalid(terminal, &error);
            Err(Exit::Failure)
        }
    }
}

/// `CFG-014`: show where the problem is and change nothing.
fn report_invalid(terminal: &mut Terminal<'_>, error: &ConfigError) {
    terminal.line(&format!(
        "contextveil: `{}` is not a valid ContextVeil configuration: {}.",
        sanitize::path(&error.path),
        error.kind.reason()
    ));
    terminal.line("Setup made no change. Repair or remove the file and run setup again.");
}

enum PhaseResult {
    /// The user chose the no-change path.
    Kept(Vec<SourceRef>),
    Saved(Vec<SourceRef>),
    Stopped(Exit),
}

struct EnrollmentContext<'a> {
    config_path: &'a Path,
    project_root: &'a Path,
    environment: &'a Environment,
    home: Option<&'a Path>,
    aliases: &'a mut AliasInventory,
}

/// One enrollment phase.
fn enrollment_phase(
    terminal: &mut Terminal<'_>,
    scope: Scope,
    existing: &Config,
    mut items: Vec<Item>,
    notices: Vec<known_source::Notice>,
    mut context: EnrollmentContext<'_>,
) -> PhaseResult {
    refresh_items(scope, &mut items, &mut context);

    loop {
        terminal.line(scope.title());
        terminal.line(&format!("  file: {}", sanitize::path(context.config_path)));
        for notice in &notices {
            terminal.line(&format!(
                "  unavailable: {} ({})",
                notice.display, notice.reason
            ));
        }
        render(terminal, &items);
        render_actions(terminal, visible_count(&items));
        let answer = match terminal.ask(">") {
            Ok(answer) => answer,
            Err(Cancelled) => return cancelled(terminal),
        };

        match answer.trim() {
            "" => {
                if let Some(blocker) = blocking_item(&items) {
                    terminal.line(&format!(
                        "Cannot save: {blocker} must be repaired or deselected first."
                    ));
                    continue;
                }
                let selected = selected_sources(&items);
                return match write::write(context.config_path, &selected, scope == Scope::Global) {
                    Ok(changed) => {
                        terminal.line(if changed {
                            "Saved."
                        } else {
                            "No change; the file already matches."
                        });
                        terminal.blank();
                        PhaseResult::Saved(selected)
                    }
                    Err(error) => {
                        terminal.line(&format!(
                            "contextveil: `{}` could not be written because {}.",
                            sanitize::path(context.config_path),
                            error.reason()
                        ));
                        PhaseResult::Stopped(Exit::Failure)
                    }
                };
            }
            "s" => {
                for item in &mut items {
                    if item.is_wildcard() {
                        item.selected = item.enrolled;
                    }
                }
                context.aliases.sync_wildcards(scope, &items);
                terminal.line("Skipped; this file is unchanged.");
                terminal.blank();
                return PhaseResult::Kept(existing.sources.clone());
            }
            "q" => return cancelled(terminal),
            "a" => {
                for item in &mut items {
                    if item.problem.is_none() {
                        item.selected = true;
                        item.selection_touched = true;
                    }
                }
                refresh_items(scope, &mut items, &mut context);
            }
            "n" => {
                for item in &mut items {
                    item.selected = false;
                    item.selection_touched = true;
                }
                refresh_items(scope, &mut items, &mut context);
            }
            "e" | "k" | "w" | "j" | "p" => {
                match add_manual(terminal, answer.trim(), scope, &mut items, &mut context) {
                    Ok(()) => {}
                    Err(Cancelled) => return cancelled(terminal),
                }
            }
            selection => {
                toggle(terminal, &mut items, selection);
                refresh_items(scope, &mut items, &mut context);
            }
        }
    }
}

fn cancelled(terminal: &mut Terminal<'_>) -> PhaseResult {
    // `CLI-004`: cancellation returns nonzero. Phases already committed stay.
    terminal.line("Setup cancelled. Nothing further was changed.");
    PhaseResult::Stopped(Exit::Failure)
}

#[derive(Default)]
struct AliasInventory {
    sources: HashMap<String, Vec<PathBuf>>,
    wildcards: Vec<WildcardAliases>,
}

struct WildcardAliases {
    scope: Scope,
    path: PathBuf,
    values: Vec<String>,
}

/// Renders a count with a correctly pluralized noun.
fn count(number: usize, singular: &str, plural: &str) -> String {
    if number == 1 {
        format!("{number} {singular}")
    } else {
        format!("{number} {plural}")
    }
}

/// Sanitized, value-free description of a source reference.
fn describe(source: &SourceRef) -> String {
    match source {
        SourceRef::Env { name } => format!("env {}", sanitize::text(name)),
        SourceRef::DotenvKey { entered, key, .. } => format!(
            "dotenv {} key {}",
            sanitize::text(entered),
            sanitize::text(key)
        ),
        SourceRef::DotenvAll { entered, .. } => {
            format!("dotenv {} (every key)", sanitize::text(entered))
        }
        SourceRef::Json {
            entered, pointer, ..
        } => format!(
            "json {} pointer {}",
            sanitize::text(entered),
            sanitize::text(pointer)
        ),
        SourceRef::Properties { entered, key, .. } => format!(
            "properties {} key {}",
            sanitize::text(entered),
            sanitize::text(key)
        ),
    }
}

fn build_items(
    scope: Scope,
    existing: &Config,
    project_root: &Path,
    environment: &Environment,
    home: Option<&Path>,
    invocation_directory: &Path,
    project_files: &discovery::ProjectFiles,
) -> (Vec<Item>, Vec<known_source::Notice>) {
    let mut resolver = Resolver::new();
    let mut items: Vec<Item> = Vec::new();

    // `CFG-015`: existing valid enrollment is preserved by default, including
    // sources that are merely unresolved right now.
    for source in &existing.sources {
        merge_item(
            &mut items,
            item_for(source.clone(), true, Vec::new(), &mut resolver, environment),
        );
    }

    let mut known: HashSet<SourceId> = items
        .iter()
        .flat_map(|item| &item.members)
        .map(|member| member.source.id())
        .collect();
    let mut discovered_known = match scope {
        Scope::Global => known_source::machine(environment, home, invocation_directory),
        Scope::Project => known_source::project(project_root, project_files),
    };
    for source in discovered_known.sources {
        let id = source.id();
        let rules = discovered_known.rules.remove(&id).unwrap_or_default();
        if known.insert(id.clone()) {
            merge_item(
                &mut items,
                item_for(source, false, rules, &mut resolver, environment),
            );
        } else {
            add_rules(&mut items, &id, rules);
        }
    }

    if scope == Scope::Global {
        // `SET-002`: the current process environment is inspected automatically.
        for name in environment_candidates(environment) {
            let source = SourceRef::Env { name };
            let id = source.id();
            let candidate = automatic_item_for(source, &mut resolver, environment);
            if known.insert(id.clone()) {
                merge_item(&mut items, candidate);
            } else {
                let rules = candidate.members[0].rules.clone();
                add_rules(&mut items, &id, rules);
            }
        }
    }

    let discovered = match scope {
        // `SET-004`: bounded probe locations only.
        Scope::Global => home.map(discovery::global_dotenv_files).unwrap_or_default(),
        // `SET-003`: recursive project discovery.
        Scope::Project => project_files.dotenv.clone(),
    };
    for file in &discovered {
        for candidate in file_candidates(file, &mut resolver, environment) {
            let id = candidate.members[0].source.id();
            if known.insert(id.clone()) {
                merge_item(&mut items, candidate);
            } else {
                let rules = candidate.members[0].rules.clone();
                add_rules(&mut items, &id, rules);
            }
        }
    }

    sort_initial_items(&mut items);
    (items, discovered_known.notices)
}

/// Establishes the two initial display tiers and each group's representative.
fn sort_initial_items(items: &mut [Item]) {
    for item in items.iter_mut() {
        item.members.sort_by_key(|member| member.source.id());
    }
    items.sort_by(|left, right| {
        right
            .any_enrolled()
            .cmp(&left.any_enrolled())
            .then_with(|| {
                left.members[0]
                    .source
                    .id()
                    .cmp(&right.members[0].source.id())
            })
    });
}

/// Name-gated and credential-bearing URL environment variables, in stable order.
fn environment_candidates(environment: &Environment) -> Vec<String> {
    let mut names: Vec<String> = environment
        .names()
        .filter(|name| {
            vocabulary::gating_term(name).is_some()
                || environment
                    .get_str(name)
                    .is_some_and(|value| credential_url::is_credential_bearing(value.trim()))
        })
        .map(str::to_string)
        .collect();
    names.sort();
    names
}

/// Candidates offered for one discovered dotenv file.
fn file_candidates(
    file: &Discovered,
    resolver: &mut Resolver,
    environment: &Environment,
) -> Vec<Item> {
    let (Some(entered), State::Available(dotenv)) = (&file.entered, &file.state) else {
        return Vec::new();
    };
    dotenv
        .entries()
        .filter(|(key, value)| {
            !value.trim().is_empty()
                && (vocabulary::gating_term(key).is_some()
                    || credential_url::is_credential_bearing(value.trim()))
        })
        .map(|(key, _)| SourceRef::DotenvKey {
            entered: entered.clone(),
            path: file.path.clone(),
            key: key.to_string(),
        })
        .map(|source| automatic_item_for(source, resolver, environment))
        .collect()
}

fn automatic_item_for(
    source: SourceRef,
    resolver: &mut Resolver,
    environment: &Environment,
) -> Item {
    let mut item = item_for(source.clone(), false, Vec::new(), resolver, environment);
    let rules = admission_rules(&source, item.value.as_deref());
    if item.problem.is_none() && !rules.is_empty() {
        item.selected = true;
    }
    item.members[0].rules = rules;
    item
}

fn item_for(
    source: SourceRef,
    enrolled: bool,
    rules: Vec<Rule>,
    resolver: &mut Resolver,
    environment: &Environment,
) -> Item {
    let automatically_admitted = !rules.is_empty();
    let mut item = Item {
        members: vec![Member {
            source: source.clone(),
            rules,
            enrolled,
            suppressed: false,
        }],
        enrolled,
        selected: enrolled || automatically_admitted,
        selection_touched: false,
        detail: String::new(),
        problem: None,
        value: None,
        resolved: false,
        wildcard_values: Vec::new(),
        collisions: None,
    };

    match resolver.resolve(&source, environment) {
        Resolution::Resolved(secrets) => {
            item.resolved = true;
            let value = if matches!(source, SourceRef::DotenvAll { .. }) {
                None
            } else {
                secrets.first().map(|secret| secret.value.clone())
            };
            item.detail = match &value {
                Some(value) => preview::describe(value),
                None => format!("{} current keys", secrets.len()),
            };
            if matches!(source, SourceRef::DotenvAll { .. }) {
                item.detail = format!("{} current key(s)", secrets.len());
                item.wildcard_values = secrets.into_iter().map(|secret| secret.value).collect();
            }
            item.value = value;
            if automatically_admitted
                && item
                    .value
                    .as_deref()
                    .is_some_and(credential_url::is_credential_bearing)
                && !item.members[0].rules.contains(&Rule::CredentialBearingUrl)
            {
                item.members[0].rules.push(Rule::CredentialBearingUrl);
            }
        }
        Resolution::Unresolved { why, .. } => {
            item.detail = format!("unresolved: {}", unresolved_reason(why));
        }
        Resolution::Malfunction { why, .. } => {
            // `SET-013`: an enrolled malformed or unreadable source must be
            // repaired or removed before setup can complete, so it stays
            // selected and blocks saving until the user deselects it.
            item.problem = Some(why.reason());
            item.detail = format!("unavailable: {}", why.reason());
            item.selected = enrolled;
        }
    }
    item
}

fn admission_rules(source: &SourceRef, value: Option<&str>) -> Vec<Rule> {
    let mut rules = Vec::new();
    let name = match source {
        SourceRef::Env { name }
        | SourceRef::DotenvKey { key: name, .. }
        | SourceRef::Properties { key: name, .. } => Some(name.as_str()),
        SourceRef::DotenvAll { .. } | SourceRef::Json { .. } => None,
    };
    if name.and_then(vocabulary::gating_term).is_some() {
        rules.push(Rule::SecretLikeName);
    }
    if name.is_some() && value.is_some_and(credential_url::is_credential_bearing) {
        rules.push(Rule::CredentialBearingUrl);
    }
    rules
}

fn add_rules(items: &mut [Item], source: &SourceId, rules: Vec<Rule>) {
    for member in items.iter_mut().flat_map(|item| &mut item.members) {
        if member.source.id() == *source {
            member.rules.extend(rules);
            member.rules.sort_unstable();
            member.rules.dedup();
            return;
        }
    }
}

fn merge_item(items: &mut Vec<Item>, mut incoming: Item) {
    if let Some(value) = incoming.value.as_ref()
        && let Some(existing) = items
            .iter_mut()
            .find(|item| item.value.as_ref() == Some(value))
    {
        existing.enrolled |= incoming.enrolled;
        existing.selected |= incoming.selected;
        existing.selection_touched |= incoming.selection_touched;
        existing.members.append(&mut incoming.members);
        return;
    }
    items.push(incoming);
}

fn alias_inventory<'a>(phases: impl IntoIterator<Item = (Scope, &'a [Item])>) -> AliasInventory {
    let mut aliases = AliasInventory::default();
    for (scope, items) in phases {
        for item in items {
            if let Some(value) = &item.value {
                for member in &item.members {
                    add_alias(&mut aliases.sources, value, member.source.file());
                }
            }
            if item.is_selected_wildcard() {
                aliases.wildcards.push(WildcardAliases {
                    scope,
                    path: item.members[0]
                        .source
                        .file()
                        .expect("a wildcard always has a file")
                        .to_path_buf(),
                    values: item.wildcard_values.clone(),
                });
            }
        }
    }
    aliases
}

impl AliasInventory {
    fn sync_wildcards(&mut self, scope: Scope, items: &[Item]) {
        self.wildcards.retain(|wildcard| wildcard.scope != scope);
        self.wildcards
            .extend(
                items
                    .iter()
                    .filter(|item| item.is_selected_wildcard())
                    .map(|item| WildcardAliases {
                        scope,
                        path: item.members[0]
                            .source
                            .file()
                            .expect("a wildcard always has a file")
                            .to_path_buf(),
                        values: item.wildcard_values.clone(),
                    }),
            );
    }

    fn source_files(&self, value: &str) -> Vec<PathBuf> {
        let mut files = self.sources.get(value).cloned().unwrap_or_default();
        for wildcard in &self.wildcards {
            if wildcard.values.iter().any(|known| known == value)
                && !files.iter().any(|known| known == &wildcard.path)
            {
                files.push(wildcard.path.clone());
            }
        }
        files
    }
}

fn add_alias(aliases: &mut HashMap<String, Vec<PathBuf>>, value: &str, file: Option<&Path>) {
    let Some(file) = file else { return };
    let files = aliases.entry(value.to_string()).or_default();
    if !files.iter().any(|known| known == file) {
        files.push(file.to_path_buf());
    }
}

fn unresolved_reason(why: Unresolved) -> &'static str {
    why.reason()
}

/// Runs collision analysis for every resolvable candidate (`SET-011`).
fn annotate_collisions(items: &mut [Item], project_root: &Path, aliases: &AliasInventory) {
    let values: Vec<&str> = items
        .iter()
        .filter_map(|item| item.value.as_deref())
        .collect();
    let source_files: Vec<Vec<PathBuf>> = values
        .iter()
        .map(|value| aliases.source_files(value))
        .collect();
    let subjects: Vec<collision::Subject<'_>> = values
        .iter()
        .zip(&source_files)
        .map(|(value, source_files)| collision::Subject {
            value,
            source_files,
        })
        .collect();
    if subjects.is_empty() {
        return;
    }
    let reports = collision::analyze(project_root, &subjects);

    let mut report = reports.into_iter();
    for item in items.iter_mut() {
        if item.value.is_none() {
            continue;
        }
        let Some(collisions) = report.next() else {
            break;
        };
        item.collisions = None;
        if !collisions.is_empty() {
            // `SET-007`: a colliding candidate stays visible but unselected,
            // unless it is already enrolled (`CFG-015`).
            if !item.enrolled && !item.selection_touched {
                item.selected = false;
            }
            item.collisions = Some(collisions);
        } else if !item.enrolled && !item.selection_touched {
            item.selected = true;
        }
    }
}

fn render(terminal: &mut Terminal<'_>, items: &[Item]) {
    for line in render::enrollment(items).lines() {
        terminal.line(line);
    }
}

fn render_actions(terminal: &mut Terminal<'_>, row_count: usize) {
    for line in render::enrollment_actions(row_count).lines() {
        terminal.line(line);
    }
}

/// The first selected source that blocks saving (`SET-013`).
fn blocking_item(items: &[Item]) -> Option<String> {
    items
        .iter()
        .find(|item| item.visible() && item.any_selected() && item.problem.is_some())
        .and_then(|item| item.visible_members().next())
        .map(|member| describe(&member.source))
}

fn selected_sources(items: &[Item]) -> Vec<SourceRef> {
    let mut selected: Vec<SourceRef> = items
        .iter()
        .filter(|item| item.selected && item.visible())
        .flat_map(|item| {
            item.members
                .iter()
                .filter(|member| !member.suppressed)
                .map(|member| member.source.clone())
        })
        .collect();
    selected.sort_by_key(SourceRef::id);
    selected
}

fn toggle(terminal: &mut Terminal<'_>, items: &mut [Item], selection: &str) {
    let visible: Vec<usize> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| item.visible().then_some(index))
        .collect();
    let mut unknown = Vec::new();
    for token in selection.split_whitespace() {
        match token.parse::<usize>() {
            Ok(number) if number >= 1 && number <= visible.len() => {
                let item = &mut items[visible[number - 1]];
                if item.problem.is_some() && !item.selected {
                    terminal.line(&format!(
                        "  {} is unavailable and cannot be selected.",
                        describe(&item.members[0].source)
                    ));
                    continue;
                }
                item.selected = !item.selected;
                item.selection_touched = true;
            }
            _ => unknown.push(sanitize::text(token)),
        }
    }
    if !unknown.is_empty() {
        terminal.line(&format!("  Not a choice: {}", unknown.join(", ")));
    }
}

fn visible_count(items: &[Item]) -> usize {
    items.iter().filter(|item| item.visible()).count()
}

fn update_suppression(items: &mut [Item], aliases: &AliasInventory) {
    let selected_wildcards: Vec<&Path> = aliases
        .wildcards
        .iter()
        .map(|wildcard| wildcard.path.as_path())
        .collect();
    for item in items {
        if item.is_wildcard() {
            continue;
        }
        for member in &mut item.members {
            member.suppressed = !member.enrolled
                && matches!(
                    &member.source,
                    SourceRef::DotenvKey { path, .. }
                        if selected_wildcards.iter().any(|wildcard| *wildcard == path)
                );
        }
    }
}

fn refresh_items(scope: Scope, items: &mut [Item], context: &mut EnrollmentContext<'_>) {
    context.aliases.sync_wildcards(scope, items);
    update_suppression(items, context.aliases);
    annotate_collisions(items, context.project_root, context.aliases);
}

/// Manual entry of a source (`SET-005`).
fn add_manual(
    terminal: &mut Terminal<'_>,
    kind: &str,
    scope: Scope,
    items: &mut Vec<Item>,
    context: &mut EnrollmentContext<'_>,
) -> Result<(), Cancelled> {
    let base = context.config_path.parent().unwrap_or(Path::new("."));
    let source = match kind {
        "e" => {
            let name = terminal.ask("Environment variable name:")?;
            if name.trim().is_empty() {
                terminal.line("  No name entered.");
                return Ok(());
            }
            SourceRef::Env {
                name: name.trim().to_string(),
            }
        }
        "k" | "w" => {
            let entered = terminal.ask("Dotenv file path:")?;
            let entered = entered.trim().to_string();
            if entered.is_empty() {
                terminal.line("  No path entered.");
                return Ok(());
            }
            let path = match paths::expand(&entered, base, context.home) {
                Ok(path) => path,
                Err(problem) => {
                    terminal.line(&format!("  That path {}.", problem.reason()));
                    return Ok(());
                }
            };
            if kind == "k" {
                let key = terminal.ask("Key name:")?;
                if key.trim().is_empty() {
                    terminal.line("  No key entered.");
                    return Ok(());
                }
                SourceRef::DotenvKey {
                    entered,
                    path,
                    key: key.trim().to_string(),
                }
            } else {
                // `SET-009`: wildcard enrollment needs its own confirmation.
                terminal.line(
                    "  Wildcard enrollment protects every current and future key in that file.",
                );
                terminal.line(
                    "  Short, common, and future values are enrolled without individual review, \
                     and a common value can replace unrelated text.",
                );
                if !terminal.confirm("  Enroll every key in this file?", false)? {
                    terminal.line("  Not added.");
                    return Ok(());
                }
                SourceRef::DotenvAll { entered, path }
            }
        }
        "j" => {
            let entered = terminal.ask("JSON file path:")?;
            let entered = entered.trim().to_string();
            if entered.is_empty() {
                terminal.line("  No path entered.");
                return Ok(());
            }
            let path = match paths::expand(&entered, base, context.home) {
                Ok(path) => path,
                Err(problem) => {
                    terminal.line(&format!("  That path {}.", problem.reason()));
                    return Ok(());
                }
            };
            let pointer = terminal.ask("JSON Pointer:")?;
            if pointer.trim().is_empty() {
                terminal.line("  No pointer entered.");
                return Ok(());
            }
            if crate::json::final_token(&pointer).is_err() {
                terminal.line(
                        "  Enter a plain RFC 6901 pointer beginning with `/`, with a non-empty final token and no wildcards.",
                    );
                return Ok(());
            }
            SourceRef::Json {
                entered,
                path,
                pointer,
            }
        }
        "p" => {
            let entered = terminal.ask("Properties file path:")?;
            let entered = entered.trim().to_string();
            if entered.is_empty() {
                terminal.line("  No path entered.");
                return Ok(());
            }
            let path = match paths::expand(&entered, base, context.home) {
                Ok(path) => path,
                Err(problem) => {
                    terminal.line(&format!("  That path {}.", problem.reason()));
                    return Ok(());
                }
            };
            let key = terminal.ask("Decoded key name:")?;
            if key.is_empty() {
                terminal.line("  No key entered.");
                return Ok(());
            }
            SourceRef::Properties { entered, path, key }
        }
        _ => return Ok(()),
    };

    if items
        .iter()
        .flat_map(|item| &item.members)
        .any(|member| member.source.id() == source.id())
    {
        terminal.line("  That source is already listed.");
        return Ok(());
    }

    let mut resolver = Resolver::new();
    let mut item = item_for(
        source,
        false,
        Vec::new(),
        &mut resolver,
        context.environment,
    );
    if item.problem.is_some() {
        terminal.line(&format!("  This source is currently {}.", item.detail));
        terminal.line("  Not added; repair the source and try again.");
        return Ok(());
    }
    if !item.resolved {
        // `SET-005`: a currently absent manual source may be saved after an
        // explicit confirmation.
        terminal.line(&format!("  This source is currently {}.", item.detail));
        if !terminal.confirm("  Save it anyway?", false)? {
            terminal.line("  Not added.");
            return Ok(());
        }
    }
    item.selected = true;
    // Manual entry is itself an affirmative enrollment choice. Collisions stay
    // visible but do not reverse that choice (`SET-008`).
    item.selection_touched = true;
    if let Some(value) = &item.value {
        add_alias(
            &mut context.aliases.sources,
            value,
            item.members[0].source.file(),
        );
    }
    merge_item(items, item);
    refresh_items(scope, items, context);
    Ok(())
}

/// Offline verification (`SET-001` phase four).
fn verification_phase(
    terminal: &mut Terminal<'_>,
    environment: &Environment,
    project_root: &Path,
    global_sources: &[SourceRef],
    project_sources: &[SourceRef],
) -> Exit {
    terminal.line("Verification");
    match crate::registry::build(environment, Some(project_root)) {
        crate::registry::Outcome::Ready(registry) => {
            let enrolled = global_sources.len() + project_sources.len();
            terminal.line(&format!(
                "  {} enrolled: {} active, {} unresolved.",
                count(enrolled, "source", "sources"),
                registry.redactor.active_count(),
                registry.unresolved.len()
            ));
            for (path, keys) in &registry.duplicate_keys {
                // `SRC-004`: warn about duplicates without showing either value.
                terminal.line(&format!(
                    "  warning: {} assigns {} more than once; the last assignment wins.",
                    sanitize::path(path),
                    keys.len()
                ));
            }
            if registry.redactor.is_empty() {
                terminal.line("  INACTIVE: no source resolves to a value right now.");
            }
            terminal.line("Setup complete.");
            Exit::Ok
        }
        crate::registry::Outcome::Malfunction(malfunction) => {
            terminal.line(&format!("  verification failed: {}", malfunction.message()));
            Exit::Failure
        }
    }
}

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
pub mod discovery;
pub mod integrations;
pub mod preview;
pub mod ui;
pub mod vocabulary;
pub mod write;

use std::collections::HashSet;
use std::path::Path;

use crate::cli::Exit;
use crate::config::{self, Config, ConfigError, Load};
use crate::paths::{self, PROJECT_CONFIG_FILENAME};
use crate::sanitize;
use crate::secret::SourceId;
use crate::source::{Environment, Resolution, Resolver, SourceRef, Unresolved};

use collision::Collisions;
use discovery::{Discovered, State};
use ui::{Cancelled, Terminal};
use vocabulary::Signal;

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
            "secretsieve: the configuration location could not be determined. Set HOME or \
             XDG_CONFIG_HOME.",
        );
        return Exit::Failure;
    };
    let project_root = paths::setup_project_root(current_directory);
    let project_path = project_root.join(PROJECT_CONFIG_FILENAME);

    terminal.line("SecretSieve setup");
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

    let global_result = enrollment_phase(
        terminal,
        Scope::Global,
        &global,
        &global_path,
        &project_root,
        environment,
        home.as_deref(),
    );
    let global_sources = match global_result {
        PhaseResult::Kept(sources) | PhaseResult::Saved(sources) => sources,
        PhaseResult::Stopped(exit) => return exit,
    };

    let project_result = enrollment_phase(
        terminal,
        Scope::Project,
        &project,
        &project_path,
        &project_root,
        environment,
        home.as_deref(),
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
            "secretsieve: `{}` could not be written because {}.",
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
        "secretsieve: `{}` is not a valid SecretSieve configuration: {}.",
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

/// One enrollment phase.
fn enrollment_phase(
    terminal: &mut Terminal<'_>,
    scope: Scope,
    existing: &Config,
    config_path: &Path,
    project_root: &Path,
    environment: &Environment,
    home: Option<&Path>,
) -> PhaseResult {
    let mut items = build_items(scope, existing, project_root, environment, home);
    annotate_collisions(&mut items, project_root);

    terminal.line(scope.title());
    terminal.line(&format!("  file: {}", sanitize::path(config_path)));
    loop {
        render(terminal, &items);
        let answer = match terminal.ask(
            "Toggle numbers, [a]ll, [n]one, add [e]nv, add dotenv [k]ey, add [w]ildcard file, \
             Enter to save, [s]kip, [q]uit:",
        ) {
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
                return match write::write(config_path, &selected, scope == Scope::Global) {
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
                            "secretsieve: `{}` could not be written because {}.",
                            sanitize::path(config_path),
                            error.reason()
                        ));
                        PhaseResult::Stopped(Exit::Failure)
                    }
                };
            }
            "s" => {
                terminal.line("Skipped; this file is unchanged.");
                terminal.blank();
                return PhaseResult::Kept(existing.sources.clone());
            }
            "q" => return cancelled(terminal),
            "a" => {
                for item in &mut items {
                    if item.problem.is_none() {
                        item.selected = true;
                    }
                }
            }
            "n" => {
                for item in &mut items {
                    item.selected = false;
                }
            }
            "e" | "k" | "w" => {
                match add_manual(
                    terminal,
                    answer.trim(),
                    &mut items,
                    config_path,
                    home,
                    environment,
                ) {
                    Ok(()) => {}
                    Err(Cancelled) => return cancelled(terminal),
                }
            }
            selection => toggle(terminal, &mut items, selection),
        }
    }
}

fn cancelled(terminal: &mut Terminal<'_>) -> PhaseResult {
    // `CLI-004`: cancellation returns nonzero. Phases already committed stay.
    terminal.line("Setup cancelled. Nothing further was changed.");
    PhaseResult::Stopped(Exit::Failure)
}

/// One selectable line in a phase.
struct Item {
    source: SourceRef,
    enrolled: bool,
    selected: bool,
    /// Masked preview and explanatory signals, when the source resolves.
    detail: String,
    /// Why the source cannot be used, when it currently cannot.
    problem: Option<String>,
    /// The resolved value, kept only in memory for collision analysis.
    value: Option<String>,
    collisions: Option<Collisions>,
}

impl Item {
    fn description(&self) -> String {
        describe(&self.source)
    }
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
    }
}

fn build_items(
    scope: Scope,
    existing: &Config,
    project_root: &Path,
    environment: &Environment,
    home: Option<&Path>,
) -> Vec<Item> {
    let mut resolver = Resolver::new();
    let mut items: Vec<Item> = Vec::new();

    // `CFG-015`: existing valid enrollment is preserved by default, including
    // sources that are merely unresolved right now.
    for source in &existing.sources {
        items.push(item_for(source.clone(), true, &mut resolver, environment));
    }

    let known: HashSet<SourceId> = items.iter().map(|item| item.source.id()).collect();
    let mut candidates: Vec<Item> = Vec::new();

    if scope == Scope::Global {
        // `SET-002`: the current process environment is inspected automatically.
        for name in environment_candidates(environment) {
            let source = SourceRef::Env { name };
            if known.contains(&source.id()) {
                continue;
            }
            candidates.push(item_for(source, false, &mut resolver, environment));
        }
    }

    let discovered = match scope {
        // `SET-004`: bounded probe locations only.
        Scope::Global => home.map(discovery::global_dotenv_files).unwrap_or_default(),
        // `SET-003`: recursive project discovery.
        Scope::Project => discovery::project_dotenv_files(project_root),
    };
    for file in &discovered {
        candidates.extend(file_candidates(file, &known, &mut resolver, environment));
    }

    // Rank suggestions by their advisory signals; gating already decided which
    // candidates exist at all (`SET-006`).
    candidates.sort_by_key(|item| std::cmp::Reverse(rank_of(item)));
    items.extend(candidates);
    items
}

fn rank_of(item: &Item) -> u32 {
    match &item.value {
        None => 0,
        Some(value) => {
            let mut signals = vocabulary::value_signals(value);
            signals.push(Signal::NameMatches("name"));
            vocabulary::rank(&signals)
        }
    }
}

/// Name-gated environment variables, in a stable order.
fn environment_candidates(environment: &Environment) -> Vec<String> {
    let mut names: Vec<String> = environment
        .names()
        .filter(|name| vocabulary::gating_term(name).is_some())
        .map(str::to_string)
        .collect();
    names.sort();
    names
}

/// Candidates offered for one discovered dotenv file.
fn file_candidates(
    file: &Discovered,
    known: &HashSet<SourceId>,
    resolver: &mut Resolver,
    environment: &Environment,
) -> Vec<Item> {
    let (Some(entered), State::Available(dotenv)) = (&file.entered, &file.state) else {
        return Vec::new();
    };
    dotenv
        .entries()
        .filter(|(key, value)| !value.is_empty() && vocabulary::gating_term(key).is_some())
        .map(|(key, _)| SourceRef::DotenvKey {
            entered: entered.clone(),
            path: file.path.clone(),
            key: key.to_string(),
        })
        .filter(|source| !known.contains(&source.id()))
        .map(|source| item_for(source, false, resolver, environment))
        .collect()
}

fn item_for(
    source: SourceRef,
    enrolled: bool,
    resolver: &mut Resolver,
    environment: &Environment,
) -> Item {
    let mut item = Item {
        source: source.clone(),
        enrolled,
        // `SET-007`: gated candidates are selected by default; collision
        // analysis may unselect them afterwards.
        selected: true,
        detail: String::new(),
        problem: None,
        value: None,
        collisions: None,
    };

    match resolver.resolve(&source, environment) {
        Resolution::Resolved(secrets) => {
            let value = secrets.first().map(|secret| secret.value.clone());
            item.detail = match &value {
                Some(value) => {
                    // The gating term explains why the candidate is offered at
                    // all; value signals only explain its rank (`SET-006`).
                    let mut signals: Vec<Signal> = source
                        .id()
                        .key()
                        .and_then(vocabulary::gating_term)
                        .map(Signal::NameMatches)
                        .into_iter()
                        .collect();
                    signals.extend(vocabulary::value_signals(value));
                    let described: Vec<String> = signals.iter().map(Signal::describe).collect();
                    if described.is_empty() {
                        preview::describe(value)
                    } else {
                        format!("{}; {}", preview::describe(value), described.join(", "))
                    }
                }
                None => format!("{} current keys", secrets.len()),
            };
            if matches!(source, SourceRef::DotenvAll { .. }) {
                item.detail = format!("{} current key(s)", secrets.len());
            }
            item.value = value;
        }
        Resolution::Unresolved { why, .. } => {
            item.detail = format!("unresolved: {}", unresolved_reason(why));
            // An unresolved source is not an error; it is simply not selected by
            // default unless it is already enrolled (`CFG-015`).
            item.selected = enrolled;
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

fn unresolved_reason(why: Unresolved) -> &'static str {
    why.reason()
}

/// Runs collision analysis for every resolvable candidate (`SET-011`).
fn annotate_collisions(items: &mut [Item], project_root: &Path) {
    let subjects: Vec<collision::Subject<'_>> = items
        .iter()
        .filter_map(|item| {
            item.value.as_ref().map(|value| collision::Subject {
                value,
                source_file: item.source.file(),
            })
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
        if !collisions.is_empty() {
            // `SET-007`: a colliding candidate stays visible but unselected,
            // unless it is already enrolled (`CFG-015`).
            if !item.enrolled {
                item.selected = false;
            }
            item.collisions = Some(collisions);
        }
    }
}

fn render(terminal: &mut Terminal<'_>, items: &[Item]) {
    terminal.blank();
    if items.is_empty() {
        terminal.line("  (no candidates found)");
        return;
    }
    for (index, item) in items.iter().enumerate() {
        let marker = match (&item.problem, item.selected) {
            (Some(_), _) => "!",
            (None, true) => "x",
            (None, false) => " ",
        };
        let enrolled = if item.enrolled { " (enrolled)" } else { "" };
        terminal.line(&format!(
            "  {:>2} [{marker}] {}{enrolled}",
            index + 1,
            item.description()
        ));
        if !item.detail.is_empty() {
            terminal.line(&format!("        {}", item.detail));
        }
        if let Some(collisions) = &item.collisions {
            terminal.line(&format!("        collision: {}", collisions.describe()));
        }
    }
}

/// The first selected source that blocks saving (`SET-013`).
fn blocking_item(items: &[Item]) -> Option<String> {
    items
        .iter()
        .find(|item| item.selected && item.problem.is_some())
        .map(|item| item.description())
}

fn selected_sources(items: &[Item]) -> Vec<SourceRef> {
    items
        .iter()
        .filter(|item| item.selected)
        .map(|item| item.source.clone())
        .collect()
}

fn toggle(terminal: &mut Terminal<'_>, items: &mut [Item], selection: &str) {
    let mut unknown = Vec::new();
    for token in selection.split_whitespace() {
        match token.parse::<usize>() {
            Ok(number) if number >= 1 && number <= items.len() => {
                let item = &mut items[number - 1];
                if item.problem.is_some() && !item.selected {
                    terminal.line(&format!(
                        "  {} is unavailable and cannot be selected.",
                        item.description()
                    ));
                    continue;
                }
                item.selected = !item.selected;
            }
            _ => unknown.push(sanitize::text(token)),
        }
    }
    if !unknown.is_empty() {
        terminal.line(&format!("  Not a choice: {}", unknown.join(", ")));
    }
}

/// Manual entry of a source (`SET-005`).
fn add_manual(
    terminal: &mut Terminal<'_>,
    kind: &str,
    items: &mut Vec<Item>,
    config_path: &Path,
    home: Option<&Path>,
    environment: &Environment,
) -> Result<(), Cancelled> {
    let base = config_path.parent().unwrap_or(Path::new("."));
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
            let path = match paths::expand(&entered, base, home) {
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
        _ => return Ok(()),
    };

    if items.iter().any(|item| item.source.id() == source.id()) {
        terminal.line("  That source is already listed.");
        return Ok(());
    }

    let mut resolver = Resolver::new();
    let mut item = item_for(source, false, &mut resolver, environment);
    if item.problem.is_some() || item.value.is_none() {
        // `SET-005`: a currently absent manual source may be saved after an
        // explicit confirmation.
        terminal.line(&format!("  This source is currently {}.", item.detail));
        if !terminal.confirm("  Save it anyway?", false)? {
            terminal.line("  Not added.");
            return Ok(());
        }
        item.problem = None;
    }
    item.selected = true;
    items.push(item);
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

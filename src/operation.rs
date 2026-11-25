//! Operations: changing Packages safely.
//!
//! An update first works out its Plan (every Package `brew` would change,
//! dependencies included) from a dry run of each target, and holds any
//! target whose part of the Plan is In Use. Execution then runs the targets
//! one at a time, checking In Use again just before each, streaming `brew`'s
//! output, and carrying on past a failure.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use crate::in_use::{self, InUse, Reason, Target};
use crate::inventory::{self, Kind, Package};
use crate::outdated::{self, Outdated};
use crate::system::{Command, System};

/// The name brewmax is installed under.
pub const BREWMAX: &str = "brewmax";

/// One Package change in a Plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub name: String,
    pub from: String,
    pub to: String,
}

/// What an operation does to each target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Upgrade,
    Install,
    /// Remove the Package, leaving a Cask's settings and data alone.
    Uninstall,
    /// Remove a Cask together with its settings, caches and data.
    Zap,
}

impl Action {
    fn verb(self) -> &'static str {
        match self {
            Action::Upgrade => "upgrade",
            Action::Install => "install",
            Action::Uninstall | Action::Zap => "uninstall",
        }
    }

    /// Whether this Action removes Packages.
    pub fn removes(self) -> bool {
        matches!(self, Action::Uninstall | Action::Zap)
    }

    /// What a target the Action succeeded on has become, e.g. "Updated".
    pub fn done(self) -> &'static str {
        match self {
            Action::Upgrade => "Updated",
            Action::Install => "Installed",
            Action::Uninstall => "Uninstalled",
            Action::Zap => "Zapped",
        }
    }
}

/// What changing one target does: the target and the dependencies `brew`
/// installs or upgrades along with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPlan {
    pub target: String,
    pub kind: Kind,
    pub action: Action,
    pub steps: Vec<Step>,
}

impl TargetPlan {
    pub fn command(&self) -> Command {
        let kind = kind_flag(self.kind);
        match self.action {
            Action::Zap => Command::brew(&["uninstall", "--zap", kind, &self.target]),
            action => Command::brew(&[action.verb(), kind, &self.target]),
        }
    }

    /// The dry run of exactly the command [`TargetPlan::command`] will run.
    fn dry_run_command(action: Action, target: &Package) -> Command {
        Command::brew(&[
            action.verb(),
            "--dry-run",
            kind_flag(target.kind),
            &target.name,
        ])
    }
}

/// Everything an update would change, target by target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub targets: Vec<TargetPlan>,
}

impl Plan {
    /// Every Package change in the Plan, each once.
    pub fn steps(&self) -> Vec<&Step> {
        let mut steps: Vec<&Step> = Vec::new();
        for step in self.targets.iter().flat_map(|t| &t.steps) {
            if !steps.iter().any(|s| s.name == step.name) {
                steps.push(step);
            }
        }
        steps
    }

    /// A Plan touching more than one Package must be confirmed first.
    pub fn needs_confirmation(&self) -> bool {
        self.steps().len() > 1
    }
}

/// The flag that tells `brew` which kind of Package a name refers to.
fn kind_flag(kind: Kind) -> &'static str {
    match kind {
        Kind::Formula => "--formula",
        Kind::Cask => "--cask",
    }
}

/// Packages that are In Use, each with why.
pub type Held = Vec<(String, Vec<Reason>)>;

/// What happened in a background operation, sent to the app as it happens.
#[derive(Debug)]
pub enum Event {
    /// The Plan was worked out; `held` lists its In Use Packages.
    Planned(Result<(Plan, Held), String>),
    /// Just before running `target`, something in its part of the Plan was
    /// In Use, so it was left unchanged.
    StepHeld { target: String, held: Held },
    /// A line of `brew` output.
    Line(String),
    /// `brew` finished with `target`.
    StepFinished {
        target: String,
        success: bool,
        error: Option<String>,
    },
    /// The installed Packages and outdated versions, re-read at the end.
    Reloaded {
        packages: Result<Vec<Package>, String>,
        outdated: Result<Outdated, String>,
    },
}

/// Work out the Plan for updating `packages`, and which Packages in it are In
/// Use right now. A target that another target's upgrade brings along is
/// left out.
pub fn plan(
    system: &impl System,
    packages: &[Package],
    targets: &[Target],
) -> Result<(Plan, Held), String> {
    plan_action(system, Action::Upgrade, packages, targets)
}

/// Like [`plan`], for any [`Action`]. Installing can upgrade installed
/// dependencies too, so an install's Plan is checked for In Use the same way.
pub fn plan_action(
    system: &impl System,
    action: Action,
    packages: &[Package],
    targets: &[Target],
) -> Result<(Plan, Held), String> {
    let mut planned = Vec::new();
    for package in packages {
        if planned
            .iter()
            .any(|t: &TargetPlan| t.steps.iter().any(|s| s.name == package.name))
        {
            continue;
        }
        planned.push(plan_target(system, action, package)?);
    }
    // Drop any target another target's upgrade brings along anyway, whatever
    // order they were planned in, so nothing is upgraded twice.
    let brought_along = |t: &TargetPlan| {
        planned
            .iter()
            .any(|other| other.target != t.target && other.steps.iter().any(|s| s.name == t.target))
    };
    let plan = Plan {
        targets: planned
            .iter()
            .filter(|t| !brought_along(t))
            .cloned()
            .collect(),
    };
    let held = held(system, plan.steps(), targets)?;
    Ok((plan, held))
}

fn plan_target(
    system: &impl System,
    action: Action,
    target: &Package,
) -> Result<TargetPlan, String> {
    let output = system
        .run(&TargetPlan::dry_run_command(action, target))
        .map_err(|e| format!("{e:#}"))?;
    if !output.success {
        return Err(format!(
            "brew could not plan the {} of {}: {}",
            action.verb(),
            target.name,
            output.stderr.trim()
        ));
    }
    let mut steps = parse_steps(&output.stdout);
    if !steps.iter().any(|s| s.name == target.name) {
        steps.insert(
            0,
            Step {
                name: target.name.clone(),
                from: match action {
                    Action::Upgrade | Action::Uninstall | Action::Zap => target.version.clone(),
                    Action::Install => String::new(),
                },
                to: String::new(),
            },
        );
    }
    Ok(TargetPlan {
        target: target.name.clone(),
        kind: target.kind,
        action,
        steps,
    })
}

/// The Plan for uninstalling (or, with [`Action::Zap`], zapping) one
/// installed Package: just the Package itself.
pub fn removal(package: &Package, action: Action) -> TargetPlan {
    TargetPlan {
        target: package.name.clone(),
        kind: package.kind,
        action,
        steps: vec![Step {
            name: package.name.clone(),
            from: package.version.clone(),
            to: String::new(),
        }],
    }
}

/// Work out the Plan for uninstalling (or zapping) `package`, and whether it
/// is In Use. Refused if other installed Packages depend on it; a failed
/// dependents check is refused too, never treated as "no dependents".
pub fn plan_removal(
    system: &impl System,
    package: &Package,
    action: Action,
    targets: &[Target],
) -> Result<(Plan, Held), String> {
    refuse_if_depended_on(system, package)?;
    let plan = Plan {
        targets: vec![removal(package, action)],
    };
    let held = held(system, plan.steps(), targets)?;
    Ok((plan, held))
}

/// Refuse to remove `package` if other installed Packages depend on it,
/// naming them. A failed check refuses too, never counting as "none".
fn refuse_if_depended_on(system: &impl System, package: &Package) -> Result<(), String> {
    let dependents = dependents(system, package)?;
    if dependents.is_empty() {
        return Ok(());
    }
    Err(format!(
        "Can't uninstall {}: other installed Packages depend on it: {}",
        package.name,
        dependents.join(", ")
    ))
}

/// The installed Packages that depend on `package`, read afresh: Formulae
/// (and Casks) from `brew uses`, which only accepts Formula names, plus the
/// installed Casks whose `depends_on` names it.
fn dependents(system: &impl System, package: &Package) -> Result<Vec<String>, String> {
    let failed = |e: String| format!("Could not check what depends on {}: {e}", package.name);
    let installed = inventory::load_installed(system).map_err(|e| failed(format!("{e:#}")))?;
    let mut dependents: Vec<String> = Vec::new();
    if package.kind == Kind::Formula {
        let output = system
            .run(&Command::brew(&["uses", "--installed", &package.name]))
            .map_err(|e| failed(format!("{e:#}")))?;
        if !output.success {
            return Err(failed(output.stderr.trim().to_string()));
        }
        dependents.extend(
            output
                .stdout
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(String::from),
        );
    }
    for cask in installed
        .iter()
        .filter(|p| p.cask_dependencies.contains(&package.name))
    {
        if !dependents.contains(&cask.name) {
            dependents.push(cask.name.clone());
        }
    }
    Ok(dependents)
}

/// Run each target's part of a Plan in turn: check In Use just before it,
/// upgrade it, and carry on whatever happened. Then re-read the installed
/// Packages. Progress is sent as [`Event`]s. Returns the targets `brew`
/// succeeded on.
pub fn execute(
    system: &impl System,
    plan: &[TargetPlan],
    targets: &[Target],
    events: &Sender<Event>,
) -> Vec<String> {
    let mut succeeded = Vec::new();
    for target_plan in plan {
        let target = target_plan.target.clone();
        match held(system, target_plan.steps.iter().collect(), targets) {
            Ok(held) if !held.is_empty() => {
                let _ = events.send(Event::StepHeld { target, held });
                continue;
            }
            Ok(_) => {}
            Err(error) => {
                let _ = events.send(Event::StepFinished {
                    target,
                    success: false,
                    error: Some(format!("In Use check failed, left unchanged: {error}")),
                });
                continue;
            }
        }

        let _ = events.send(Event::Line(format!("$ {}", target_plan.command())));
        let result = system.stream(&target_plan.command(), &mut |line| {
            let _ = events.send(Event::Line(line.to_string()));
        });
        let (success, error) = match result {
            Ok(success) => (success, None),
            Err(e) => (false, Some(format!("{e:#}"))),
        };
        if success && error.is_none() {
            succeeded.push(target.clone());
        }
        let _ = events.send(Event::StepFinished {
            target,
            success,
            error,
        });
    }

    let _ = events.send(Event::Reloaded {
        packages: inventory::load_installed(system).map_err(|e| format!("{e:#}")),
        outdated: outdated::load_outdated(system).map_err(|e| format!("{e:#}")),
    });
    succeeded
}

/// The Packages among `steps` that are In Use now.
fn held(system: &impl System, steps: Vec<&Step>, targets: &[Target]) -> Result<Held, String> {
    let in_use: InUse = in_use::scan(system, targets).map_err(|e| format!("{e:#}"))?;
    Ok(steps
        .into_iter()
        .filter_map(|step| {
            in_use
                .get(&step.name)
                .map(|r| (step.name.clone(), r.clone()))
        })
        .collect())
}

/// The Package changes in `brew upgrade --dry-run` or `brew install
/// --dry-run` output: every `name old -> new` line (an upgrade), and every
/// `name [version]` line under a `==> Would install` heading (a new install,
/// with an empty `from`).
fn parse_steps(output: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut installing = false;
    for line in output.lines() {
        if let Some(heading) = line.strip_prefix("==>") {
            installing = heading.trim_start().starts_with("Would install");
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            [name, from, "->", to, ..] => steps.push(Step {
                name: (*name).into(),
                from: (*from).into(),
                to: (*to).into(),
            }),
            [name, rest @ ..] if installing => steps.push(Step {
                name: (*name).into(),
                from: String::new(),
                to: rest.first().map(|v| v.to_string()).unwrap_or_default(),
            }),
            _ => {}
        }
    }
    steps
}

/// How long a Graceful Quit waits for an app to exit, and how often it checks.
#[derive(Debug, Clone, Copy)]
pub struct QuitWait {
    pub timeout: Duration,
    pub interval: Duration,
}

impl Default for QuitWait {
    /// Long enough to answer a save prompt.
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(60),
            interval: Duration::from_millis(500),
        }
    }
}

/// What a Graceful Quit goes on to do once the Cask's apps have quit.
#[derive(Debug, Clone)]
pub enum AfterQuit {
    /// Update the Cask, then reopen the apps.
    Update,
    /// Run this (confirmed) removal; reopen the apps only if the Cask is
    /// still installed afterwards.
    Remove(TargetPlan),
}

/// Graceful Quit, then update the Cask as usual and reopen the apps brewmax
/// quit.
pub fn graceful_update(
    system: &impl System,
    package: &Package,
    targets: &[Target],
    wait: QuitWait,
    events: &Sender<Event>,
) {
    graceful(system, package, targets, wait, AfterQuit::Update, events);
}

/// Graceful Quit: ask each of the Cask's running apps to quit the normal way
/// (so it can prompt to save), wait until it has exited, then do `after`.
/// Never force-kills: if an app refuses or doesn't exit in time, the Cask
/// stays Held and unchanged.
pub fn graceful(
    system: &impl System,
    package: &Package,
    targets: &[Target],
    wait: QuitWait,
    after: AfterQuit,
    events: &Sender<Event>,
) {
    let line = |text: String| {
        let _ = events.send(Event::Line(text));
    };
    // What about the Cask is In Use now. A failed check is an error, never
    // "nothing is In Use".
    let held_now = || -> anyhow::Result<Held> {
        let in_use = in_use::scan(system, targets)?;
        Ok(in_use
            .get(&package.name)
            .map(|reasons| vec![(package.name.clone(), reasons.clone())])
            .unwrap_or_default())
    };
    let last_known = || held_now().unwrap_or_default();
    let hold = |held: Held| {
        let _ = events.send(Event::StepHeld {
            target: package.name.clone(),
            held,
        });
    };

    // Never quit anything for a removal that would be refused.
    if let AfterQuit::Remove(_) = after
        && let Err(error) = refuse_if_depended_on(system, package)
    {
        let _ = events.send(Event::StepFinished {
            target: package.name.clone(),
            success: false,
            error: Some(error),
        });
        return;
    }

    let apps = match in_use::running_apps(system, &package.app_paths) {
        Ok(apps) if !apps.is_empty() => apps,
        Ok(_) => {
            line(format!(
                "None of {}'s apps are running, so there is nothing to quit",
                package.name
            ));
            hold(last_known());
            return;
        }
        Err(e) => {
            line(format!("Could not check which apps are running: {e:#}"));
            hold(last_known());
            return;
        }
    };

    let mut quit = Vec::new();
    for app in &apps {
        let name = app_name(app);
        let result = bundle_id(system, app).and_then(|id| {
            line(format!("Asking {name} to quit…"));
            let script = format!("tell application id \"{id}\" to quit");
            let output = system.run(&Command::new("osascript", &["-e", &script]))?;
            if output.success {
                Ok(())
            } else {
                anyhow::bail!("{}", output.stderr.trim())
            }
        });
        if let Err(e) = result {
            line(format!("{name} did not quit, nothing was changed: {e:#}"));
            hold(last_known());
            reopen(system, &quit, &line);
            return;
        }
        quit.push(app.clone());
    }

    let deadline = Instant::now() + wait.timeout;
    let mut last_held = Held::new();
    loop {
        match held_now() {
            Ok(held) if held.is_empty() => break,
            Ok(held) => last_held = held,
            // Can't tell whether the app has quit: keep waiting.
            Err(e) => line(format!("In Use check failed, still waiting: {e:#}")),
        }
        if Instant::now() >= deadline {
            line(format!(
                "{} did not quit within {}s, nothing was changed",
                package.name,
                wait.timeout.as_secs_f32()
            ));
            if last_held.is_empty() {
                last_held = vec![(package.name.clone(), Vec::new())];
            }
            hold(last_held);
            // Reopen any app that did quit, so brewmax leaves nothing closed.
            let still_running =
                in_use::running_apps(system, &quit).unwrap_or_else(|_| quit.clone());
            let exited: Vec<String> = quit
                .iter()
                .filter(|a| !still_running.contains(a))
                .cloned()
                .collect();
            reopen(system, &exited, &line);
            return;
        }
        std::thread::sleep(wait.interval);
    }

    match after {
        AfterQuit::Update => {
            match plan(system, std::slice::from_ref(package), targets) {
                Err(error) => {
                    let _ = events.send(Event::StepFinished {
                        target: package.name.clone(),
                        success: false,
                        error: Some(error),
                    });
                }
                Ok((_, held)) if !held.is_empty() => hold(held),
                Ok((plan, _)) => {
                    execute(system, &plan.targets, targets, events);
                }
            }
            reopen(system, &quit, &line);
        }
        AfterQuit::Remove(removal) => {
            let removed = execute(system, std::slice::from_ref(&removal), targets, events);
            if !removed.contains(&package.name) {
                // Still installed: leave nothing closed.
                reopen(system, &quit, &line);
            }
        }
    }
}

/// Reopen apps brewmax quit.
fn reopen(system: &impl System, apps: &[String], line: &impl Fn(String)) {
    for app in apps {
        match system.run(&Command::new("open", &["-a", app])) {
            Ok(output) if output.success => line(format!("Reopened {}", app_name(app))),
            Ok(output) => line(format!(
                "Could not reopen {}: {}",
                app_name(app),
                output.stderr.trim()
            )),
            Err(e) => line(format!("Could not reopen {}: {e:#}", app_name(app))),
        }
    }
}

/// "Slack" for "/Applications/Slack.app".
fn app_name(app: &str) -> &str {
    let file = app.rsplit('/').next().unwrap_or(app);
    file.strip_suffix(".app").unwrap_or(file)
}

/// The app's bundle identifier. Quitting by identifier never makes macOS
/// ask "where is this app?", unlike quitting by name.
fn bundle_id(system: &impl System, app: &str) -> anyhow::Result<String> {
    let plist = format!("{app}/Contents/Info.plist");
    let output = system.run(&Command::new(
        "/usr/libexec/PlistBuddy",
        &["-c", "Print :CFBundleIdentifier", &plist],
    ))?;
    let id = output.stdout.trim();
    if !output.success || id.is_empty() {
        anyhow::bail!(
            "could not read the bundle id of {app}: {}",
            output.stderr.trim()
        );
    }
    Ok(id.to_string())
}

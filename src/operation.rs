//! Operations: changing Packages safely.
//!
//! An update first works out its Plan (every Package `brew` would change,
//! dependencies included) from a dry run, and holds the whole Plan if any
//! Package in it is In Use. Execution checks In Use again just before
//! running, and streams `brew`'s output.

use std::sync::mpsc::Sender;

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

/// Everything an update of `target` would change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub target: String,
    pub kind: Kind,
    pub steps: Vec<Step>,
}

impl Plan {
    /// A Plan touching more than one Package must be confirmed first.
    pub fn needs_confirmation(&self) -> bool {
        self.steps.len() > 1
    }

    pub fn upgrade_command(&self) -> Command {
        Command::brew(&["upgrade", kind_flag(self.kind), &self.target])
    }

    /// The dry run of exactly the command [`Plan::upgrade_command`] will run.
    fn dry_run_command(target: &Package) -> Command {
        Command::brew(&["upgrade", "--dry-run", kind_flag(target.kind), &target.name])
    }
}

/// The flag that tells `brew` which kind of Package a name refers to.
fn kind_flag(kind: Kind) -> &'static str {
    match kind {
        Kind::Formula => "--formula",
        Kind::Cask => "--cask",
    }
}

/// Packages in a Plan that are In Use, each with why.
pub type Held = Vec<(String, Vec<Reason>)>;

/// What happened in a background operation, sent to the app as it happens.
#[derive(Debug)]
pub enum Event {
    /// The Plan was worked out; `held` lists its In Use Packages.
    Planned(Result<(Plan, Held), String>),
    /// Just before running, something in the Plan was In Use.
    Held(Held),
    /// A line of `brew` output.
    Line(String),
    /// `brew` finished.
    Finished {
        success: bool,
        error: Option<String>,
    },
    /// The installed Packages and outdated versions, re-read afterwards.
    Reloaded {
        packages: Result<Vec<Package>, String>,
        outdated: Result<Outdated, String>,
    },
}

/// Work out the Plan for updating `target`, and which of its Packages are In
/// Use right now.
pub fn plan(
    system: &impl System,
    target: &Package,
    targets: &[Target],
) -> Result<(Plan, Held), String> {
    let output = system
        .run(&Plan::dry_run_command(target))
        .map_err(|e| format!("{e:#}"))?;
    if !output.success {
        return Err(format!(
            "brew could not plan the update: {}",
            output.stderr.trim()
        ));
    }
    let mut steps = parse_steps(&output.stdout);
    if !steps.iter().any(|s| s.name == target.name) {
        steps.insert(
            0,
            Step {
                name: target.name.clone(),
                from: target.version.clone(),
                to: String::new(),
            },
        );
    }
    let plan = Plan {
        target: target.name.clone(),
        kind: target.kind,
        steps,
    };
    let held = held(system, &plan, targets)?;
    Ok((plan, held))
}

/// Run a Plan: check In Use once more, then run the upgrade, then re-read the
/// installed Packages. Progress is sent as [`Event`]s.
pub fn execute(system: &impl System, plan: &Plan, targets: &[Target], events: &Sender<Event>) {
    match held(system, plan, targets) {
        Ok(held) if !held.is_empty() => {
            let _ = events.send(Event::Held(held));
            return;
        }
        Ok(_) => {}
        Err(error) => {
            let _ = events.send(Event::Finished {
                success: false,
                error: Some(format!("In Use check failed, nothing was changed: {error}")),
            });
            return;
        }
    }

    let result = system.stream(&plan.upgrade_command(), &mut |line| {
        let _ = events.send(Event::Line(line.to_string()));
    });
    let _ = match result {
        Ok(success) => events.send(Event::Finished {
            success,
            error: None,
        }),
        Err(e) => events.send(Event::Finished {
            success: false,
            error: Some(format!("{e:#}")),
        }),
    };

    let _ = events.send(Event::Reloaded {
        packages: inventory::load_installed(system).map_err(|e| format!("{e:#}")),
        outdated: outdated::load_outdated(system).map_err(|e| format!("{e:#}")),
    });
}

/// The Packages in `plan` that are In Use now.
fn held(system: &impl System, plan: &Plan, targets: &[Target]) -> Result<Held, String> {
    let in_use: InUse = in_use::scan(system, targets).map_err(|e| format!("{e:#}"))?;
    Ok(plan
        .steps
        .iter()
        .filter_map(|step| {
            in_use
                .get(&step.name)
                .map(|r| (step.name.clone(), r.clone()))
        })
        .collect())
}

/// Every `name old -> new` line in `brew upgrade --dry-run` output.
fn parse_steps(output: &str) -> Vec<Step> {
    output
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let (name, from, arrow, to) =
                (words.next()?, words.next()?, words.next()?, words.next()?);
            (arrow == "->").then(|| Step {
                name: name.into(),
                from: from.into(),
                to: to.into(),
            })
        })
        .collect()
}

//! Operations: changing Packages safely.
//!
//! An update first works out its Plan (every Package `brew` would change,
//! dependencies included) from a dry run of each target, and holds any
//! target whose part of the Plan is In Use. Execution then runs the targets
//! one at a time, checking In Use again just before each, streaming `brew`'s
//! output, and carrying on past a failure.

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

/// What updating one target changes: the target and the dependencies `brew`
/// upgrades along with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetPlan {
    pub target: String,
    pub kind: Kind,
    pub steps: Vec<Step>,
}

impl TargetPlan {
    pub fn upgrade_command(&self) -> Command {
        Command::brew(&["upgrade", kind_flag(self.kind), &self.target])
    }

    /// The dry run of exactly the command [`TargetPlan::upgrade_command`] will run.
    fn dry_run_command(target: &Package) -> Command {
        Command::brew(&["upgrade", "--dry-run", kind_flag(target.kind), &target.name])
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
    let mut planned = Vec::new();
    for package in packages {
        if planned
            .iter()
            .any(|t: &TargetPlan| t.steps.iter().any(|s| s.name == package.name))
        {
            continue;
        }
        planned.push(plan_target(system, package)?);
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

fn plan_target(system: &impl System, target: &Package) -> Result<TargetPlan, String> {
    let output = system
        .run(&TargetPlan::dry_run_command(target))
        .map_err(|e| format!("{e:#}"))?;
    if !output.success {
        return Err(format!(
            "brew could not plan the update of {}: {}",
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
                from: target.version.clone(),
                to: String::new(),
            },
        );
    }
    Ok(TargetPlan {
        target: target.name.clone(),
        kind: target.kind,
        steps,
    })
}

/// Run each target's part of a Plan in turn: check In Use just before it,
/// upgrade it, and carry on whatever happened. Then re-read the installed
/// Packages. Progress is sent as [`Event`]s.
pub fn execute(
    system: &impl System,
    plan: &[TargetPlan],
    targets: &[Target],
    events: &Sender<Event>,
) {
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

        let _ = events.send(Event::Line(format!("$ {}", target_plan.upgrade_command())));
        let result = system.stream(&target_plan.upgrade_command(), &mut |line| {
            let _ = events.send(Event::Line(line.to_string()));
        });
        let (success, error) = match result {
            Ok(success) => (success, None),
            Err(e) => (false, Some(format!("{e:#}"))),
        };
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

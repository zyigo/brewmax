//! In Use detection: which Packages something running depends on right now.
//!
//! One `lsof` listing of every process's executable and loaded libraries
//! (`txt` files) tells us which processes run from a Cask's app bundle or
//! Caskroom directory, or run or load anything from a Formula's Cellar
//! directory. `brew services` tells us which Formulae run as services.

use std::collections::BTreeMap;
use std::fmt;

use serde::Deserialize;

use crate::inventory::{Kind, Package};
use crate::system::{Command, System};

/// The name brewmax is installed under; it is always In Use while running.
const BREWMAX: &str = "brewmax";

/// Why a Package is In Use.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    /// A running process runs from, or has loaded a file from, the Package.
    Process { name: String, pid: u32 },
    /// The Formula is running as a `brew services` service.
    Service,
    /// The Package is brewmax itself.
    Brewmax,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::Process { name, pid } => write!(f, "{name} (pid {pid})"),
            Reason::Service => write!(f, "running as a brew service"),
            Reason::Brewmax => write!(f, "this is brewmax"),
        }
    }
}

/// The In Use Packages, by name, each with why.
pub type InUse = BTreeMap<String, Vec<Reason>>;

/// What the scan needs to know about one Package.
#[derive(Debug, Clone)]
pub struct Target {
    name: String,
    kind: Kind,
    app_paths: Vec<String>,
}

impl From<&Package> for Target {
    fn from(p: &Package) -> Self {
        Self {
            name: p.name.clone(),
            kind: p.kind,
            app_paths: p.app_paths.clone(),
        }
    }
}

/// Scan the machine and work out which of `targets` are In Use.
pub fn scan(system: &impl System, targets: &[Target]) -> anyhow::Result<InUse> {
    let mut in_use = InUse::new();
    let mut add = |name: &str, reason: Reason| {
        let reasons = in_use.entry(name.to_string()).or_default();
        if !reasons.contains(&reason) {
            reasons.push(reason);
        }
    };

    // lsof exits non-zero when it can't inspect some processes (e.g. other
    // users'), so use whatever it printed regardless of exit status.
    let lsof = system.run(&Command::new("lsof", &["-nP", "-F", "pcn", "-d", "txt"]))?;
    for (process, path) in process_files(&lsof.stdout) {
        for target in targets {
            if uses(target, &path) {
                add(
                    &target.name,
                    Reason::Process {
                        name: process.name.clone(),
                        pid: process.pid,
                    },
                );
            }
        }
    }

    // Unlike lsof, a failed or unreadable services listing is a failed scan:
    // reporting "no services running" could clear a real service's marker.
    let services = system.run(&Command::brew(&["services", "list", "--json"]))?;
    if !services.success {
        anyhow::bail!("brew services list failed: {}", services.stderr.trim());
    }
    let services: Vec<ServiceJson> = serde_json::from_str(&services.stdout)
        .map_err(|e| anyhow::anyhow!("could not read brew services list: {e}"))?;
    for service in services.iter().filter(|s| s.status == "started") {
        if targets
            .iter()
            .any(|t| t.kind == Kind::Formula && t.name == service.name)
        {
            add(&service.name, Reason::Service);
        }
    }

    if targets.iter().any(|t| t.name == BREWMAX) {
        add(BREWMAX, Reason::Brewmax);
    }

    for reasons in in_use.values_mut() {
        reasons.sort();
    }
    Ok(in_use)
}

/// Does a process file at `path` belong to `target`?
fn uses(target: &Target, path: &str) -> bool {
    match target.kind {
        Kind::Formula => path.contains(&format!("/Cellar/{}/", target.name)),
        Kind::Cask => {
            path.contains(&format!("/Caskroom/{}/", target.name))
                || target.app_paths.iter().any(|app| {
                    path.strip_prefix(app.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
                })
        }
    }
}

#[derive(Debug, Clone)]
struct Process {
    pid: u32,
    name: String,
}

/// Parse `lsof -F pcn` output into (process, file path) pairs.
fn process_files(output: &str) -> Vec<(Process, String)> {
    let mut files = Vec::new();
    let mut current: Option<Process> = None;
    for line in output.lines() {
        let (tag, value) = line.split_at(line.len().min(1));
        match tag {
            "p" => {
                current = value.parse().ok().map(|pid| Process {
                    pid,
                    name: String::new(),
                })
            }
            "c" => {
                if let Some(process) = current.as_mut() {
                    process.name = value.to_string();
                }
            }
            "n" => {
                if let Some(process) = &current {
                    files.push((process.clone(), value.to_string()));
                }
            }
            _ => {}
        }
    }
    files
}

#[derive(Deserialize)]
struct ServiceJson {
    name: String,
    #[serde(default)]
    status: String,
}

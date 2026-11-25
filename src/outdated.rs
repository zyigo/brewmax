//! The refresh: update Homebrew's metadata, then read which Packages are
//! outdated and what version is available for each.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::system::{Command, System};

/// The newer version available for an outdated Package, by name.
pub type Outdated = BTreeMap<String, String>;

/// The result of one refresh.
#[derive(Debug, Clone)]
pub struct Refresh {
    /// Why `brew update` failed, if it did. Outdated information then comes
    /// from Homebrew's last successful update.
    pub update_error: Option<String>,
    pub outdated: Result<Outdated, String>,
}

/// Run `brew update`, then read the outdated Packages, including
/// Self-Updating Casks (`--greedy`).
pub fn refresh(system: &impl System) -> Refresh {
    let update_error = match system.run(&Command::brew(&["update"])) {
        Ok(output) if output.success => None,
        Ok(output) => Some(output.stderr.trim().to_string()),
        Err(e) => Some(format!("{e:#}")),
    };
    Refresh {
        update_error,
        outdated: load_outdated(system).map_err(|e| format!("{e:#}")),
    }
}

/// Read the outdated Packages from Homebrew's current metadata.
pub fn load_outdated(system: &impl System) -> anyhow::Result<Outdated> {
    let output = system.run(&Command::brew(&["outdated", "--json=v2", "--greedy"]))?;
    if !output.success {
        anyhow::bail!("{}", output.stderr.trim());
    }
    let json: OutdatedJson = serde_json::from_str(&output.stdout)?;
    Ok(json
        .formulae
        .into_iter()
        .chain(json.casks)
        .map(|p| (p.name, p.current_version))
        .collect())
}

#[derive(Deserialize)]
struct OutdatedJson {
    #[serde(default)]
    formulae: Vec<OutdatedPackageJson>,
    #[serde(default)]
    casks: Vec<OutdatedPackageJson>,
}

#[derive(Deserialize)]
struct OutdatedPackageJson {
    name: String,
    current_version: String,
}

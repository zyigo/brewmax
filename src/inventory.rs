//! Inventory: the Packages Homebrew has installed, read from brew's JSON.

use serde::Deserialize;

use crate::system::{Command, System};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Cask,
    Formula,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Cask => "Cask",
            Kind::Formula => "Formula",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub kind: Kind,
    pub version: String,
    pub desc: Option<String>,
}

#[derive(Deserialize)]
struct InfoJson {
    #[serde(default)]
    formulae: Vec<FormulaJson>,
    #[serde(default)]
    casks: Vec<CaskJson>,
}

#[derive(Deserialize)]
struct FormulaJson {
    name: String,
    desc: Option<String>,
    #[serde(default)]
    installed: Vec<InstalledJson>,
}

#[derive(Deserialize)]
struct InstalledJson {
    version: String,
}

#[derive(Deserialize)]
struct CaskJson {
    token: String,
    desc: Option<String>,
    installed: Option<String>,
}

/// Read every installed Package, sorted by name.
pub fn load_installed(system: &impl System) -> anyhow::Result<Vec<Package>> {
    let output = system.run(&Command::brew(&["info", "--json=v2", "--installed"]))?;
    if !output.success {
        anyhow::bail!("brew info failed: {}", output.stderr.trim());
    }
    let info: InfoJson = serde_json::from_str(&output.stdout)?;

    let formulae = info.formulae.into_iter().map(|f| Package {
        version: f
            .installed
            .last()
            .map(|i| i.version.clone())
            .unwrap_or_default(),
        name: f.name,
        kind: Kind::Formula,
        desc: f.desc,
    });
    let casks = info.casks.into_iter().map(|c| Package {
        version: c.installed.unwrap_or_default(),
        name: c.token,
        kind: Kind::Cask,
        desc: c.desc,
    });

    let mut packages: Vec<Package> = formulae.chain(casks).collect();
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(packages)
}

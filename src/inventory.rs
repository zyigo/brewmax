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
    /// For Casks: where their `.app` bundles are installed.
    pub app_paths: Vec<String>,
    /// For Casks: the settings, caches and data a Zap removes, as the Cask
    /// lists them (paths may start with `~` or contain wildcards).
    pub zap_paths: Vec<String>,
    pub pinned: bool,
    /// A Cask that updates itself, so brew only treats it as outdated when asked to.
    pub self_updating: bool,
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
    #[serde(default)]
    pinned: bool,
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
    #[serde(default)]
    artifacts: Vec<serde_json::Value>,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    auto_updates: Option<bool>,
}

/// The installed locations of a Cask's `.app` bundles. Homebrew records the
/// installed path as `target`; older output only names the bundle, which
/// lands in `/Applications`.
fn app_paths(artifacts: &[serde_json::Value]) -> Vec<String> {
    artifacts
        .iter()
        .filter_map(|artifact| {
            let apps = artifact.get("app")?.as_array()?;
            if let Some(target) = artifact.get("target").and_then(|t| t.as_str()) {
                return Some(vec![target.to_string()]);
            }
            Some(
                apps.iter()
                    .filter_map(|a| a.as_str())
                    .map(|a| format!("/Applications/{a}"))
                    .collect(),
            )
        })
        .flatten()
        .collect()
}

/// Everything a Cask's `zap` stanza trashes or deletes. Each of `trash`,
/// `delete` and `rmdir` may be one path or a list.
fn zap_paths(artifacts: &[serde_json::Value]) -> Vec<String> {
    artifacts
        .iter()
        .filter_map(|artifact| artifact.get("zap")?.as_array())
        .flatten()
        .flat_map(|directive| {
            ["trash", "delete", "rmdir"]
                .into_iter()
                .filter_map(|key| directive.get(key))
                .flat_map(|paths| match paths {
                    serde_json::Value::String(path) => vec![path.clone()],
                    serde_json::Value::Array(paths) => paths
                        .iter()
                        .filter_map(|p| p.as_str().map(String::from))
                        .collect(),
                    _ => Vec::new(),
                })
                .collect::<Vec<_>>()
        })
        .collect()
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
        app_paths: Vec::new(),
        zap_paths: Vec::new(),
        pinned: f.pinned,
        self_updating: false,
    });
    let casks = info.casks.into_iter().map(|c| Package {
        version: c.installed.unwrap_or_default(),
        app_paths: app_paths(&c.artifacts),
        zap_paths: zap_paths(&c.artifacts),
        pinned: c.pinned,
        self_updating: c.auto_updates.unwrap_or(false),
        name: c.token,
        kind: Kind::Cask,
        desc: c.desc,
    });

    let mut packages: Vec<Package> = formulae.chain(casks).collect();
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(packages)
}

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
    /// For Casks: everything a Zap removes beyond an ordinary uninstall, as
    /// the Cask's zap stanza lists it.
    pub zap: Vec<ZapEntry>,
    /// For Casks: the Casks it depends on.
    pub cask_dependencies: Vec<String>,
    pub pinned: bool,
    /// A Cask that updates itself, so brew only treats it as outdated when asked to.
    pub self_updating: bool,
}

/// One thing a Zap removes or does, from a Cask's zap stanza. Paths are as
/// the Cask writes them: they may start with `~` or contain wildcards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZapEntry {
    /// A file or folder deleted or moved to the Trash (`trash`, `delete`).
    Remove(String),
    /// A folder removed only if it is empty (`rmdir`).
    RemoveIfEmpty(String),
    /// A background service unloaded (`launchctl`).
    Service(String),
    /// A package receipt forgotten (`pkgutil`).
    Receipt(String),
    /// A login item removed (`login_item`).
    LoginItem(String),
    /// A script run, by its executable (`script`).
    Script(String),
    /// A directive brewmax doesn't recognise, as `key: value` written.
    Other(String),
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
    #[serde(default)]
    depends_on: serde_json::Value,
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

/// The strings in a JSON value that may be one string or a list of them.
/// An item that isn't a string comes back as `Err`, holding the item as
/// written, so it can be shown rather than dropped.
fn string_or_list(value: &serde_json::Value) -> Vec<Result<String, String>> {
    let one = |v: &serde_json::Value| v.as_str().map(String::from).ok_or_else(|| v.to_string());
    match value {
        serde_json::Value::Array(values) => values.iter().map(one).collect(),
        value => vec![one(value)],
    }
}

/// The Casks a Cask depends on (`depends_on.cask`, one name or a list).
fn cask_dependencies(depends_on: &serde_json::Value) -> Vec<String> {
    depends_on
        .get("cask")
        .map(|casks| {
            string_or_list(casks)
                .into_iter()
                .filter_map(Result::ok)
                .collect()
        })
        .unwrap_or_default()
}

/// Every entry of a Cask's zap stanza: the directives brewmax knows, in a
/// fixed order, then any others as written.
fn zap_entries(artifacts: &[serde_json::Value]) -> Vec<ZapEntry> {
    type ToEntry = fn(String) -> ZapEntry;
    const KNOWN: [(&str, ToEntry); 7] = [
        ("trash", ZapEntry::Remove),
        ("delete", ZapEntry::Remove),
        ("rmdir", ZapEntry::RemoveIfEmpty),
        ("launchctl", ZapEntry::Service),
        ("pkgutil", ZapEntry::Receipt),
        ("login_item", ZapEntry::LoginItem),
        ("script", ZapEntry::Script),
    ];
    let directives = artifacts
        .iter()
        .filter_map(|artifact| artifact.get("zap")?.as_array())
        .flatten()
        .filter_map(|directive| directive.as_object());
    let mut entries = Vec::new();
    for directive in directives {
        for (key, make) in KNOWN {
            let Some(value) = directive.get(key) else {
                continue;
            };
            // A script may be an object naming its executable.
            let value = match (key, value.get("executable")) {
                ("script", Some(executable)) => executable,
                _ => value,
            };
            for item in string_or_list(value) {
                entries.push(match item {
                    Ok(text) => make(text),
                    Err(raw) => ZapEntry::Other(format!("{key}: {raw}")),
                });
            }
        }
        for (key, value) in directive {
            if !KNOWN.iter().any(|(known, _)| known == key) {
                entries.push(ZapEntry::Other(format!("{key}: {value}")));
            }
        }
    }
    entries
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
        zap: Vec::new(),
        cask_dependencies: Vec::new(),
        pinned: f.pinned,
        self_updating: false,
    });
    let casks = info.casks.into_iter().map(|c| Package {
        version: c.installed.unwrap_or_default(),
        app_paths: app_paths(&c.artifacts),
        zap: zap_entries(&c.artifacts),
        cask_dependencies: cask_dependencies(&c.depends_on),
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

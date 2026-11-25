//! Searching the Homebrew catalog: matching names from `brew search`, then
//! their descriptions and latest versions from one `brew info` per kind.

use serde::Deserialize;

use crate::inventory::{Kind, Package};
use crate::system::{Command, System};

/// At most this many results of each kind are shown.
const LIMIT: usize = 40;

/// Search the catalog for `query`. Each result is a Package whose `version`
/// is the latest available one.
pub fn search(system: &impl System, query: &str) -> Result<Vec<Package>, String> {
    let formulae = names(system, "--formula", query)?;
    let casks = names(system, "--cask", query)?;
    let mut results = info(system, Kind::Formula, &formulae)?;
    results.extend(info(system, Kind::Cask, &casks)?);
    Ok(results)
}

/// Matching names of one kind. `brew search` fails when nothing matches,
/// which is just no results.
fn names(system: &impl System, flag: &str, query: &str) -> Result<Vec<String>, String> {
    let output = system
        .run(&Command::brew(&["search", flag, query]))
        .map_err(|e| format!("{e:#}"))?;
    let names: Vec<String> = output
        .stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("==>"))
        .take(LIMIT)
        .map(String::from)
        .collect();
    // `brew search` fails with "No formulae or casks found" (or "No formulae
    // found" / "No casks found") when nothing matches: that is no results.
    // Any other failure is a real error.
    let no_matches = output.stderr.contains("No formulae")
        || output.stderr.contains("No casks")
        || output.stderr.contains("No formulae or casks");
    if !output.success && !no_matches {
        return Err(format!("brew search failed: {}", output.stderr.trim()));
    }
    Ok(names)
}

fn info(system: &impl System, kind: Kind, names: &[String]) -> Result<Vec<Package>, String> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let flag = match kind {
        Kind::Formula => "--formula",
        Kind::Cask => "--cask",
    };
    let mut args = vec!["info", "--json=v2", flag];
    args.extend(names.iter().map(String::as_str));
    let output = system
        .run(&Command::brew(&args))
        .map_err(|e| format!("{e:#}"))?;
    if !output.success {
        return Err(format!("brew info failed: {}", output.stderr.trim()));
    }
    let json: InfoJson = serde_json::from_str(&output.stdout).map_err(|e| e.to_string())?;
    let package = |name: String, desc: Option<String>, version: String| Package {
        name,
        kind,
        version,
        desc,
        app_paths: Vec::new(),
        zap_paths: Vec::new(),
        pinned: false,
        self_updating: false,
    };
    Ok(match kind {
        Kind::Formula => json
            .formulae
            .into_iter()
            .map(|f| package(f.name, f.desc, f.versions.stable.unwrap_or_default()))
            .collect(),
        Kind::Cask => json
            .casks
            .into_iter()
            .map(|c| package(c.token, c.desc, c.version.unwrap_or_default()))
            .collect(),
    })
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
    versions: VersionsJson,
}

#[derive(Deserialize, Default)]
struct VersionsJson {
    stable: Option<String>,
}

#[derive(Deserialize)]
struct CaskJson {
    token: String,
    desc: Option<String>,
    version: Option<String>,
}

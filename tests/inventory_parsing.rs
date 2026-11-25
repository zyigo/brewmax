//! Reading installed Packages from `brew info --json=v2 --installed`, through
//! `load_installed` on a scripted System.

mod support;

use brewmax::inventory::{self, Package, ZapEntry};
use support::{FakeSystem, INSTALLED_CMD, one_cask_json as cask};

fn load(json: &str) -> Vec<Package> {
    inventory::load_installed(&FakeSystem::new().on(INSTALLED_CMD, json)).expect("parses")
}

fn package<'a>(packages: &'a [Package], name: &str) -> &'a Package {
    packages
        .iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("no {name} in {packages:#?}"))
}

#[test]
fn a_casks_cask_dependencies_are_read() {
    let packages = load(&cask(
        "battle-net",
        r#""depends_on": {"cask": ["wine-stable", "xquartz"], "macos": {">=": ["12"]}}"#,
    ));
    assert_eq!(
        package(&packages, "battle-net").cask_dependencies,
        vec!["wine-stable", "xquartz"]
    );
}

#[test]
fn a_cask_without_cask_dependencies_has_none() {
    for extra in [
        "",
        r#""depends_on": {}"#,
        r#""depends_on": {"macos": {">=": ["12"]}}"#,
        r#""depends_on": {"cask": []}"#,
    ] {
        let packages = load(&cask("slack", extra));
        assert!(
            package(&packages, "slack").cask_dependencies.is_empty(),
            "{extra}"
        );
    }
}

#[test]
fn a_single_cask_dependency_given_as_a_string_is_read() {
    let packages = load(&cask("battle-net", r#""depends_on": {"cask": "xquartz"}"#));
    assert_eq!(
        package(&packages, "battle-net").cask_dependencies,
        vec!["xquartz"]
    );
}

fn zap_of(stanza: &str) -> Vec<ZapEntry> {
    let packages = load(&cask(
        "slack",
        &format!(r#""artifacts": [{{"zap": [{stanza}]}}]"#),
    ));
    package(&packages, "slack").zap.clone()
}

#[test]
fn every_zap_directive_kind_is_read_as_a_string_or_a_list() {
    let zap = zap_of(
        r#"{"trash": ["~/Library/A", "~/Library/B*"],
            "delete": "/Library/C",
            "rmdir": "~/Library/D",
            "launchctl": ["com.slack.helper"],
            "pkgutil": "com.slack.pkg",
            "login_item": "Slack"}"#,
    );
    assert_eq!(
        zap,
        vec![
            ZapEntry::Remove("~/Library/A".into()),
            ZapEntry::Remove("~/Library/B*".into()),
            ZapEntry::Remove("/Library/C".into()),
            ZapEntry::RemoveIfEmpty("~/Library/D".into()),
            ZapEntry::Service("com.slack.helper".into()),
            ZapEntry::Receipt("com.slack.pkg".into()),
            ZapEntry::LoginItem("Slack".into()),
        ]
    );
}

#[test]
fn a_zap_script_is_read_as_its_executable_whether_a_string_or_an_object() {
    assert_eq!(
        zap_of(r#"{"script": "/usr/local/bin/cleanup"}"#),
        vec![ZapEntry::Script("/usr/local/bin/cleanup".into())]
    );
    assert_eq!(
        zap_of(
            r#"{"script": {"executable": "/opt/uninstall.sh", "args": ["--all"], "sudo": true}}"#
        ),
        vec![ZapEntry::Script("/opt/uninstall.sh".into())]
    );
}

#[test]
fn unrecognised_zap_directives_and_values_are_kept_as_written() {
    assert_eq!(
        zap_of(r#"{"signal": [["TERM", "com.slack"]], "trash": [42]}"#),
        vec![
            ZapEntry::Other("trash: 42".into()),
            ZapEntry::Other(r#"signal: [["TERM","com.slack"]]"#.into()),
        ]
    );
}

#[test]
fn a_cask_with_no_artifacts_or_no_zap_stanza_has_nothing_to_zap() {
    assert!(package(&load(&cask("slack", "")), "slack").zap.is_empty());
    let packages = load(&cask("slack", r#""artifacts": [{"app": ["Slack.app"]}]"#));
    assert!(package(&packages, "slack").zap.is_empty());
}

#[test]
fn formulae_have_no_cask_dependencies_or_zap() {
    let packages =
        load(r#"{"formulae": [{"name": "jq", "installed": [{"version": "1.8.1"}]}], "casks": []}"#);
    let jq = package(&packages, "jq");
    assert!(jq.cask_dependencies.is_empty());
    assert!(jq.zap.is_empty());
}

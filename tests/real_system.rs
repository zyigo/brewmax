//! Opt-in tests against the real macOS System. They need Homebrew installed
//! and are skipped by default: run with `cargo test -- --ignored`.

use brewmax::inventory::{self, Kind};
use brewmax::system::MacSystem;

#[test]
#[ignore = "needs a real Homebrew installation"]
fn reads_the_real_installed_packages() {
    let packages = inventory::load_installed(&MacSystem).expect("brew info should parse");

    assert!(
        !packages.is_empty(),
        "expected at least one installed package"
    );
    assert!(
        packages.iter().any(|p| p.kind == Kind::Formula),
        "expected at least one Formula"
    );
    for package in &packages {
        assert!(!package.name.is_empty());
        assert!(
            !package.version.is_empty(),
            "{} has no installed version",
            package.name
        );
    }
}

#[test]
#[ignore = "needs a real Homebrew installation"]
fn scans_the_real_machine_for_in_use_packages() {
    let packages = inventory::load_installed(&MacSystem).expect("brew info should parse");
    let targets: Vec<_> = packages.iter().map(brewmax::in_use::Target::from).collect();

    let in_use =
        brewmax::in_use::scan(&MacSystem, &targets).expect("lsof and brew services should run");

    for name in in_use.keys() {
        assert!(
            packages.iter().any(|p| &p.name == name),
            "{name} is not an installed package"
        );
    }
    eprintln!("In Use on this machine: {in_use:#?}");
}

#[test]
#[ignore = "needs a real Homebrew installation and network access"]
fn refreshes_and_reads_the_real_outdated_packages() {
    let packages = inventory::load_installed(&MacSystem).expect("brew info should parse");
    let refresh = brewmax::outdated::refresh(&MacSystem);

    let outdated = refresh.outdated.expect("brew outdated should parse");
    for name in outdated.keys() {
        assert!(
            packages.iter().any(|p| &p.name == name),
            "{name} is not an installed package"
        );
    }
    eprintln!(
        "update error: {:?}\noutdated: {outdated:#?}",
        refresh.update_error
    );
}

#[test]
fn streams_stdout_and_stderr_lines_from_a_real_process_with_its_environment() {
    use brewmax::system::{Command, System};
    let mut command = Command::new(
        "/bin/sh",
        &[
            "-c",
            "echo one; echo two >&2; echo \"env=$BREWMAX_TEST\"; exit 3",
        ],
    );
    command.env.push(("BREWMAX_TEST".into(), "yes".into()));

    let mut lines = Vec::new();
    let success = MacSystem
        .stream(&command, &mut |l| lines.push(l.to_string()))
        .unwrap();

    assert!(!success);
    lines.sort();
    assert_eq!(lines, vec!["env=yes", "one", "two"]);
}

#[test]
#[ignore = "needs a real Homebrew installation with at least one outdated Formula"]
fn plans_a_real_update_without_changing_anything() {
    let packages = inventory::load_installed(&MacSystem).expect("brew info should parse");
    let outdated =
        brewmax::outdated::load_outdated(&MacSystem).expect("brew outdated should parse");
    let Some(target) = packages
        .iter()
        .find(|p| p.kind == Kind::Formula && outdated.contains_key(&p.name))
    else {
        eprintln!("no outdated Formula to plan");
        return;
    };
    let targets: Vec<_> = packages.iter().map(brewmax::in_use::Target::from).collect();

    let (plan, held) = brewmax::operation::plan(&MacSystem, std::slice::from_ref(target), &targets)
        .expect("dry run should parse");

    assert!(
        plan.steps()
            .iter()
            .any(|s| s.name == target.name && !s.to.is_empty()),
        "{plan:#?}"
    );
    eprintln!("plan: {plan:#?}\nheld: {held:#?}");
}

#[test]
#[ignore = "needs a real Homebrew installation; reads, never quits"]
fn finds_which_real_cask_apps_are_running() {
    let packages = inventory::load_installed(&MacSystem).expect("brew info should parse");
    let app_paths: Vec<String> = packages
        .iter()
        .filter(|p| p.kind == Kind::Cask)
        .flat_map(|p| p.app_paths.clone())
        .collect();

    let running = brewmax::in_use::running_apps(&MacSystem, &app_paths).expect("lsof should run");

    for app in &running {
        assert!(app_paths.contains(app));
    }
    eprintln!("running Cask apps: {running:#?}");
}

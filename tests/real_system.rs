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

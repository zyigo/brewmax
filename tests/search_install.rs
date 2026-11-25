mod support;

use std::time::Instant;

use ratatui::crossterm::event::KeyCode;
use support::{FakeSystem, Harness, INSTALLED_CMD};

const UPDATE_CMD: &str = "brew update";
const OUTDATED_CMD: &str = "brew outdated --json=v2 --greedy";
const LSOF_CMD: &str = "lsof -nP -F pcn -d txt";
const SERVICES_CMD: &str = "brew services list --json";

const SEARCH_FORMULAE: &str = "brew search --formula rip";
const SEARCH_CASKS: &str = "brew search --cask rip";
const INFO_FORMULAE: &str = "brew info --json=v2 --formula ripgrep ripgrep-all";
const INFO_CASKS: &str = "brew info --json=v2 --cask ripme";

const DRY_RUN_RIPME: &str = "brew install --dry-run --cask ripme";
const INSTALL_RIPME: &str = "brew install --cask ripme";
const DRY_RUN_RGA: &str = "brew install --dry-run --formula ripgrep-all";
const INSTALL_RGA: &str = "brew install --formula ripgrep-all";

/// ripgrep and pcre2 are installed; pcre2 is outdated.
const INSTALLED_JSON: &str = r#"{
  "formulae": [
    {"name": "pcre2", "desc": "Perl compatible regular expressions library", "installed": [{"version": "10.45"}]},
    {"name": "ripgrep", "desc": "Search tool like grep and The Silver Searcher", "installed": [{"version": "15.1.0"}]}
  ],
  "casks": []
}"#;

const INSTALLED_AFTER_RIPME: &str = r#"{
  "formulae": [
    {"name": "pcre2", "desc": "Perl compatible regular expressions library", "installed": [{"version": "10.45"}]},
    {"name": "ripgrep", "desc": "Search tool like grep and The Silver Searcher", "installed": [{"version": "15.1.0"}]}
  ],
  "casks": [
    {"token": "ripme", "desc": "Album ripper for various websites", "installed": "2.1.20", "artifacts": []}
  ]
}"#;

const INFO_FORMULAE_JSON: &str = r#"{
  "formulae": [
    {"name": "ripgrep", "desc": "Search tool like grep and The Silver Searcher", "versions": {"stable": "15.1.0"}, "installed": [{"version": "15.1.0"}]},
    {"name": "ripgrep-all", "desc": "Wrapper around ripgrep that adds multiple rich file types", "versions": {"stable": "0.10.10"}, "installed": []}
  ],
  "casks": []
}"#;

const INFO_CASKS_JSON: &str = r#"{
  "formulae": [],
  "casks": [
    {"token": "ripme", "desc": "Album ripper for various websites", "version": "2.1.20", "installed": null}
  ]
}"#;

fn machine() -> FakeSystem {
    FakeSystem::new()
        .on(INSTALLED_CMD, INSTALLED_JSON)
        .on(UPDATE_CMD, "Already up-to-date.")
        .on(OUTDATED_CMD, r#"{"formulae": [{"name": "pcre2", "installed_versions": ["10.45"], "current_version": "10.46"}], "casks": []}"#)
        .on(LSOF_CMD, "")
        .on(SERVICES_CMD, "[]")
        .on(SEARCH_FORMULAE, "ripgrep\nripgrep-all\n")
        .on(SEARCH_CASKS, "ripme\n")
        .on(INFO_FORMULAE, INFO_FORMULAE_JSON)
        .on(INFO_CASKS, INFO_CASKS_JSON)
        .on(DRY_RUN_RIPME, "==> Would install 1 cask:\nripme\n")
        .on(INSTALL_RIPME, "==> Installing Cask ripme\n🍺  ripme was successfully installed!\n")
        .on(
            DRY_RUN_RGA,
            "==> Would install 2 formulae:\nripgrep-all 0.10.10 (8MB)\npoppler 25.11.0 (12MB)\n\
             ==> Would upgrade 1 dependency:\npcre2 10.45 -> 10.46\n",
        )
        .on(INSTALL_RGA, "==> Installing ripgrep-all\n")
}

/// Started, on the Search tab, with "rip" searched.
fn searched(system: FakeSystem) -> Harness {
    let mut h = Harness::start(system);
    h.tick_at(Instant::now());
    h.press_char('3');
    h.press_char('/');
    h.type_text("rip");
    h.press(KeyCode::Enter);
    h.settle();
    h
}

fn list_rows(h: &Harness) -> Vec<String> {
    h.screen()
        .lines()
        .filter_map(|l| l.split('│').nth(1))
        .map(|cell| cell.trim().to_string())
        .filter(|cell| !cell.is_empty())
        .collect()
}

fn row(h: &Harness, name: &str) -> String {
    list_rows(h)
        .into_iter()
        .find(|r| r.starts_with(&format!("{name} ")))
        .unwrap_or_else(|| panic!("no row for {name}:\n{}", h.screen()))
}

#[test]
fn searching_shows_matching_casks_and_formulae() {
    let h = searched(machine());

    assert!(h.screen().contains("Search"), "{}", h.screen());
    assert!(row(&h, "ripgrep-all").contains("Formula"), "{}", h.screen());
    assert!(row(&h, "ripme").contains("Cask"), "{}", h.screen());
}

#[test]
fn search_results_show_descriptions_in_the_detail_pane() {
    let mut h = searched(machine());
    h.select("ripme");
    assert!(h.screen().contains("Album ripper"), "{}", h.screen());
}

#[test]
fn already_installed_results_are_marked() {
    let h = searched(machine());
    assert!(row(&h, "ripgrep").contains("Installed"), "{}", h.screen());
    assert!(
        !row(&h, "ripgrep-all").contains("Installed"),
        "{}",
        h.screen()
    );
}

#[test]
fn letters_typed_into_the_search_do_not_trigger_commands() {
    let mut h = Harness::start(machine());
    h.press_char('3');
    h.press_char('/');
    h.type_text("q");
    assert!(!h.app.should_quit());
}

#[test]
fn a_search_with_no_matches_says_so() {
    let system = machine()
        .failing(
            "brew search --formula zzzz",
            "Error: No formulae or casks found for \"zzzz\".",
        )
        .failing(
            "brew search --cask zzzz",
            "Error: No formulae or casks found for \"zzzz\".",
        );
    let mut h = Harness::start(system);
    h.tick_at(Instant::now());
    h.press_char('3');
    h.press_char('/');
    h.type_text("zzzz");
    h.press(KeyCode::Enter);
    h.settle();

    assert!(
        h.screen().contains("No matches for “zzzz”"),
        "{}",
        h.screen()
    );
}

#[test]
fn installing_a_package_with_no_new_dependencies_runs_straight_away() {
    let system = machine();
    let mut h = searched(system.clone());
    system.set(INSTALLED_CMD, INSTALLED_AFTER_RIPME);
    h.select("ripme");
    h.press_char('i');
    h.settle();

    assert_eq!(h.system.count(INSTALL_RIPME), 1);
    assert!(row(&h, "ripme").contains("Installed"), "{}", h.screen());
    h.press_char('1');
    assert!(
        list_rows(&h).iter().any(|r| r.starts_with("ripme ")),
        "{}",
        h.screen()
    );
}

#[test]
fn installing_with_dependencies_shows_the_plan_for_confirmation() {
    let mut h = searched(machine());
    h.select("ripgrep-all");
    h.press_char('i');
    h.settle();

    let screen = h.screen();
    assert!(screen.contains("Confirm"), "{screen}");
    assert!(screen.contains("poppler"), "{screen}");
    assert!(screen.contains("10.45 → 10.46"), "{screen}");
    assert_eq!(h.system.count(INSTALL_RGA), 0);

    h.press_char('n');
    h.settle();
    assert_eq!(h.system.count(INSTALL_RGA), 0);

    h.press_char('i');
    h.settle();
    h.press_char('y');
    h.settle();
    assert_eq!(h.system.count(INSTALL_RGA), 1);
}

#[test]
fn an_install_that_would_upgrade_an_in_use_dependency_is_held() {
    let system = machine().on(
        LSOF_CMD,
        "p12\ncphp\nftxt\nn/opt/homebrew/Cellar/pcre2/10.45/lib/libpcre2-8.0.dylib\n",
    );
    let mut h = searched(system);
    h.select("ripgrep-all");
    h.press_char('i');
    h.settle();

    let screen = h.screen();
    assert!(screen.contains("Held"), "{screen}");
    assert!(
        screen.contains("pcre2 is In Use by php (pid 12)"),
        "{screen}"
    );
    assert_eq!(h.system.count(INSTALL_RGA), 0);
}

#[test]
fn installing_something_already_installed_says_so_and_runs_nothing() {
    let mut h = searched(machine());
    h.select("ripgrep");
    h.press_char('i');
    h.settle();

    assert!(h.screen().contains("already installed"), "{}", h.screen());
    assert!(
        !h.system
            .commands()
            .iter()
            .any(|c| c.starts_with("brew install"))
    );
}

#[test]
fn install_commands_carry_the_safety_environment() {
    let mut h = searched(machine());
    h.select("ripme");
    h.press_char('i');
    h.settle();

    let install = h
        .system
        .sent()
        .into_iter()
        .find(|c| c.to_string() == INSTALL_RIPME)
        .expect("install sent");
    for (key, value) in brewmax::system::BREW_ENV {
        assert!(install.env.contains(&(key.to_string(), value.to_string())));
    }
}

#[test]
fn a_real_search_failure_is_reported_not_shown_as_no_matches() {
    let system = machine()
        .failing(
            "brew search --formula rip",
            "Error: Failure while executing; `git fetch` exited with 128.",
        )
        .failing(
            "brew search --cask rip",
            "Error: Failure while executing; `git fetch` exited with 128.",
        );
    let h = searched(system);

    assert!(h.screen().contains("Search failed"), "{}", h.screen());
    assert!(h.screen().contains("git fetch"), "{}", h.screen());
    assert!(!h.screen().contains("No matches"), "{}", h.screen());
}

#[test]
fn i_does_nothing_outside_the_search_tab() {
    let mut h = Harness::start(machine());
    h.tick_at(Instant::now());
    h.press_char('i');
    h.settle();

    assert!(!h.screen().contains("already installed"), "{}", h.screen());
    assert!(
        !h.system
            .commands()
            .iter()
            .any(|c| c.starts_with("brew install"))
    );
}

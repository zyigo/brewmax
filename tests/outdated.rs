mod support;

use std::time::Instant;

use ratatui::crossterm::event::KeyCode;
use support::{FakeSystem, Harness, INSTALLED_CMD};

const UPDATE_CMD: &str = "brew update";
const OUTDATED_CMD: &str = "brew outdated --json=v2 --greedy";
const LSOF_CMD: &str = "lsof -nP -F pcn -d txt";
const SERVICES_CMD: &str = "brew services list --json";

/// ffmpeg and slack are outdated; gh is outdated but Pinned; 1password is an
/// outdated Self-Updating Cask; jq and dia are current.
const INSTALLED_JSON: &str = r#"{
  "formulae": [
    {"name": "ffmpeg", "desc": "Play, record, convert, and stream audio and video", "installed": [{"version": "7.1_3"}], "pinned": false},
    {"name": "gh", "desc": "GitHub command-line tool", "installed": [{"version": "2.80.0"}], "pinned": true},
    {"name": "jq", "desc": "Lightweight and flexible command-line JSON processor", "installed": [{"version": "1.8.1"}], "pinned": false}
  ],
  "casks": [
    {"token": "1password", "desc": "Password manager", "installed": "8.12.44", "auto_updates": true, "pinned": false, "artifacts": []},
    {"token": "dia", "desc": "Web browser", "installed": "1.58.2", "auto_updates": true, "pinned": false, "artifacts": []},
    {"token": "slack", "desc": "Team communication and collaboration software", "installed": "4.46.0", "auto_updates": false, "pinned": false, "artifacts": []}
  ]
}"#;

const OUTDATED_JSON: &str = r#"{
  "formulae": [
    {"name": "ffmpeg", "installed_versions": ["7.1_3"], "current_version": "7.2", "pinned": false, "pinned_version": null},
    {"name": "gh", "installed_versions": ["2.80.0"], "current_version": "2.83.1", "pinned": true, "pinned_version": "2.80.0"}
  ],
  "casks": [
    {"name": "1password", "installed_versions": ["8.12.44"], "current_version": "8.12.46", "pinned": false, "pinned_version": null},
    {"name": "slack", "installed_versions": ["4.46.0"], "current_version": "4.47.1", "pinned": false, "pinned_version": null}
  ]
}"#;

fn machine() -> FakeSystem {
    FakeSystem::new()
        .on(INSTALLED_CMD, INSTALLED_JSON)
        .on(UPDATE_CMD, "Already up-to-date.")
        .on(OUTDATED_CMD, OUTDATED_JSON)
        .on(LSOF_CMD, "")
        .on(SERVICES_CMD, "[]")
}

/// Started, refreshed, and showing the Outdated tab.
fn outdated_tab() -> Harness {
    let mut h = Harness::start(machine());
    h.tick_at(Instant::now());
    h.press(KeyCode::Tab);
    h
}

/// The list rows (left pane) on screen, trimmed.
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
fn installed_packages_show_before_the_refresh_finishes_with_an_indicator() {
    let system = machine();
    let update = system.hold(UPDATE_CMD);
    let mut h = Harness::start(system);

    h.tick_no_wait(Instant::now());
    let screen = h.screen();
    assert!(screen.contains("ffmpeg"), "{screen}");
    assert!(screen.contains("Refreshing"), "{screen}");

    update.release();
    h.settle();
    assert!(!h.screen().contains("Refreshing"), "{}", h.screen());
    assert_eq!(
        h.system
            .commands()
            .iter()
            .filter(|c| c.starts_with("brew update") || c.starts_with("brew outdated"))
            .collect::<Vec<_>>(),
        vec![UPDATE_CMD, OUTDATED_CMD],
        "brew update must finish before outdated is read"
    );
}

#[test]
fn the_refresh_runs_once_per_launch() {
    let mut h = Harness::start(machine());
    let t0 = Instant::now();
    h.tick_at(t0);
    h.tick_at(t0 + std::time::Duration::from_secs(10));
    assert_eq!(h.system.count(UPDATE_CMD), 1);
}

#[test]
fn outdated_tab_lists_outdated_packages_with_installed_and_available_versions() {
    let h = outdated_tab();

    let screen = h.screen();
    assert!(screen.contains("Outdated"), "{screen}");
    let ffmpeg = row(&h, "ffmpeg");
    assert!(ffmpeg.contains("7.1_3 → 7.2"), "{ffmpeg}");
    let slack = row(&h, "slack");
    assert!(slack.contains("4.46.0 → 4.47.1"), "{slack}");
    assert!(
        !list_rows(&h).iter().any(|r| r.starts_with("jq ")),
        "current packages are not outdated:\n{screen}"
    );
}

#[test]
fn outdated_tab_says_it_is_still_checking_before_the_refresh_finishes() {
    let system = machine();
    let update = system.hold(UPDATE_CMD);
    let mut h = Harness::start(system);
    h.tick_no_wait(Instant::now());
    h.press(KeyCode::Tab);

    assert!(
        h.screen().contains("Checking for updates"),
        "{}",
        h.screen()
    );
    assert!(!list_rows(&h).iter().any(|r| r.starts_with("ffmpeg ")));

    update.release();
    h.settle();
    assert!(row(&h, "ffmpeg").contains("→ 7.2"));
}

#[test]
fn self_updating_casks_are_grouped_separately_under_a_label() {
    let h = outdated_tab();

    let rows = list_rows(&h);
    let label = rows
        .iter()
        .position(|r| r.contains("Self-Updating"))
        .unwrap_or_else(|| panic!("no Self-Updating group:\n{}", h.screen()));
    let position = |name: &str| {
        rows.iter()
            .position(|r| r.starts_with(&format!("{name} ")))
            .unwrap()
    };
    assert!(
        position("ffmpeg") < label && position("slack") < label,
        "{}",
        h.screen()
    );
    assert!(position("1password") > label, "{}", h.screen());
}

#[test]
fn the_selection_skips_the_group_label() {
    let mut h = outdated_tab();

    h.select("slack");
    h.press(KeyCode::Down);
    assert!(h.screen().contains("Password manager"), "{}", h.screen());
}

#[test]
fn pinned_packages_are_marked_in_both_tabs() {
    let mut h = outdated_tab();
    assert!(row(&h, "gh").contains("Pinned"), "{}", h.screen());
    assert!(!row(&h, "ffmpeg").contains("Pinned"));

    h.press_char('1');
    assert!(h.screen().contains("┌ Installed"), "{}", h.screen());
    assert!(row(&h, "gh").contains("Pinned"), "{}", h.screen());
}

#[test]
fn the_detail_pane_shows_the_available_version() {
    let mut h = outdated_tab();
    h.select("ffmpeg");
    assert!(h.screen().contains("Available: 7.2"), "{}", h.screen());
}

#[test]
fn a_failed_brew_update_still_shows_outdated_from_local_data_with_a_warning() {
    let mut h =
        Harness::start(machine().failing(UPDATE_CMD, "Error: Could not resolve host: github.com"));
    h.tick_at(Instant::now());
    h.press(KeyCode::Tab);

    assert!(row(&h, "ffmpeg").contains("→ 7.2"), "{}", h.screen());
    assert!(h.screen().contains("Could not refresh"), "{}", h.screen());
}

#[test]
fn a_failed_outdated_check_says_so() {
    let mut h = Harness::start(machine().failing(OUTDATED_CMD, "Error: broken"));
    h.tick_at(Instant::now());
    h.press(KeyCode::Tab);

    assert!(
        h.screen().contains("Could not check for updates"),
        "{}",
        h.screen()
    );
}

#[test]
fn typing_after_slash_filters_the_current_list_and_escape_clears_it() {
    let mut h = Harness::start(machine());
    h.tick_at(Instant::now());

    h.press_char('/');
    h.type_text("sla");
    let rows = list_rows(&h);
    assert!(
        rows.iter().any(|r| r.starts_with("slack ")),
        "{}",
        h.screen()
    );
    assert!(
        !rows.iter().any(|r| r.starts_with("ffmpeg ")),
        "{}",
        h.screen()
    );
    assert!(!h.screen().contains("Password manager"));
    assert!(
        h.screen().contains("Team communication"),
        "selection follows the filter:\n{}",
        h.screen()
    );

    h.press(KeyCode::Esc);
    assert!(
        list_rows(&h).iter().any(|r| r.starts_with("ffmpeg ")),
        "{}",
        h.screen()
    );
}

#[test]
fn letters_typed_into_the_filter_do_not_trigger_commands() {
    let mut h = Harness::start(machine());
    h.press_char('/');
    h.type_text("q");
    assert!(!h.app.should_quit());
}

#[test]
fn enter_keeps_the_filter_and_returns_to_navigation() {
    let mut h = Harness::start(machine());
    h.tick_at(Instant::now());
    h.press_char('/');
    h.type_text("1p");
    h.press(KeyCode::Enter);

    assert!(
        !list_rows(&h).iter().any(|r| r.starts_with("ffmpeg ")),
        "{}",
        h.screen()
    );
    h.press_char('q');
    assert!(h.app.should_quit());
}

#[test]
fn the_filter_applies_to_the_outdated_tab_too() {
    let mut h = outdated_tab();
    h.press_char('/');
    h.type_text("ff");

    let rows = list_rows(&h);
    assert!(
        rows.iter().any(|r| r.starts_with("ffmpeg ")),
        "{}",
        h.screen()
    );
    assert!(
        !rows.iter().any(|r| r.starts_with("slack ")),
        "{}",
        h.screen()
    );
}

#[test]
fn a_footer_lists_the_key_bindings() {
    let h = Harness::start(machine());
    let footer = h.screen().lines().last().unwrap().to_string();
    for binding in ["Tab", "/", "?", "q"] {
        assert!(
            footer.contains(binding),
            "footer missing {binding}: {footer}"
        );
    }
}

#[test]
fn question_mark_opens_and_closes_the_help_screen() {
    let mut h = Harness::start(machine());
    h.press_char('?');
    assert!(h.screen().contains("Help"), "{}", h.screen());
    assert!(h.screen().contains("Filter"), "{}", h.screen());

    h.press_char('?');
    assert!(!h.screen().contains(" Help "), "{}", h.screen());

    h.press_char('?');
    h.press(KeyCode::Esc);
    assert!(!h.screen().contains(" Help "), "{}", h.screen());
}

#[test]
fn tab_cycles_through_installed_outdated_and_search() {
    let mut h = Harness::start(machine());
    h.press(KeyCode::Tab);
    assert!(h.screen().contains("┌ Outdated"), "{}", h.screen());
    h.press(KeyCode::Tab);
    assert!(h.screen().contains("┌ Search"), "{}", h.screen());
    h.press(KeyCode::Tab);
    assert!(h.screen().contains("┌ Installed"), "{}", h.screen());
}

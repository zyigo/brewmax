mod support;

use ratatui::crossterm::event::KeyCode;
use support::{FakeSystem, Harness, INSTALLED_CMD, INSTALLED_JSON};

#[test]
fn installed_tab_lists_every_package_with_kind_and_version() {
    let h = Harness::start(FakeSystem::new().on(INSTALLED_CMD, INSTALLED_JSON));

    let screen = h.screen();
    assert!(screen.contains("Installed"), "{screen}");
    for (name, kind, version) in [
        ("1password", "Cask", "8.12.44"),
        ("ffmpeg", "Formula", "7.1_3"),
        ("gh", "Formula", "2.80.0"),
    ] {
        let row = screen
            .lines()
            .find(|line| line.contains(name))
            .unwrap_or_else(|| panic!("no row for {name}:\n{screen}"));
        assert!(
            row.contains(kind) && row.contains(version),
            "row for {name} was `{row}`"
        );
    }
    assert_eq!(h.system.commands(), vec![INSTALLED_CMD]);
}

#[test]
fn detail_pane_follows_the_selection() {
    let mut h = Harness::start(FakeSystem::new().on(INSTALLED_CMD, INSTALLED_JSON));

    assert!(h.screen().contains("Password manager"), "{}", h.screen());

    h.press(KeyCode::Down);
    let screen = h.screen();
    assert!(screen.contains("Play, record, convert"), "{screen}");
    assert!(!screen.contains("Password manager"), "{screen}");

    h.press(KeyCode::Char('j'));
    assert!(
        h.screen().contains("GitHub command-line tool"),
        "{}",
        h.screen()
    );

    h.press(KeyCode::Up);
    assert!(
        h.screen().contains("Play, record, convert"),
        "{}",
        h.screen()
    );
}

#[test]
fn selection_stops_at_the_ends_of_the_list() {
    let mut h = Harness::start(FakeSystem::new().on(INSTALLED_CMD, INSTALLED_JSON));

    h.press(KeyCode::Up);
    assert!(h.screen().contains("Password manager"), "{}", h.screen());

    for _ in 0..5 {
        h.press(KeyCode::Down);
    }
    assert!(
        h.screen().contains("GitHub command-line tool"),
        "{}",
        h.screen()
    );
}

#[test]
fn q_quits() {
    let mut h = Harness::start(FakeSystem::new().on(INSTALLED_CMD, INSTALLED_JSON));
    assert!(!h.app.should_quit());

    h.press(KeyCode::Char('q'));
    assert!(h.app.should_quit());
}

#[test]
fn a_failing_brew_is_reported_on_screen_instead_of_an_empty_list() {
    let h = Harness::start(FakeSystem::new().failing(INSTALLED_CMD, "Error: brew is broken"));

    let screen = h.screen();
    assert!(screen.contains("brew is broken"), "{screen}");
}

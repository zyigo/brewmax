mod support;

use std::time::{Duration, Instant};

use ratatui::crossterm::event::KeyCode;
use support::{FakeSystem, Harness, INSTALLED_CMD};

const UPDATE_CMD: &str = "brew update";
const OUTDATED_CMD: &str = "brew outdated --json=v2 --greedy";
const LSOF_CMD: &str = "lsof -nP -F pcn -d txt";
const SERVICES_CMD: &str = "brew services list --json";

const USES_XZ: &str = "brew uses --installed xz";
const USES_JQ: &str = "brew uses --installed jq";
const UNINSTALL_JQ: &str = "brew uninstall --formula jq";
const UNINSTALL_SLACK: &str = "brew uninstall --cask slack";
const ZAP_SLACK: &str = "brew uninstall --zap --cask slack";

const BUNDLE_ID_CMD: &str = "/usr/libexec/PlistBuddy -c Print :CFBundleIdentifier /Applications/Slack.app/Contents/Info.plist";
const QUIT_CMD: &str = "osascript -e tell application id \"com.tinyspeck.slackmacgap\" to quit";

/// jq has no dependents; xz is needed by ffmpeg and libarchive. Slack is a
/// Cask with settings and data it would Zap.
const INSTALLED_JSON: &str = r#"{
  "formulae": [
    {"name": "ffmpeg", "desc": "Play, record, convert, and stream audio and video", "installed": [{"version": "7.1_3"}]},
    {"name": "jq", "desc": "Lightweight and flexible command-line JSON processor", "installed": [{"version": "1.8.1"}]},
    {"name": "libarchive", "desc": "Multi-format archive and compression library", "installed": [{"version": "3.8.1"}]},
    {"name": "xz", "desc": "General-purpose data compression", "installed": [{"version": "5.8.1"}]}
  ],
  "casks": [
    {"token": "slack", "desc": "Team communication and collaboration software", "installed": "4.46.0",
     "artifacts": [
       {"app": ["Slack.app"], "target": "/Applications/Slack.app"},
       {"uninstall": [{"quit": "com.tinyspeck.slackmacgap"}]},
       {"zap": [{"trash": ["~/Library/Application Support/Slack", "~/Library/Caches/com.tinyspeck.slackmacgap*"],
                 "delete": "~/Library/Preferences/com.tinyspeck.slackmacgap.plist",
                 "rmdir": "~/Library/Slack"}]}
     ]}
  ]
}"#;

const INSTALLED_AFTER_JQ: &str = r#"{
  "formulae": [
    {"name": "ffmpeg", "installed": [{"version": "7.1_3"}]},
    {"name": "libarchive", "installed": [{"version": "3.8.1"}]},
    {"name": "xz", "installed": [{"version": "5.8.1"}]}
  ],
  "casks": []
}"#;

const LSOF_SLACK_RUNNING: &str =
    "p77\ncSlack\nftxt\nn/Applications/Slack.app/Contents/MacOS/Slack\n";

fn machine() -> FakeSystem {
    FakeSystem::new()
        .on(INSTALLED_CMD, INSTALLED_JSON)
        .on(UPDATE_CMD, "Already up-to-date.")
        .on(OUTDATED_CMD, r#"{"formulae": [], "casks": []}"#)
        .on(LSOF_CMD, "")
        .on(SERVICES_CMD, "[]")
        .on(USES_JQ, "")
        .on(USES_XZ, "ffmpeg\nlibarchive\n")
        .on(
            UNINSTALL_JQ,
            "Uninstalling /opt/homebrew/Cellar/jq/1.8.1... (19 files, 1.4MB)\n",
        )
        .on(UNINSTALL_SLACK, "==> Uninstalling Cask slack\n")
        .on(
            ZAP_SLACK,
            "==> Uninstalling Cask slack\n==> Trashing files:\n",
        )
}

fn ready(system: FakeSystem, name: &str) -> Harness {
    let mut h = Harness::start(system);
    h.app
        .set_quit_wait(Duration::from_millis(200), Duration::from_millis(10));
    h.tick_at(Instant::now());
    h.select(name);
    h
}

fn sent_uninstall(h: &Harness) -> Vec<String> {
    h.system
        .commands()
        .into_iter()
        .filter(|c| c.starts_with("brew uninstall"))
        .collect()
}

#[test]
fn uninstall_asks_for_confirmation_and_cancelling_sends_nothing() {
    let mut h = ready(machine(), "jq");
    h.press_char('x');
    h.settle();

    let screen = h.screen();
    assert!(screen.contains("Confirm"), "{screen}");
    assert!(screen.contains("Uninstall jq"), "{screen}");
    assert!(sent_uninstall(&h).is_empty());

    h.press_char('n');
    h.settle();
    assert!(sent_uninstall(&h).is_empty());
    assert!(h.screen().contains("cancelled"), "{}", h.screen());

    h.press_char('x');
    h.settle();
    h.press(KeyCode::Esc);
    h.settle();
    assert!(sent_uninstall(&h).is_empty());
}

#[test]
fn confirming_uninstalls_the_package() {
    let system = machine();
    let mut h = ready(system.clone(), "jq");
    system.set(INSTALLED_CMD, INSTALLED_AFTER_JQ);
    h.press_char('x');
    h.settle();
    h.press_char('y');
    h.settle();

    assert_eq!(sent_uninstall(&h), vec![UNINSTALL_JQ]);
    assert!(h.screen().contains("done"), "{}", h.screen());
    assert!(!h.screen().contains("│jq "), "{}", h.screen());
}

#[test]
fn uninstalling_a_cask_whose_app_is_open_is_held_and_offers_graceful_quit() {
    let mut h = ready(machine().on(LSOF_CMD, LSOF_SLACK_RUNNING), "slack");
    h.press_char('x');
    h.settle();

    let screen = h.screen();
    assert!(screen.contains("Held"), "{screen}");
    assert!(
        screen.contains("slack is In Use by Slack (pid 77)"),
        "{screen}"
    );
    assert!(
        screen.contains("press g to quit it gracefully and uninstall"),
        "{screen}"
    );
    assert!(sent_uninstall(&h).is_empty());
}

#[test]
fn uninstalling_a_formula_running_as_a_service_is_held() {
    let services = r#"[{"name": "jq", "status": "started"}]"#;
    let mut h = ready(machine().on(SERVICES_CMD, services), "jq");
    h.press_char('x');
    h.settle();

    let screen = h.screen();
    assert!(screen.contains("Held"), "{screen}");
    assert!(screen.contains("running as a brew service"), "{screen}");
    assert!(!screen.contains("press g"), "{screen}");
    assert!(sent_uninstall(&h).is_empty());
}

#[test]
fn a_package_that_comes_into_use_while_confirming_is_held() {
    let system = machine();
    let mut h = ready(system.clone(), "slack");
    h.press_char('x');
    h.settle();
    assert!(h.screen().contains("Confirm"), "{}", h.screen());

    system.set(LSOF_CMD, LSOF_SLACK_RUNNING);
    h.press_char('y');
    h.settle();

    assert!(sent_uninstall(&h).is_empty());
    assert!(h.screen().contains("Held"), "{}", h.screen());
}

#[test]
fn uninstalling_a_package_others_depend_on_is_refused_and_lists_them() {
    let mut h = ready(machine(), "xz");
    h.press_char('x');
    h.settle();

    let screen = h.screen();
    assert!(screen.contains("Can't uninstall xz"), "{screen}");
    assert!(screen.contains("ffmpeg, libarchive"), "{screen}");
    assert!(!screen.contains("Confirm"), "{screen}");
    h.press_char('y');
    h.settle();
    assert!(sent_uninstall(&h).is_empty());
}

#[test]
fn a_failing_dependents_check_refuses_the_uninstall() {
    let system = machine().failing(USES_JQ, "Error: something went wrong");
    let mut h = ready(system, "jq");
    h.press_char('x');
    h.settle();

    let screen = h.screen();
    assert!(
        screen.contains("Could not check what depends on jq"),
        "{screen}"
    );
    assert!(!screen.contains("Confirm"), "{screen}");
    assert!(sent_uninstall(&h).is_empty());
}

#[test]
fn a_normal_cask_uninstall_never_zaps() {
    let mut h = ready(machine(), "slack");
    h.press_char('x');
    h.settle();
    assert!(
        h.screen().contains("settings and data are kept"),
        "{}",
        h.screen()
    );
    h.press_char('y');
    h.settle();

    assert_eq!(sent_uninstall(&h), vec![UNINSTALL_SLACK]);
    assert!(!h.system.commands().iter().any(|c| c.contains("--zap")));
}

#[test]
fn uninstall_acts_on_the_selected_package_never_the_marked_ones() {
    let mut h = ready(machine(), "ffmpeg");
    h.press_char(' ');
    h.select("jq");
    h.press_char(' ');
    h.press_char('x');
    h.settle();
    h.press_char('y');
    h.settle();

    assert_eq!(sent_uninstall(&h), vec![UNINSTALL_JQ]);
}

#[test]
fn uninstall_commands_carry_the_safety_environment() {
    let mut h = ready(machine(), "jq");
    h.press_char('x');
    h.settle();
    h.press_char('y');
    h.settle();

    let uninstall = h
        .system
        .sent()
        .into_iter()
        .find(|c| c.to_string() == UNINSTALL_JQ)
        .expect("uninstall sent");
    for (key, value) in brewmax::system::BREW_ENV {
        assert!(
            uninstall
                .env
                .contains(&(key.to_string(), value.to_string()))
        );
    }
    assert!(
        uninstall
            .env
            .contains(&("HOMEBREW_NO_AUTOREMOVE".to_string(), "1".to_string())),
        "an uninstall must not remove dependencies behind Cleanup's back"
    );
}

// ---- Zap ---------------------------------------------------------------

#[test]
fn zap_lists_every_path_and_says_it_cannot_be_undone() {
    let mut h = ready(machine(), "slack");
    h.press_char('Z');
    h.settle();

    let screen = h.screen();
    for path in [
        "~/Library/Application Support/Slack",
        "~/Library/Caches/com.tinyspeck.slackmacgap*",
        "~/Library/Preferences/com.tinyspeck.slackmacgap.plist",
        "~/Library/Slack",
    ] {
        assert!(screen.contains(path), "{path} missing:\n{screen}");
    }
    assert!(screen.contains("cannot be undone"), "{screen}");
    assert!(sent_uninstall(&h).is_empty());
}

#[test]
fn the_ordinary_confirmation_does_not_confirm_a_zap() {
    let mut h = ready(machine(), "slack");
    h.press_char('Z');
    h.settle();
    h.press_char('y');
    h.settle();
    h.press(KeyCode::Enter);
    h.settle();

    assert!(sent_uninstall(&h).is_empty());
    assert!(h.screen().contains("Confirm"), "{}", h.screen());
}

#[test]
fn typing_the_casks_name_confirms_the_zap() {
    let mut h = ready(machine(), "slack");
    h.press_char('Z');
    h.settle();
    h.type_text("slack");
    h.press(KeyCode::Enter);
    h.settle();

    assert_eq!(sent_uninstall(&h), vec![ZAP_SLACK]);
}

#[test]
fn cancelling_a_zap_sends_nothing() {
    let mut h = ready(machine(), "slack");
    h.press_char('Z');
    h.settle();
    h.type_text("sla");
    h.press(KeyCode::Esc);
    h.settle();

    assert!(sent_uninstall(&h).is_empty());
    assert!(h.screen().contains("cancelled"), "{}", h.screen());
}

#[test]
fn a_zap_list_too_long_for_the_screen_scrolls_to_show_every_path() {
    let paths: Vec<String> = (1..=40)
        .map(|i| format!("\"~/Library/Junk/item-{i:02}\""))
        .collect();
    let json = format!(
        r#"{{"formulae": [], "casks": [{{"token": "slack", "installed": "4.46.0",
            "artifacts": [{{"zap": [{{"trash": [{}]}}]}}]}}]}}"#,
        paths.join(",")
    );
    let mut h = ready(machine().on(INSTALLED_CMD, &json), "slack");
    h.press_char('Z');
    h.settle();

    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..60 {
        for i in 1..=40 {
            if h.screen().contains(&format!("item-{i:02}")) {
                seen.insert(i);
            }
        }
        h.press(KeyCode::Down);
    }
    assert_eq!(seen.len(), 40, "unseen paths; last screen:\n{}", h.screen());
}

#[test]
fn zap_is_refused_for_a_formula() {
    let mut h = ready(machine(), "jq");
    h.press_char('Z');
    h.settle();

    assert!(
        h.screen().contains("Zap is only for Casks"),
        "{}",
        h.screen()
    );
    assert!(sent_uninstall(&h).is_empty());
}

#[test]
fn zapping_an_in_use_cask_is_held() {
    let mut h = ready(machine().on(LSOF_CMD, LSOF_SLACK_RUNNING), "slack");
    h.press_char('Z');
    h.settle();

    assert!(h.screen().contains("Held"), "{}", h.screen());
    assert!(sent_uninstall(&h).is_empty());
}

// ---- Graceful Quit then uninstall --------------------------------------

/// Slack is open, and quits when asked.
fn machine_where_open_slack_quits() -> FakeSystem {
    let system = machine()
        .on(LSOF_CMD, LSOF_SLACK_RUNNING)
        .on(BUNDLE_ID_CMD, "com.tinyspeck.slackmacgap\n")
        .on(QUIT_CMD, "")
        .on("open -a /Applications/Slack.app", "");
    let after_quit = system.clone();
    system.on_run(QUIT_CMD, move || after_quit.set(LSOF_CMD, ""))
}

#[test]
fn g_after_a_held_uninstall_confirms_then_quits_the_app_and_uninstalls() {
    let mut h = ready(machine_where_open_slack_quits(), "slack");
    h.press_char('x');
    h.settle();
    h.press_char('g');
    h.settle();

    assert!(h.screen().contains("Confirm"), "{}", h.screen());
    assert_eq!(h.system.count(QUIT_CMD), 0, "quit before confirming");

    h.press_char('y');
    h.settle();

    let commands = h.system.commands();
    let quit = commands
        .iter()
        .position(|c| c == QUIT_CMD)
        .expect("quit sent");
    let uninstall = commands
        .iter()
        .position(|c| c == UNINSTALL_SLACK)
        .expect("uninstall sent");
    assert!(quit < uninstall, "{commands:#?}");
    assert!(
        !commands.iter().any(|c| c.starts_with("open ")),
        "{commands:#?}"
    );
    assert!(
        !commands.iter().any(|c| c.starts_with("brew upgrade")),
        "{commands:#?}"
    );
}

#[test]
fn g_then_cancel_quits_nothing() {
    let mut h = ready(machine_where_open_slack_quits(), "slack");
    h.press_char('x');
    h.settle();
    h.press_char('g');
    h.settle();
    h.press_char('n');
    h.settle();

    assert_eq!(h.system.count(QUIT_CMD), 0);
    assert!(sent_uninstall(&h).is_empty());
}

#[test]
fn if_the_app_does_not_quit_the_uninstall_stays_held() {
    let system = machine()
        .on(LSOF_CMD, LSOF_SLACK_RUNNING)
        .on(BUNDLE_ID_CMD, "com.tinyspeck.slackmacgap\n")
        .on(QUIT_CMD, "");
    let mut h = ready(system, "slack");
    h.press_char('x');
    h.settle();
    h.press_char('g');
    h.settle();
    h.press_char('y');
    h.settle();

    assert_eq!(h.system.count(QUIT_CMD), 1);
    assert!(sent_uninstall(&h).is_empty());
    assert!(h.screen().contains("Held"), "{}", h.screen());
}

#[test]
fn g_after_a_held_zap_still_needs_the_cask_name() {
    let mut h = ready(machine_where_open_slack_quits(), "slack");
    h.press_char('Z');
    h.settle();
    h.press_char('g');
    h.settle();
    h.press_char('y');
    h.settle();
    assert_eq!(h.system.count(QUIT_CMD), 0);

    h.press(KeyCode::Backspace);
    h.type_text("slack");
    h.press(KeyCode::Enter);
    h.settle();
    assert_eq!(sent_uninstall(&h), vec![ZAP_SLACK]);
    assert!(h.system.count(QUIT_CMD) == 1);
}

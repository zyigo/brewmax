mod support;

use std::time::{Duration, Instant};

use support::{FakeSystem, Harness, INSTALLED_CMD};

const UPDATE_CMD: &str = "brew update";
const OUTDATED_CMD: &str = "brew outdated --json=v2 --greedy";
const LSOF_CMD: &str = "lsof -nP -F pcn -d txt";
const SERVICES_CMD: &str = "brew services list --json";

const BUNDLE_ID_CMD: &str = "/usr/libexec/PlistBuddy -c Print :CFBundleIdentifier /Applications/Slack.app/Contents/Info.plist";
const QUIT_CMD: &str = "osascript -e tell application id \"com.tinyspeck.slackmacgap\" to quit";
const REOPEN_CMD: &str = "open -a /Applications/Slack.app";
const DRY_RUN_SLACK: &str = "brew upgrade --dry-run --cask slack";
const UPGRADE_SLACK: &str = "brew upgrade --cask slack";

const INSTALLED_JSON: &str = r#"{
  "formulae": [
    {"name": "uv", "desc": "Extremely fast Python package installer", "installed": [{"version": "0.13.9"}]}
  ],
  "casks": [
    {"token": "slack", "desc": "Team communication and collaboration software", "installed": "4.46.0",
     "artifacts": [{"app": ["Slack.app"], "target": "/Applications/Slack.app"}]},
    {"token": "zed", "desc": "Code editor", "installed": "0.200.0",
     "artifacts": [{"app": ["Zed.app"], "target": "/Applications/Zed.app"}]}
  ]
}"#;

const OUTDATED_JSON: &str = r#"{
  "formulae": [{"name": "uv", "installed_versions": ["0.13.9"], "current_version": "0.13.10", "pinned": false}],
  "casks": [{"name": "slack", "installed_versions": ["4.46.0"], "current_version": "4.47.1", "pinned": false}]
}"#;

/// Slack (and its helper) and Zed are running.
const LSOF_SLACK_RUNNING: &str = "\
p77
cSlack
ftxt
n/Applications/Slack.app/Contents/MacOS/Slack
p78
cSlack Helper
ftxt
n/Applications/Slack.app/Contents/Frameworks/Slack Helper.app/Contents/MacOS/Slack Helper
p90
czed
ftxt
n/Applications/Zed.app/Contents/MacOS/zed
";

const LSOF_SLACK_QUIT: &str = "\
p90
czed
ftxt
n/Applications/Zed.app/Contents/MacOS/zed
";

fn machine() -> FakeSystem {
    FakeSystem::new()
        .on(INSTALLED_CMD, INSTALLED_JSON)
        .on(UPDATE_CMD, "Already up-to-date.")
        .on(OUTDATED_CMD, OUTDATED_JSON)
        .on(LSOF_CMD, LSOF_SLACK_RUNNING)
        .on(SERVICES_CMD, "[]")
        .on(BUNDLE_ID_CMD, "com.tinyspeck.slackmacgap\n")
        .on(QUIT_CMD, "")
        .on(
            DRY_RUN_SLACK,
            "==> Would upgrade 1 outdated package\nslack 4.46.0 -> 4.47.1\n",
        )
        .on(UPGRADE_SLACK, "==> Upgrading slack\n")
        .on(REOPEN_CMD, "")
}

/// Slack quits when asked.
fn machine_where_slack_quits() -> FakeSystem {
    let system = machine();
    let after_quit = system.clone();
    system.on_run(QUIT_CMD, move || after_quit.set(LSOF_CMD, LSOF_SLACK_QUIT))
}

fn ready(system: FakeSystem, name: &str) -> Harness {
    let mut h = Harness::start(system);
    h.app
        .set_quit_wait(Duration::from_millis(200), Duration::from_millis(10));
    h.tick_at(Instant::now());
    h.select(name);
    h
}

fn position(h: &Harness, command: &str) -> usize {
    h.system
        .commands()
        .iter()
        .position(|c| c == command)
        .unwrap_or_else(|| panic!("`{command}` was never sent: {:#?}", h.system.commands()))
}

fn never_force_killed(h: &Harness) {
    for command in h.system.sent() {
        assert!(
            !["kill", "pkill", "killall"].contains(&command.program.as_str()),
            "force-killed with `{command}`"
        );
    }
}

#[test]
fn quit_and_update_asks_the_app_to_quit_waits_upgrades_then_reopens_it() {
    let mut h = ready(machine_where_slack_quits(), "slack");
    h.press_char('g');
    h.settle();

    let quit = position(&h, QUIT_CMD);
    let upgrade = position(&h, UPGRADE_SLACK);
    let reopen = position(&h, REOPEN_CMD);
    assert!(
        quit < upgrade && upgrade < reopen,
        "{:#?}",
        h.system.commands()
    );
    assert!(h.screen().contains("done"), "{}", h.screen());
    never_force_killed(&h);
}

#[test]
fn only_apps_brewmax_quit_are_reopened() {
    let mut h = ready(machine_where_slack_quits(), "slack");
    h.press_char('g');
    h.settle();

    let opened: Vec<_> = h
        .system
        .commands()
        .into_iter()
        .filter(|c| c.starts_with("open "))
        .collect();
    assert_eq!(opened, vec![REOPEN_CMD]);
}

#[test]
fn an_app_that_does_not_quit_leaves_the_cask_held_and_unchanged() {
    // osascript succeeds but Slack is still running when the wait runs out
    // (e.g. a save prompt is still open).
    let mut h = ready(machine(), "slack");
    h.press_char('g');
    h.settle();

    assert_eq!(h.system.count(QUIT_CMD), 1);
    assert_eq!(h.system.count(UPGRADE_SLACK), 0);
    assert_eq!(h.system.count(REOPEN_CMD), 0);
    let screen = h.screen();
    assert!(screen.contains("Held"), "{screen}");
    assert!(screen.contains("did not quit"), "{screen}");
    never_force_killed(&h);
}

#[test]
fn cancelling_the_apps_save_prompt_leaves_the_cask_held_and_unchanged() {
    let system = machine().failing(QUIT_CMD, "execution error: User canceled. (-128)");
    let mut h = ready(system, "slack");
    h.press_char('g');
    h.settle();

    assert_eq!(h.system.count(UPGRADE_SLACK), 0);
    assert_eq!(h.system.count(REOPEN_CMD), 0);
    let screen = h.screen();
    assert!(screen.contains("Held"), "{screen}");
    assert!(screen.contains("User canceled"), "{screen}");
    never_force_killed(&h);
}

#[test]
fn graceful_quit_is_only_offered_for_an_in_use_cask() {
    let mut h = ready(machine(), "uv");
    h.press_char('g');
    h.settle();
    assert!(
        h.screen()
            .contains("Graceful Quit is for a Cask that is In Use"),
        "{}",
        h.screen()
    );
    assert!(
        !h.system
            .commands()
            .iter()
            .any(|c| c.starts_with("osascript"))
    );
}

#[test]
fn a_held_cask_suggests_graceful_quit() {
    let mut h = ready(machine(), "slack");
    h.press_char('u');
    h.settle();
    assert!(h.screen().contains("press g to quit it"), "{}", h.screen());
}

#[test]
fn an_ordinary_update_never_opens_any_app() {
    let mut h = ready(machine().on(LSOF_CMD, LSOF_SLACK_QUIT), "slack");
    h.press_char('u');
    h.settle();

    assert_eq!(h.system.count(UPGRADE_SLACK), 1);
    assert!(!h.system.commands().iter().any(|c| c.starts_with("open ")));
}

#[test]
fn a_failing_in_use_check_while_waiting_never_counts_as_the_app_having_quit() {
    let system = machine();
    let after_quit = system.clone();
    // After the quit request, lsof stops working.
    let system = system.on_run(QUIT_CMD, move || after_quit.remove(LSOF_CMD));
    let mut h = ready(system, "slack");
    h.press_char('g');
    h.settle();

    assert_eq!(
        h.system.count(UPGRADE_SLACK),
        0,
        "{:#?}",
        h.system.commands()
    );
    assert!(h.screen().contains("Held"), "{}", h.screen());
}

const TWO_APPS_JSON: &str = r#"{
  "formulae": [],
  "casks": [
    {"token": "office", "desc": "Office suite", "installed": "16.90",
     "artifacts": [{"app": ["Word.app"], "target": "/Applications/Word.app"},
                   {"app": ["Excel.app"], "target": "/Applications/Excel.app"}]}
  ]
}"#;

#[test]
fn when_one_app_quits_and_another_does_not_the_one_that_quit_is_reopened() {
    let both = "p1\ncWord\nftxt\nn/Applications/Word.app/Contents/MacOS/Word\n\
                p2\ncExcel\nftxt\nn/Applications/Excel.app/Contents/MacOS/Excel\n";
    let excel_only = "p2\ncExcel\nftxt\nn/Applications/Excel.app/Contents/MacOS/Excel\n";
    let quit_word = "osascript -e tell application id \"com.microsoft.Word\" to quit";
    let quit_excel = "osascript -e tell application id \"com.microsoft.Excel\" to quit";
    let system = FakeSystem::new()
        .on(INSTALLED_CMD, TWO_APPS_JSON)
        .on(UPDATE_CMD, "")
        .on(OUTDATED_CMD, r#"{"formulae": [], "casks": [{"name": "office", "installed_versions": ["16.90"], "current_version": "16.91"}]}"#)
        .on(LSOF_CMD, both)
        .on(SERVICES_CMD, "[]")
        .on("/usr/libexec/PlistBuddy -c Print :CFBundleIdentifier /Applications/Word.app/Contents/Info.plist", "com.microsoft.Word")
        .on("/usr/libexec/PlistBuddy -c Print :CFBundleIdentifier /Applications/Excel.app/Contents/Info.plist", "com.microsoft.Excel")
        .on(quit_word, "")
        .on(quit_excel, "")
        .on("open -a /Applications/Word.app", "");
    let after_quit = system.clone();
    // Word quits; Excel keeps a save prompt open.
    let system = system.on_run(quit_word, move || after_quit.set(LSOF_CMD, excel_only));
    let mut h = ready(system, "office");
    h.press_char('g');
    h.settle();

    assert!(
        !h.system
            .commands()
            .iter()
            .any(|c| c.contains("brew upgrade --cask office"))
    );
    let opened: Vec<_> = h
        .system
        .commands()
        .into_iter()
        .filter(|c| c.starts_with("open "))
        .collect();
    assert_eq!(opened, vec!["open -a /Applications/Word.app"]);
}

#[test]
fn a_cask_in_use_by_something_other_than_its_app_is_not_offered_graceful_quit() {
    // Slack's app isn't running, but a process uses a file from its Caskroom.
    let lsof = "p5\ncslack-cli\nftxt\nn/opt/homebrew/Caskroom/slack/4.46.0/slack-cli\n";
    let mut h = ready(machine().on(LSOF_CMD, lsof), "slack");
    h.press_char('u');
    h.settle();
    assert!(h.screen().contains("Held"), "{}", h.screen());
    assert!(!h.screen().contains("press g"), "{}", h.screen());

    h.press_char('g');
    h.settle();
    assert!(
        h.screen()
            .contains("Graceful Quit is for a Cask that is In Use"),
        "{}",
        h.screen()
    );
    assert!(
        !h.system
            .commands()
            .iter()
            .any(|c| c.starts_with("osascript"))
    );
}

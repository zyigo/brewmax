mod support;

use std::time::Instant;

use brewmax::system::BREW_ENV;
use support::{FakeSystem, Harness, INSTALLED_CMD};

const UPDATE_CMD: &str = "brew update";
const OUTDATED_CMD: &str = "brew outdated --json=v2 --greedy";
const LSOF_CMD: &str = "lsof -nP -F pcn -d txt";
const SERVICES_CMD: &str = "brew services list --json";

const DRY_RUN_YTDLP: &str = "brew upgrade --dry-run --formula yt-dlp";
const UPGRADE_YTDLP: &str = "brew upgrade --formula yt-dlp";
const DRY_RUN_FFMPEG: &str = "brew upgrade --dry-run --formula ffmpeg";
const UPGRADE_FFMPEG: &str = "brew upgrade --formula ffmpeg";
const DRY_RUN_SLACK: &str = "brew upgrade --dry-run --cask slack";
const UPGRADE_SLACK: &str = "brew upgrade --cask slack";

/// yt-dlp, ffmpeg (with its dependency x265), slack and brewmax are outdated;
/// gh is outdated but Pinned; jq is current.
const INSTALLED_JSON: &str = r#"{
  "formulae": [
    {"name": "brewmax", "desc": "Homebrew TUI", "installed": [{"version": "0.1.0"}]},
    {"name": "ffmpeg", "desc": "Play, record, convert, and stream audio and video", "installed": [{"version": "7.1_3"}]},
    {"name": "gh", "desc": "GitHub command-line tool", "installed": [{"version": "2.80.0"}], "pinned": true},
    {"name": "jq", "desc": "Lightweight and flexible command-line JSON processor", "installed": [{"version": "1.8.1"}]},
    {"name": "x265", "desc": "H.265/HEVC encoder", "installed": [{"version": "4.1"}]},
    {"name": "yt-dlp", "desc": "Feature-rich command-line audio/video downloader", "installed": [{"version": "2026.10.26"}]}
  ],
  "casks": [
    {"token": "slack", "desc": "Team communication and collaboration software", "installed": "4.46.0",
     "artifacts": [{"app": ["Slack.app"], "target": "/Applications/Slack.app"}]}
  ]
}"#;

const OUTDATED_JSON: &str = r#"{
  "formulae": [
    {"name": "brewmax", "installed_versions": ["0.1.0"], "current_version": "0.2.0", "pinned": false},
    {"name": "ffmpeg", "installed_versions": ["7.1_3"], "current_version": "7.2", "pinned": false},
    {"name": "gh", "installed_versions": ["2.80.0"], "current_version": "2.83.1", "pinned": true},
    {"name": "x265", "installed_versions": ["4.1"], "current_version": "4.2", "pinned": false},
    {"name": "yt-dlp", "installed_versions": ["2026.10.26"], "current_version": "2026.11.23", "pinned": false}
  ],
  "casks": [
    {"name": "slack", "installed_versions": ["4.46.0"], "current_version": "4.47.1", "pinned": false}
  ]
}"#;

/// Real `brew upgrade --dry-run` output shapes.
const DRY_RUN_YTDLP_OUT: &str = "\
==> Downloading bottle manifests
✔︎ Bottle Manifest yt-dlp (2026.11.23)
==> Would upgrade 1 requested outdated package
yt-dlp 2026.10.26 -> 2026.11.23 (5.5MB)
";

const DRY_RUN_FFMPEG_OUT: &str = "\
==> Would upgrade 1 requested outdated package
ffmpeg 7.1_3 -> 7.2 (24.1MB)
==> Would upgrade 1 outdated dependency of ffmpeg
x265 4.1 -> 4.2 (1.9MB)
";

const DRY_RUN_SLACK_OUT: &str = "\
==> Would upgrade 1 requested outdated package
slack 4.46.0 -> 4.47.1
";

const UPGRADE_YTDLP_OUT: &str = "\
==> Pouring yt-dlp--2026.11.23.arm64_golden_gate.bottle.tar.gz
🍺  /opt/homebrew/Cellar/yt-dlp/2026.11.23: 1,024 files, 22MB
";

fn machine() -> FakeSystem {
    FakeSystem::new()
        .on(INSTALLED_CMD, INSTALLED_JSON)
        .on(UPDATE_CMD, "Already up-to-date.")
        .on(OUTDATED_CMD, OUTDATED_JSON)
        .on(LSOF_CMD, "")
        .on(SERVICES_CMD, "[]")
        .on(DRY_RUN_YTDLP, DRY_RUN_YTDLP_OUT)
        .on(UPGRADE_YTDLP, UPGRADE_YTDLP_OUT)
        .on(DRY_RUN_FFMPEG, DRY_RUN_FFMPEG_OUT)
        .on(UPGRADE_FFMPEG, "==> Upgrading ffmpeg\n")
        .on(DRY_RUN_SLACK, DRY_RUN_SLACK_OUT)
        .on(UPGRADE_SLACK, "==> Upgrading slack\n")
}

/// Started, refreshed and scanned on `system`, with `name` selected.
fn ready(system: FakeSystem, name: &str) -> Harness {
    let mut h = Harness::start(system);
    h.tick_at(Instant::now());
    h.select(name);
    h
}

#[test]
fn updating_a_package_with_no_dependency_changes_runs_straight_away() {
    let system = machine();
    let mut h = ready(system.clone(), "yt-dlp");
    // After the upgrade, brew reports the new version.
    system.set(
        INSTALLED_CMD,
        &INSTALLED_JSON.replace(
            r#"[{"version": "2026.10.26"}]"#,
            r#"[{"version": "2026.11.23"}]"#,
        ),
    );
    system.set(
        OUTDATED_CMD,
        &OUTDATED_JSON.replace("yt-dlp", "not-installed"),
    );

    h.press_char('u');
    h.settle();

    let commands = h.system.commands();
    let dry_run = commands
        .iter()
        .position(|c| c == DRY_RUN_YTDLP)
        .expect("dry run");
    let upgrade = commands
        .iter()
        .position(|c| c == UPGRADE_YTDLP)
        .expect("upgrade");
    assert!(dry_run < upgrade);
    assert!(h.screen().contains("2026.11.23"), "{}", h.screen());
    assert!(!h.screen().contains("Confirm"), "{}", h.screen());
}

#[test]
fn every_brew_command_carries_the_safety_environment() {
    let mut h = ready(machine(), "yt-dlp");
    h.press_char('u');
    h.settle();

    let brew: Vec<_> = h
        .system
        .sent()
        .into_iter()
        .filter(|c| c.program == "brew")
        .collect();
    assert!(brew.iter().any(|c| c.to_string() == UPGRADE_YTDLP));
    for command in brew {
        for (key, value) in BREW_ENV {
            assert!(
                command.env.contains(&(key.to_string(), value.to_string())),
                "`{command}` is missing {key}={value}"
            );
        }
    }
}

#[test]
fn a_plan_with_more_than_one_package_is_shown_for_confirmation_and_no_cancels_it() {
    let mut h = ready(machine(), "ffmpeg");
    h.press_char('u');
    h.settle();

    let screen = h.screen();
    assert!(screen.contains("Confirm"), "{screen}");
    assert!(
        screen.contains("ffmpeg") && screen.contains("7.1_3 → 7.2"),
        "{screen}"
    );
    assert!(
        screen.contains("x265") && screen.contains("4.1 → 4.2"),
        "{screen}"
    );
    assert_eq!(
        h.system.count(UPGRADE_FFMPEG),
        0,
        "nothing runs before confirming"
    );

    h.press_char('n');
    h.settle();
    assert_eq!(h.system.count(UPGRADE_FFMPEG), 0);
    assert!(h.screen().contains("cancelled"), "{}", h.screen());
}

#[test]
fn confirming_a_multi_package_plan_runs_it() {
    let mut h = ready(machine(), "ffmpeg");
    h.press_char('u');
    h.settle();
    h.press_char('y');
    h.settle();

    assert_eq!(h.system.count(UPGRADE_FFMPEG), 1);
    assert!(h.screen().contains("done"), "{}", h.screen());
}

#[test]
fn an_in_use_target_is_held_with_the_reason_and_never_upgraded() {
    let system = machine().on(
        LSOF_CMD,
        "p77\ncSlack\nftxt\nn/Applications/Slack.app/Contents/MacOS/Slack\n",
    );
    let mut h = ready(system, "slack");
    h.press_char('u');
    h.settle();

    let screen = h.screen();
    assert!(screen.contains("Held"), "{screen}");
    assert!(screen.contains("Slack (pid 77)"), "{screen}");
    assert_eq!(h.system.count(UPGRADE_SLACK), 0);
}

#[test]
fn a_plan_is_held_when_a_dependency_in_it_is_in_use() {
    let system = machine().on(
        LSOF_CMD,
        "p950\ncobs\nftxt\nn/opt/homebrew/Cellar/x265/4.1/lib/libx265.215.dylib\n",
    );
    let mut h = ready(system, "ffmpeg");
    h.press_char('u');
    h.settle();

    let screen = h.screen();
    assert!(screen.contains("Held"), "{screen}");
    assert!(
        screen.contains("x265 is In Use by obs (pid 950)"),
        "{screen}"
    );
    assert!(
        !screen.contains("Confirm"),
        "a Held plan is not offered for confirmation:\n{screen}"
    );
    assert_eq!(h.system.count(UPGRADE_FFMPEG), 0);
}

#[test]
fn something_opened_while_confirming_is_caught_by_the_check_just_before_running() {
    let system = machine();
    let mut h = ready(system.clone(), "ffmpeg");
    h.press_char('u');
    h.settle();

    // ffmpeg starts being used while the confirmation is on screen.
    system.set(
        LSOF_CMD,
        "p31\ncffmpeg\nftxt\nn/opt/homebrew/Cellar/ffmpeg/7.1_3/bin/ffmpeg\n",
    );
    h.press_char('y');
    h.settle();

    assert!(h.screen().contains("Held"), "{}", h.screen());
    assert_eq!(h.system.count(UPGRADE_FFMPEG), 0);
}

#[test]
fn brew_output_appears_in_the_log_pane() {
    let mut h = ready(machine(), "yt-dlp");
    h.press_char('u');
    h.settle();

    let screen = h.screen();
    assert!(screen.contains("Update yt-dlp"), "{screen}");
    assert!(screen.contains("Pouring yt-dlp--2026.11.23"), "{screen}");
}

#[test]
fn a_second_operation_cannot_start_and_quitting_is_blocked_while_one_runs() {
    let system = machine();
    let upgrade = system.hold(UPGRADE_YTDLP);
    let mut h = ready(system, "yt-dlp");
    h.press_char('u');
    // Let planning finish; the upgrade itself is held mid-flight.
    for _ in 0..100 {
        h.tick_no_wait(Instant::now());
        if h.system.count(UPGRADE_YTDLP) == 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(h.system.count(UPGRADE_YTDLP), 1);

    h.select("slack");
    h.press_char('u');
    assert!(h.screen().contains("Busy"), "{}", h.screen());
    assert_eq!(h.system.count(DRY_RUN_SLACK), 0);

    h.press_char('q');
    assert!(!h.app.should_quit());
    assert!(h.screen().contains("Can't quit"), "{}", h.screen());

    upgrade.release();
    h.settle();
    h.press_char('q');
    assert!(h.app.should_quit());
}

#[test]
fn brewmax_holds_its_own_update_with_a_hint() {
    let mut h = ready(machine(), "brewmax");
    h.press_char('u');
    h.settle();

    assert!(
        h.screen().contains("brew upgrade brewmax"),
        "{}",
        h.screen()
    );
    assert!(
        !h.system
            .commands()
            .iter()
            .any(|c| c.contains("upgrade") && c.contains("brewmax"))
    );
}

#[test]
fn a_failed_upgrade_is_reported() {
    let system = machine().failing(UPGRADE_YTDLP, "Error: Download failed: yt-dlp");
    let mut h = ready(system, "yt-dlp");
    h.press_char('u');
    h.settle();

    let screen = h.screen();
    assert!(screen.contains("✘"), "{screen}");
    assert!(screen.contains("Download failed"), "{screen}");
}

#[test]
fn updating_an_up_to_date_package_says_so_and_runs_nothing() {
    let mut h = ready(machine(), "jq");
    h.press_char('u');
    h.settle();

    assert!(h.screen().contains("already up to date"), "{}", h.screen());
    assert!(!h.system.commands().iter().any(|c| c.contains("upgrade")));
}

#[test]
fn a_pinned_package_is_not_updated() {
    let mut h = ready(machine(), "gh");
    h.press_char('u');
    h.settle();

    assert!(h.screen().contains("Pinned"), "{}", h.screen());
    assert!(!h.system.commands().iter().any(|c| c.contains("upgrade")));
}

mod support;

use std::time::{Duration, Instant};

use ratatui::crossterm::event::KeyCode;
use support::{FakeSystem, Harness, INSTALLED_CMD};

const UPDATE_CMD: &str = "brew update";
const OUTDATED_CMD: &str = "brew outdated --json=v2 --greedy";
const LSOF_CMD: &str = "lsof -nP -F pcn -d txt";
const SERVICES_CMD: &str = "brew services list --json";

fn dry_run(flag: &str, name: &str) -> String {
    format!("brew upgrade --dry-run {flag} {name}")
}
fn upgrade(flag: &str, name: &str) -> String {
    format!("brew upgrade {flag} {name}")
}

/// Outdated: ffmpeg (bringing its outdated dependency x265), x265, uv,
/// yt-dlp, slack, 1password (Self-Updating) and gh (Pinned). jq is current.
const INSTALLED_JSON: &str = r#"{
  "formulae": [
    {"name": "ffmpeg", "desc": "Play, record, convert, and stream audio and video", "installed": [{"version": "7.1_3"}]},
    {"name": "gh", "desc": "GitHub command-line tool", "installed": [{"version": "2.80.0"}], "pinned": true},
    {"name": "jq", "desc": "Lightweight and flexible command-line JSON processor", "installed": [{"version": "1.8.1"}]},
    {"name": "uv", "desc": "Extremely fast Python package installer", "installed": [{"version": "0.13.9"}]},
    {"name": "x265", "desc": "H.265/HEVC encoder", "installed": [{"version": "4.1"}]},
    {"name": "yt-dlp", "desc": "Feature-rich command-line audio/video downloader", "installed": [{"version": "2026.10.26"}]}
  ],
  "casks": [
    {"token": "1password", "desc": "Password manager", "installed": "8.12.44", "auto_updates": true,
     "artifacts": [{"app": ["1Password.app"], "target": "/Applications/1Password.app"}]},
    {"token": "slack", "desc": "Team communication and collaboration software", "installed": "4.46.0",
     "artifacts": [{"app": ["Slack.app"], "target": "/Applications/Slack.app"}]}
  ]
}"#;

const OUTDATED_JSON: &str = r#"{
  "formulae": [
    {"name": "ffmpeg", "installed_versions": ["7.1_3"], "current_version": "7.2", "pinned": false},
    {"name": "gh", "installed_versions": ["2.80.0"], "current_version": "2.83.1", "pinned": true},
    {"name": "uv", "installed_versions": ["0.13.9"], "current_version": "0.13.10", "pinned": false},
    {"name": "x265", "installed_versions": ["4.1"], "current_version": "4.2", "pinned": false},
    {"name": "yt-dlp", "installed_versions": ["2026.10.26"], "current_version": "2026.11.23", "pinned": false}
  ],
  "casks": [
    {"name": "1password", "installed_versions": ["8.12.44"], "current_version": "8.12.46", "pinned": false},
    {"name": "slack", "installed_versions": ["4.46.0"], "current_version": "4.47.1", "pinned": false}
  ]
}"#;

fn would_upgrade(lines: &[&str]) -> String {
    format!(
        "==> Would upgrade {} outdated packages\n{}\n",
        lines.len(),
        lines.join("\n")
    )
}

fn machine() -> FakeSystem {
    FakeSystem::new()
        .on(INSTALLED_CMD, INSTALLED_JSON)
        .on(UPDATE_CMD, "Already up-to-date.")
        .on(OUTDATED_CMD, OUTDATED_JSON)
        .on(LSOF_CMD, "")
        .on(SERVICES_CMD, "[]")
        .on(
            &dry_run("--formula", "ffmpeg"),
            &would_upgrade(&["ffmpeg 7.1_3 -> 7.2", "x265 4.1 -> 4.2"]),
        )
        .on(
            &dry_run("--formula", "x265"),
            &would_upgrade(&["x265 4.1 -> 4.2"]),
        )
        .on(
            &dry_run("--formula", "uv"),
            &would_upgrade(&["uv 0.13.9 -> 0.13.10"]),
        )
        .on(
            &dry_run("--formula", "yt-dlp"),
            &would_upgrade(&["yt-dlp 2026.10.26 -> 2026.11.23"]),
        )
        .on(
            &dry_run("--cask", "slack"),
            &would_upgrade(&["slack 4.46.0 -> 4.47.1"]),
        )
        .on(
            &dry_run("--cask", "1password"),
            &would_upgrade(&["1password 8.12.44 -> 8.12.46"]),
        )
        .on(&upgrade("--formula", "ffmpeg"), "==> Upgrading ffmpeg\n")
        .on(&upgrade("--formula", "x265"), "==> Upgrading x265\n")
        .on(&upgrade("--formula", "uv"), "==> Upgrading uv\n")
        .on(&upgrade("--formula", "yt-dlp"), "==> Upgrading yt-dlp\n")
        .on(&upgrade("--cask", "slack"), "==> Upgrading slack\n")
        .on(&upgrade("--cask", "1password"), "==> Upgrading 1password\n")
}

fn ready(system: FakeSystem) -> Harness {
    let mut h = Harness::start(system);
    h.tick_at(Instant::now());
    h
}

/// The upgrade commands sent, in order.
fn upgrades(h: &Harness) -> Vec<String> {
    h.system
        .commands()
        .into_iter()
        .filter(|c| c.starts_with("brew upgrade") && !c.contains("--dry-run"))
        .collect()
}

fn mark(h: &mut Harness, name: &str) {
    h.select(name);
    h.press_char(' ');
}

#[test]
fn several_marked_packages_update_together_in_one_confirmed_plan() {
    let mut h = ready(machine());
    mark(&mut h, "uv");
    mark(&mut h, "yt-dlp");
    h.press_char('u');
    h.settle();

    let screen = h.screen();
    assert!(screen.contains("Confirm"), "{screen}");
    assert!(screen.contains("0.13.9 → 0.13.10"), "{screen}");
    assert!(screen.contains("2026.10.26 → 2026.11.23"), "{screen}");
    assert!(upgrades(&h).is_empty(), "nothing runs before confirming");

    h.press_char('y');
    h.settle();
    assert_eq!(
        upgrades(&h),
        vec![upgrade("--formula", "uv"), upgrade("--formula", "yt-dlp")]
    );
}

#[test]
fn marks_show_in_the_list_and_space_toggles_them() {
    let mut h = ready(machine());
    mark(&mut h, "uv");
    assert!(
        h.screen().lines().any(|l| l.contains("✓ uv")),
        "{}",
        h.screen()
    );

    h.press_char(' ');
    assert!(!h.screen().contains("✓ uv"), "{}", h.screen());
}

#[test]
fn update_all_plans_every_outdated_package_except_pinned_and_self_updating_ones() {
    let mut h = ready(machine());
    h.press_char('U');
    h.settle();

    let screen = h.screen();
    for line in [
        "7.1_3 → 7.2",
        "4.1 → 4.2",
        "0.13.9 → 0.13.10",
        "2026.10.26 → 2026.11.23",
        "4.46.0 → 4.47.1",
    ] {
        assert!(screen.contains(line), "missing {line}:\n{screen}");
    }
    assert!(
        !screen.contains("2.80.0 → 2.83.1"),
        "Pinned gh must not be planned:\n{screen}"
    );
    assert!(
        !screen.contains("8.12.44 → 8.12.46"),
        "Self-Updating 1password must not be planned:\n{screen}"
    );

    h.press_char('y');
    h.settle();
    let sent = upgrades(&h);
    assert!(
        !sent
            .iter()
            .any(|c| c.contains("1password") || c.contains(" gh")),
        "{sent:?}"
    );
    // x265 comes along with ffmpeg, so it isn't upgraded a second time.
    assert_eq!(
        sent,
        vec![
            upgrade("--formula", "ffmpeg"),
            upgrade("--cask", "slack"),
            upgrade("--formula", "uv"),
            upgrade("--formula", "yt-dlp"),
        ]
    );
}

#[test]
fn a_self_updating_cask_can_be_explicitly_included() {
    let mut h = ready(machine());
    mark(&mut h, "1password");
    h.press_char('U');
    h.settle();
    assert!(h.screen().contains("8.12.44 → 8.12.46"), "{}", h.screen());

    h.press_char('y');
    h.settle();
    assert!(
        upgrades(&h).contains(&upgrade("--cask", "1password")),
        "{:?}",
        upgrades(&h)
    );
}

#[test]
fn a_package_that_becomes_in_use_mid_run_is_held_at_its_step() {
    let system = machine();
    let slack_upgrade = system.hold(&upgrade("--cask", "slack"));
    let mut h = ready(system.clone());
    mark(&mut h, "slack");
    mark(&mut h, "uv");
    h.press_char('u');
    h.settle();
    h.press_char('y');

    // While slack is upgrading, something starts using uv.
    for _ in 0..200 {
        h.tick_no_wait(Instant::now());
        if h.system.count(&upgrade("--cask", "slack")) == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    system.set(
        LSOF_CMD,
        "p404\ncuv\nftxt\nn/opt/homebrew/Cellar/uv/0.13.9/bin/uv\n",
    );
    slack_upgrade.release();
    h.settle();

    assert_eq!(
        upgrades(&h),
        vec![upgrade("--cask", "slack")],
        "uv must not be upgraded"
    );
    let screen = h.screen();
    assert!(screen.contains("Held"), "{screen}");
    assert!(screen.contains("uv (pid 404)"), "{screen}");
}

#[test]
fn a_failed_step_does_not_stop_the_rest_and_the_summary_reports_everything() {
    let system = machine()
        .failing(
            &upgrade("--cask", "slack"),
            "Error: Download failed: Slack.dmg",
        )
        .on(
            LSOF_CMD,
            "p990\ncyt-dlp\nftxt\nn/opt/homebrew/Cellar/yt-dlp/2026.10.26/bin/yt-dlp\n",
        );
    let mut h = ready(system);
    mark(&mut h, "slack");
    mark(&mut h, "uv");
    mark(&mut h, "yt-dlp");
    h.press_char('u');
    h.settle();
    h.press_char('y');
    h.settle();

    // yt-dlp was In Use at planning, so it was never going to run; slack
    // failed; uv still ran.
    assert_eq!(
        upgrades(&h),
        vec![upgrade("--cask", "slack"), upgrade("--formula", "uv")]
    );
    let screen = h.screen();
    assert!(screen.contains("Updated: uv"), "{screen}");
    assert!(screen.contains("Failed: slack"), "{screen}");
    assert!(screen.contains("Download failed: Slack.dmg"), "{screen}");
    assert!(screen.contains("Held: yt-dlp"), "{screen}");
    assert!(screen.contains("yt-dlp (pid 990)"), "{screen}");
}

#[test]
fn marks_are_cleared_once_the_update_has_run() {
    let mut h = ready(machine());
    mark(&mut h, "uv");
    mark(&mut h, "yt-dlp");
    h.press_char('u');
    h.settle();
    h.press_char('y');
    h.settle();

    assert!(!h.screen().contains('✓'), "{}", h.screen());
}

#[test]
fn update_all_with_nothing_outdated_says_so() {
    let system = machine().on(OUTDATED_CMD, r#"{"formulae": [], "casks": []}"#);
    let mut h = ready(system);
    h.press_char('U');
    h.settle();

    assert!(h.screen().contains("Nothing to update"), "{}", h.screen());
    assert!(upgrades(&h).is_empty());
}

#[test]
fn the_footer_and_help_mention_marking_and_update_all() {
    let mut h = ready(machine());
    let footer = h.screen().lines().last().unwrap().to_string();
    assert!(footer.contains("space") && footer.contains("U"), "{footer}");
    h.press(KeyCode::Char('?'));
    assert!(h.screen().contains("Update all"), "{}", h.screen());
}

#[test]
fn update_all_before_the_refresh_finishes_says_it_is_still_checking() {
    let system = machine();
    let refresh = system.hold(UPDATE_CMD);
    let mut h = Harness::start(system);
    h.tick_no_wait(Instant::now());
    h.press_char('U');

    assert!(
        h.screen().contains("Still checking for updates"),
        "{}",
        h.screen()
    );
    assert!(!h.screen().contains("Nothing to update"), "{}", h.screen());
    refresh.release();
    h.settle();
}

#[test]
fn a_target_that_another_target_upgrades_as_a_dependency_is_not_upgraded_separately() {
    // uv sorts before yt-dlp, but yt-dlp's upgrade brings uv along.
    let system = machine().on(
        &dry_run("--formula", "yt-dlp"),
        &would_upgrade(&["yt-dlp 2026.10.26 -> 2026.11.23", "uv 0.13.9 -> 0.13.10"]),
    );
    let mut h = ready(system);
    mark(&mut h, "uv");
    mark(&mut h, "yt-dlp");
    h.press_char('u');
    h.settle();
    h.press_char('y');
    h.settle();

    assert_eq!(upgrades(&h), vec![upgrade("--formula", "yt-dlp")]);
}

mod support;

use std::time::{Duration, Instant};

use support::{FakeSystem, Harness, INSTALLED_CMD};

const LSOF_CMD: &str = "lsof -nP -F pcn -d txt";
const SERVICES_CMD: &str = "brew services list --json";

/// Installed: a Cask app, a Cask binary, three Formulae, a service Formula and
/// brewmax itself.
const INSTALLED_JSON: &str = r#"{
  "formulae": [
    {"name": "brewmax", "desc": "Homebrew TUI", "installed": [{"version": "0.1.0"}]},
    {"name": "ffmpeg", "desc": "Play, record, convert, and stream audio and video", "installed": [{"version": "7.1_3"}]},
    {"name": "gh", "desc": "GitHub command-line tool", "installed": [{"version": "2.80.0"}]},
    {"name": "jq", "desc": "Lightweight and flexible command-line JSON processor", "installed": [{"version": "1.8.1"}]},
    {"name": "postgresql@17", "desc": "Object-relational database system", "installed": [{"version": "17.6"}]}
  ],
  "casks": [
    {"token": "1password", "desc": "Password manager", "installed": "8.12.44",
     "artifacts": [{"app": ["1Password.app"], "target": "/Applications/1Password.app"}, {"zap": []}]},
    {"token": "claude-code", "desc": "Terminal-based AI coding assistant", "installed": "2.1.325",
     "artifacts": [{"binary": ["claude"], "target": "/opt/homebrew/bin/claude"}]}
  ]
}"#;

/// `lsof -F pcn` output: 1Password's app executable, `gh` running from its
/// keg, an `obs` process with an ffmpeg library loaded, and claude-code's
/// binary running from the Caskroom.
const LSOF_BUSY: &str = "\
p812
c1Password
ftxt
n/Applications/1Password.app/Contents/MacOS/1Password
ftxt
n/usr/lib/dyld
p900
cgh
ftxt
n/opt/homebrew/Cellar/gh/2.80.0/bin/gh
p950
cobs
ftxt
n/Applications/OBS.app/Contents/MacOS/obs
ftxt
n/opt/homebrew/Cellar/ffmpeg/7.1_3/lib/libavcodec.62.dylib
p990
cclaude
ftxt
n/opt/homebrew/Caskroom/claude-code/2.1.325/claude
";

const LSOF_IDLE: &str = "\
p424
cloginwindow
ftxt
n/System/Library/CoreServices/loginwindow.app/Contents/MacOS/loginwindow
";

const SERVICES_RUNNING: &str =
    r#"[{"name": "postgresql@17", "status": "started", "user": "morgan"}]"#;

fn busy_machine() -> Harness {
    let mut h = Harness::start(
        FakeSystem::new()
            .on(INSTALLED_CMD, INSTALLED_JSON)
            .on(LSOF_CMD, LSOF_BUSY)
            .on(SERVICES_CMD, SERVICES_RUNNING),
    );
    h.scan_in_use();
    h
}

fn row<'a>(screen: &'a str, name: &str) -> &'a str {
    screen
        .lines()
        .find(|line| {
            line.split('│')
                .nth(1)
                .is_some_and(|c| c.trim_start().starts_with(&format!("{name} ")))
        })
        .unwrap_or_else(|| panic!("no row for {name}:\n{screen}"))
}

fn in_use_row(h: &Harness, name: &str) -> bool {
    row(&h.screen(), name)
        .split("││")
        .next()
        .unwrap()
        .contains("In Use")
}

#[test]
fn nothing_is_marked_in_use_before_the_first_scan() {
    let h = Harness::start(
        FakeSystem::new()
            .on(INSTALLED_CMD, INSTALLED_JSON)
            .on(LSOF_CMD, LSOF_BUSY)
            .on(SERVICES_CMD, SERVICES_RUNNING),
    );
    assert!(!in_use_row(&h, "1password"), "{}", h.screen());
    assert_eq!(h.system.count(LSOF_CMD), 0);
}

#[test]
fn a_running_cask_app_is_in_use_and_names_its_process() {
    let mut h = busy_machine();
    assert!(in_use_row(&h, "1password"), "{}", h.screen());

    h.select("1password");
    assert!(h.screen().contains("1Password (pid 812)"), "{}", h.screen());
}

#[test]
fn a_cask_binary_running_from_the_caskroom_is_in_use() {
    let mut h = busy_machine();
    assert!(in_use_row(&h, "claude-code"), "{}", h.screen());

    h.select("claude-code");
    assert!(h.screen().contains("claude (pid 990)"), "{}", h.screen());
}

#[test]
fn a_formula_whose_executable_is_running_is_in_use() {
    let mut h = busy_machine();
    assert!(in_use_row(&h, "gh"), "{}", h.screen());

    h.select("gh");
    assert!(h.screen().contains("gh (pid 900)"), "{}", h.screen());
}

#[test]
fn a_formula_whose_library_another_process_loaded_is_in_use_naming_that_process() {
    let mut h = busy_machine();
    assert!(in_use_row(&h, "ffmpeg"), "{}", h.screen());

    h.select("ffmpeg");
    assert!(h.screen().contains("obs (pid 950)"), "{}", h.screen());
}

#[test]
fn a_formula_running_as_a_brew_service_is_in_use() {
    let mut h = busy_machine();
    assert!(in_use_row(&h, "postgresql@17"), "{}", h.screen());

    h.select("postgresql@17");
    assert!(h.screen().contains("brew service"), "{}", h.screen());
}

#[test]
fn brewmax_is_always_in_use() {
    let mut h = Harness::start(
        FakeSystem::new()
            .on(INSTALLED_CMD, INSTALLED_JSON)
            .on(LSOF_CMD, LSOF_IDLE)
            .on(SERVICES_CMD, "[]"),
    );
    h.scan_in_use();
    assert!(in_use_row(&h, "brewmax"), "{}", h.screen());
}

#[test]
fn packages_nothing_is_using_are_not_marked() {
    let h = busy_machine();
    assert!(!in_use_row(&h, "jq"), "{}", h.screen());
}

#[test]
fn markers_follow_apps_opening_and_closing_on_each_refresh() {
    let t0 = Instant::now();
    let mut h = Harness::start(
        FakeSystem::new()
            .on(INSTALLED_CMD, INSTALLED_JSON)
            .on(LSOF_CMD, LSOF_BUSY)
            .on(SERVICES_CMD, SERVICES_RUNNING),
    );
    h.tick_at(t0);
    assert!(in_use_row(&h, "1password"), "{}", h.screen());

    // 1Password quits; the next refresh clears its marker.
    h.system.set(LSOF_CMD, LSOF_IDLE);
    h.tick_at(t0 + Duration::from_secs(3));
    assert!(!in_use_row(&h, "1password"), "{}", h.screen());
}

#[test]
fn scans_repeat_every_few_seconds_not_on_every_tick() {
    let t0 = Instant::now();
    let mut h = Harness::start(
        FakeSystem::new()
            .on(INSTALLED_CMD, INSTALLED_JSON)
            .on(LSOF_CMD, LSOF_IDLE)
            .on(SERVICES_CMD, "[]"),
    );
    h.tick_at(t0);
    h.tick_at(t0 + Duration::from_millis(500));
    h.tick_at(t0 + Duration::from_secs(1));
    assert_eq!(h.system.count(LSOF_CMD), 1);

    h.tick_at(t0 + Duration::from_secs(3));
    assert_eq!(h.system.count(LSOF_CMD), 2);
}

#[test]
fn a_failed_scan_keeps_the_last_markers_and_says_the_check_failed() {
    let t0 = Instant::now();
    let mut h = Harness::start(
        FakeSystem::new()
            .on(INSTALLED_CMD, INSTALLED_JSON)
            .on(LSOF_CMD, LSOF_BUSY)
            .on(SERVICES_CMD, SERVICES_RUNNING),
    );
    h.tick_at(t0);
    assert!(in_use_row(&h, "1password"), "{}", h.screen());

    h.system.remove(LSOF_CMD);
    h.tick_at(t0 + Duration::from_secs(3));
    assert!(in_use_row(&h, "1password"), "{}", h.screen());
    assert!(h.screen().contains("In Use check failed"), "{}", h.screen());
}

#[test]
fn several_processes_with_the_same_name_are_summarised_on_one_line() {
    let mut h = Harness::start(
        FakeSystem::new()
            .on(INSTALLED_CMD, INSTALLED_JSON)
            .on(
                LSOF_CMD,
                "p812\nc1Password\nftxt\nn/Applications/1Password.app/Contents/MacOS/1Password\n\
                 p813\nc1Password Helper\nftxt\nn/Applications/1Password.app/Contents/Frameworks/Helper.app/Contents/MacOS/1Password Helper\n\
                 p814\nc1Password Helper\nftxt\nn/Applications/1Password.app/Contents/Frameworks/Helper.app/Contents/MacOS/1Password Helper\n\
                 p815\nc1Password Helper\nftxt\nn/Applications/1Password.app/Contents/Frameworks/Helper.app/Contents/MacOS/1Password Helper\n",
            )
            .on(SERVICES_CMD, "[]"),
    );
    h.scan_in_use();
    h.select("1password");

    let screen = h.screen();
    assert!(screen.contains("1Password (pid 812)"), "{screen}");
    assert!(
        screen.contains("1Password Helper (3 processes)"),
        "{screen}"
    );
    assert!(!screen.contains("pid 813"), "{screen}");
}

#[test]
fn a_failing_brew_services_keeps_service_markers_and_says_the_check_failed() {
    let t0 = Instant::now();
    let mut h = Harness::start(
        FakeSystem::new()
            .on(INSTALLED_CMD, INSTALLED_JSON)
            .on(LSOF_CMD, LSOF_BUSY)
            .on(SERVICES_CMD, SERVICES_RUNNING),
    );
    h.tick_at(t0);
    assert!(in_use_row(&h, "postgresql@17"), "{}", h.screen());

    h.system
        .set_failing(SERVICES_CMD, "Error: launchctl failed");
    h.tick_at(t0 + Duration::from_secs(3));
    assert!(in_use_row(&h, "postgresql@17"), "{}", h.screen());
    assert!(h.screen().contains("In Use check failed"), "{}", h.screen());

    h.system.set(SERVICES_CMD, "not json");
    h.tick_at(t0 + Duration::from_secs(6));
    assert!(in_use_row(&h, "postgresql@17"), "{}", h.screen());
    assert!(h.screen().contains("In Use check failed"), "{}", h.screen());
}

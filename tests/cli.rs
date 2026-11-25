//! The brewmax binary's command line, as the Homebrew formula's test and a
//! user's shell see it. Only flags are covered: with no arguments it starts
//! the TUI, which needs a terminal.

use std::process::Command;

fn brewmax(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_brewmax"))
        .args(args)
        .output()
        .expect("brewmax runs")
}

#[test]
fn version_prints_the_name_and_cargo_version() {
    let output = brewmax(&["--version"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("brewmax {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn short_version_flag_prints_the_name_too() {
    let output = brewmax(&["-V"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("brewmax "));
}

#[test]
fn help_describes_usage() {
    let output = brewmax(&["--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage: brewmax"), "{stdout}");
    assert!(stdout.contains("--version"), "{stdout}");
}

#[test]
fn an_unknown_argument_is_an_error_and_does_not_start_the_tui() {
    let output = brewmax(&["--frobnicate"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--frobnicate"), "{stderr}");
    assert!(stderr.contains("Usage: brewmax"), "{stderr}");
}

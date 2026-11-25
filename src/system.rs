//! The System boundary: every interaction brewmax has with the outside world
//! (running `brew`, inspecting processes, and later controlling apps) goes
//! through the [`System`] trait. Tests substitute a scripted fake.

use std::fmt;
use std::io::{BufRead, BufReader, Read};
use std::sync::mpsc;

/// Environment for every `brew` command brewmax runs:
/// - automatic cleanup off, so old versions a running process may still use
///   are only removed by brewmax's Cleanup (ADR 0001);
/// - automatic `brew update` off, so an upgrade does exactly what its dry-run
///   Plan said (brewmax refreshes on launch instead);
/// - automatic removal of unneeded dependencies off, so an uninstall removes
///   exactly the Package confirmed (unneeded dependencies are Cleanup's job).
pub const BREW_ENV: [(&str, &str); 3] = [
    ("HOMEBREW_NO_INSTALL_CLEANUP", "1"),
    ("HOMEBREW_NO_AUTO_UPDATE", "1"),
    ("HOMEBREW_NO_AUTOREMOVE", "1"),
];

/// A command to run: a program, its arguments and extra environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl Command {
    pub fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env: Vec::new(),
        }
    }

    /// A `brew` command, always carrying [`BREW_ENV`].
    pub fn brew(args: &[&str]) -> Self {
        let mut command = Self::new("brew", args);
        command.env = BREW_ENV
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        command
    }
}

/// The command line, without its environment.
impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.program)?;
        for arg in &self.args {
            write!(f, " {arg}")?;
        }
        Ok(())
    }
}

/// The captured result of running a [`Command`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub trait System {
    /// Run a command to completion and capture its output.
    fn run(&self, command: &Command) -> anyhow::Result<Output>;

    /// Run a command to completion, passing each line it prints (stdout and
    /// stderr) to `on_line` as it appears. Returns whether it succeeded.
    fn stream(&self, command: &Command, on_line: &mut dyn FnMut(&str)) -> anyhow::Result<bool>;
}

/// The real macOS implementation.
#[derive(Debug, Clone, Default)]
pub struct MacSystem;

impl MacSystem {
    fn process(command: &Command) -> std::process::Command {
        let mut process = std::process::Command::new(&command.program);
        process
            .args(&command.args)
            .envs(command.env.iter().map(|(k, v)| (k, v)))
            .stdin(std::process::Stdio::null());
        process
    }
}

impl System for MacSystem {
    fn run(&self, command: &Command) -> anyhow::Result<Output> {
        let output = Self::process(command)
            .output()
            .map_err(|e| anyhow::anyhow!("failed to run `{command}`: {e}"))?;
        Ok(Output {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn stream(&self, command: &Command, on_line: &mut dyn FnMut(&str)) -> anyhow::Result<bool> {
        let mut child = Self::process(command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to run `{command}`: {e}"))?;

        // Read stdout and stderr concurrently, interleaving lines as they come.
        let (tx, rx) = mpsc::channel();
        let readers = [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        .map(|pipe| {
            let tx = tx.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    let _ = tx.send(line);
                }
            })
        })
        .collect::<Vec<_>>();
        drop(tx);
        for line in rx {
            on_line(&line);
        }
        for reader in readers {
            let _ = reader.join();
        }
        Ok(child.wait()?.success())
    }
}

//! The System boundary: every interaction brewmax has with the outside world
//! (running `brew`, and later inspecting processes and controlling apps) goes
//! through the [`System`] trait. Tests substitute a scripted fake.

use std::fmt;

/// A command to run: a program and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub program: String,
    pub args: Vec<String>,
}

impl Command {
    pub fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }

    pub fn brew(args: &[&str]) -> Self {
        Self::new("brew", args)
    }
}

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
}

/// The real macOS implementation.
#[derive(Debug, Clone, Default)]
pub struct MacSystem;

impl System for MacSystem {
    fn run(&self, command: &Command) -> anyhow::Result<Output> {
        let output = std::process::Command::new(&command.program)
            .args(&command.args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| anyhow::anyhow!("failed to run `{command}`: {e}"))?;
        Ok(Output {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

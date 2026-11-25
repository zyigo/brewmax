//! Test harness: a scripted fake `System` and helpers to drive the whole app
//! through key presses and inspect the rendered screen.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use brewmax::app::App;
use brewmax::system::{Command, Output, System};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

/// A `System` whose command outputs are scripted per test and which records
/// every command brewmax sends it.
#[derive(Clone, Default)]
pub struct FakeSystem {
    responses: Rc<RefCell<HashMap<String, Output>>>,
    commands: Rc<RefCell<Vec<String>>>,
}

impl FakeSystem {
    pub fn new() -> Self {
        Self::default()
    }

    /// Script the stdout returned for an exact command line.
    pub fn on(self, command_line: &str, stdout: &str) -> Self {
        self.responses.borrow_mut().insert(
            command_line.to_string(),
            Output {
                success: true,
                stdout: stdout.to_string(),
                stderr: String::new(),
            },
        );
        self
    }

    /// Script a failing exit with the given stderr for an exact command line.
    pub fn failing(self, command_line: &str, stderr: &str) -> Self {
        self.responses.borrow_mut().insert(
            command_line.to_string(),
            Output {
                success: false,
                stdout: String::new(),
                stderr: stderr.to_string(),
            },
        );
        self
    }

    /// Every command line brewmax has sent, in order.
    pub fn commands(&self) -> Vec<String> {
        self.commands.borrow().clone()
    }
}

impl System for FakeSystem {
    fn run(&self, command: &Command) -> anyhow::Result<Output> {
        let line = command.to_string();
        self.commands.borrow_mut().push(line.clone());
        self.responses
            .borrow()
            .get(&line)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("fake system has no response scripted for `{line}`"))
    }
}

/// The whole app running on a fake System, rendered into an in-memory terminal.
pub struct Harness {
    pub app: App<FakeSystem>,
    pub system: FakeSystem,
    terminal: Terminal<TestBackend>,
}

impl Harness {
    pub fn start(system: FakeSystem) -> Self {
        let app = App::new(system.clone());
        let terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        let mut harness = Self {
            app,
            system,
            terminal,
        };
        harness.render();
        harness
    }

    pub fn press(&mut self, code: KeyCode) {
        self.app.handle_key(KeyEvent::from(code));
        self.render();
    }

    fn render(&mut self) {
        self.terminal.draw(|frame| self.app.render(frame)).unwrap();
    }

    /// The rendered screen as plain text, one line per row.
    pub fn screen(&self) -> String {
        let buffer = self.terminal.backend().buffer();
        let width = buffer.area.width as usize;
        buffer
            .content
            .chunks(width)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub const INSTALLED_CMD: &str = "brew info --json=v2 --installed";

/// Two Formulae and one Cask, in the shape `brew info --json=v2 --installed` emits.
pub const INSTALLED_JSON: &str = r#"{
  "formulae": [
    {"name": "ffmpeg", "full_name": "ffmpeg", "desc": "Play, record, convert, and stream audio and video",
     "installed": [{"version": "7.1_3"}], "versions": {"stable": "7.1"}},
    {"name": "gh", "full_name": "gh", "desc": "GitHub command-line tool",
     "installed": [{"version": "2.80.0"}], "versions": {"stable": "2.80.0"}}
  ],
  "casks": [
    {"token": "1password", "full_token": "1password", "name": ["1Password"],
     "desc": "Password manager that keeps all passwords secure behind one password",
     "installed": "8.12.44", "version": "8.12.46"}
  ]
}"#;

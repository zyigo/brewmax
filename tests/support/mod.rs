//! Test harness: a scripted fake `System` and helpers to drive the whole app
//! through key presses and inspect the rendered screen.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use brewmax::app::App;
use brewmax::system::{Command, Output, System};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

/// A `System` whose command outputs are scripted per test and which records
/// every command brewmax sends it.
#[derive(Clone, Default)]
pub struct FakeSystem {
    responses: Arc<Mutex<HashMap<String, Output>>>,
    commands: Arc<Mutex<Vec<String>>>,
    holds: Arc<Mutex<HashMap<String, Receiver<()>>>>,
}

/// Lets a held command finish. Dropping it releases the command too.
pub struct Release(Sender<()>);

impl Release {
    pub fn release(self) {
        let _ = self.0.send(());
    }
}

impl FakeSystem {
    pub fn new() -> Self {
        Self::default()
    }

    /// Script the stdout returned for an exact command line.
    pub fn on(self, command_line: &str, stdout: &str) -> Self {
        self.responses.lock().unwrap().insert(
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
        self.responses.lock().unwrap().insert(
            command_line.to_string(),
            Output {
                success: false,
                stdout: String::new(),
                stderr: stderr.to_string(),
            },
        );
        self
    }

    /// Change the stdout scripted for a command line mid-test (e.g. an app quits).
    pub fn set(&self, command_line: &str, stdout: &str) {
        self.responses.lock().unwrap().insert(
            command_line.to_string(),
            Output {
                success: true,
                stdout: stdout.to_string(),
                stderr: String::new(),
            },
        );
    }

    /// Make a command line fail mid-test.
    pub fn set_failing(&self, command_line: &str, stderr: &str) {
        self.responses.lock().unwrap().insert(
            command_line.to_string(),
            Output {
                success: false,
                stdout: String::new(),
                stderr: stderr.to_string(),
            },
        );
    }

    /// Stop scripting a command line, so running it errors as if the program
    /// were missing.
    pub fn remove(&self, command_line: &str) {
        self.responses.lock().unwrap().remove(command_line);
    }

    /// Make the next run of a command line wait, mid-flight, until the
    /// returned [`Release`] is released, e.g. to observe a slow `brew update`.
    pub fn hold(&self, command_line: &str) -> Release {
        let (tx, rx) = mpsc::channel();
        self.holds
            .lock()
            .unwrap()
            .insert(command_line.to_string(), rx);
        Release(tx)
    }

    /// How many times an exact command line has been sent.
    pub fn count(&self, command_line: &str) -> usize {
        self.commands()
            .iter()
            .filter(|c| *c == command_line)
            .count()
    }

    /// Every command line brewmax has sent, in order.
    pub fn commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().clone()
    }
}

impl System for FakeSystem {
    fn run(&self, command: &Command) -> anyhow::Result<Output> {
        let line = command.to_string();
        self.commands.lock().unwrap().push(line.clone());
        let hold = self.holds.lock().unwrap().remove(&line);
        if let Some(hold) = hold {
            let _ = hold.recv();
        }
        self.responses
            .lock()
            .unwrap()
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
        let terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
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

    /// Advance the app's clock to `now`, wait for any background work that
    /// tick started (In Use scan, refresh) to finish, and re-render.
    pub fn tick_at(&mut self, now: Instant) {
        self.app.tick(now);
        self.app.wait_for_background();
        self.render();
    }

    /// Advance the app's clock without waiting for background work.
    pub fn tick_no_wait(&mut self, now: Instant) {
        self.app.tick(now);
        self.render();
    }

    /// Wait for all background work in flight, apply it and re-render.
    pub fn settle(&mut self) {
        self.app.wait_for_background();
        self.render();
    }

    pub fn press_char(&mut self, c: char) {
        self.press(KeyCode::Char(c));
    }

    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.press_char(c);
        }
    }

    /// Run one In Use scan to completion.
    pub fn scan_in_use(&mut self) {
        self.tick_at(Instant::now());
    }

    /// Move the selection to the row containing `name`.
    pub fn select(&mut self, name: &str) {
        for _ in 0..50 {
            if self.detail_title_is(name) {
                return;
            }
            self.press(KeyCode::Down);
        }
        panic!("could not select {name}:\n{}", self.screen());
    }

    fn detail_title_is(&self, name: &str) -> bool {
        // The detail pane's first content line is the selected Package's name.
        let screen = self.screen();
        let mut lines = screen.lines().skip_while(|l| !l.contains(" Details "));
        lines
            .nth(1)
            .and_then(|l| l.rsplit('│').nth(1))
            .is_some_and(|cell| cell.trim() == name)
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

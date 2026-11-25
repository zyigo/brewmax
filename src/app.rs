//! The application: state, key handling and rendering. It talks to the outside
//! world only through the [`System`] it is given.

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState, Wrap};

use crate::in_use::{self, InUse, Reason, Target};
use crate::inventory::{self, Package};
use crate::system::System;

/// How often the In Use scan repeats.
const SCAN_INTERVAL: Duration = Duration::from_secs(3);

pub struct App<S: System + Clone + Send + 'static> {
    system: S,
    packages: Vec<Package>,
    selected: usize,
    load_error: Option<String>,
    quit: bool,
    in_use: InUse,
    /// Why the most recent In Use scan failed, if it did.
    scan_error: Option<String>,
    /// When the last In Use scan started.
    last_scan: Option<Instant>,
    /// The In Use scan running in the background, if any.
    scan_in_flight: Option<Receiver<anyhow::Result<InUse>>>,
}

impl<S: System + Clone + Send + 'static> App<S> {
    pub fn new(system: S) -> Self {
        let (packages, load_error) = match inventory::load_installed(&system) {
            Ok(packages) => (packages, None),
            Err(e) => (Vec::new(), Some(format!("{e:#}"))),
        };
        Self {
            system,
            packages,
            selected: 0,
            load_error,
            quit: false,
            in_use: InUse::new(),
            scan_error: None,
            last_scan: None,
            scan_in_flight: None,
        }
    }

    /// Advance the app's clock: collect a finished In Use scan, and start a
    /// new one in the background when the last started `SCAN_INTERVAL` ago.
    pub fn tick(&mut self, now: Instant) {
        self.collect_scan(false);
        let due = self
            .last_scan
            .is_none_or(|last| now.duration_since(last) >= SCAN_INTERVAL);
        if due && self.scan_in_flight.is_none() {
            self.start_scan(now);
        }
    }

    /// Block until the In Use scan in flight (if any) finishes and apply it.
    pub fn wait_for_scan(&mut self) {
        self.collect_scan(true);
    }

    fn start_scan(&mut self, now: Instant) {
        let system = self.system.clone();
        let targets: Vec<Target> = self.packages.iter().map(Target::from).collect();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(in_use::scan(&system, &targets));
        });
        self.last_scan = Some(now);
        self.scan_in_flight = Some(rx);
    }

    fn collect_scan(&mut self, block: bool) {
        let Some(rx) = &self.scan_in_flight else {
            return;
        };
        let result = if block {
            rx.recv().map_err(|_| ())
        } else {
            match rx.try_recv() {
                Err(mpsc::TryRecvError::Empty) => return,
                other => other.map_err(|_| ()),
            }
        };
        self.scan_in_flight = None;
        match result {
            Ok(Ok(in_use)) => {
                self.in_use = in_use;
                self.scan_error = None;
            }
            // Keep the last known state; a failed scan never clears a marker.
            Ok(Err(e)) => self.scan_error = Some(format!("{e:#}")),
            Err(()) => self.scan_error = Some("In Use scan stopped unexpectedly".to_string()),
        }
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected + 1 < self.packages.len() {
                    self.selected += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Char('q') => self.quit = true,
            _ => {}
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let [list_area, detail_area] =
            Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                .areas(frame.area());

        let rows = self.packages.iter().map(|p| {
            let marker = if self.in_use.contains_key(&p.name) {
                "● In Use"
            } else {
                ""
            };
            Row::new(vec![
                p.name.clone(),
                marker.to_string(),
                p.kind.label().to_string(),
                p.version.clone(),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Fill(1),
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Length(16),
            ],
        )
        .block(Block::bordered().title(" Installed "))
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        if let Some(error) = &self.load_error {
            let message = Paragraph::new(format!("Could not read installed packages:\n{error}"))
                .block(Block::bordered().title(" Installed "))
                .wrap(Wrap { trim: true });
            frame.render_widget(message, list_area);
        } else {
            let mut state = TableState::default().with_selected(Some(self.selected));
            frame.render_stateful_widget(table, list_area, &mut state);
        }

        frame.render_widget(self.detail(), detail_area);
    }

    fn detail(&self) -> Paragraph<'_> {
        let block = Block::bordered().title(" Details ");
        let Some(package) = self.packages.get(self.selected) else {
            return Paragraph::new("No package selected").block(block);
        };
        let field = |label: &'static str, value: String| {
            Line::from(vec![
                Span::styled(label, Style::new().add_modifier(Modifier::BOLD)),
                Span::raw(value),
            ])
        };
        let mut lines = vec![
            Line::styled(
                package.name.clone(),
                Style::new().add_modifier(Modifier::BOLD),
            ),
            field("Kind:    ", package.kind.label().to_string()),
            field("Version: ", package.version.clone()),
        ];
        if let Some(reasons) = self.in_use.get(&package.name) {
            lines.push(Line::default());
            lines.push(Line::styled(
                "In Use by:",
                Style::new().add_modifier(Modifier::BOLD),
            ));
            lines.extend(
                summarise(reasons)
                    .into_iter()
                    .map(|r| Line::raw(format!("  {r}"))),
            );
        }
        if let Some(error) = &self.scan_error {
            lines.push(Line::default());
            lines.push(Line::raw(format!("In Use check failed: {error}")));
        }
        lines.push(Line::default());
        if let Some(desc) = &package.desc {
            lines.push(Line::raw(desc.clone()));
        }
        Paragraph::new(lines).block(block).wrap(Wrap { trim: true })
    }
}

/// One line per distinct reason; several processes sharing a name (an app's
/// helpers, say) collapse into one "name (N processes)" line.
fn summarise(reasons: &[Reason]) -> Vec<String> {
    let mut lines: Vec<(String, Vec<u32>)> = Vec::new();
    for reason in reasons {
        match reason {
            Reason::Process { name, pid } => match lines
                .iter_mut()
                .find(|(n, pids)| n == name && !pids.is_empty())
            {
                Some((_, pids)) => pids.push(*pid),
                None => lines.push((name.clone(), vec![*pid])),
            },
            other => lines.push((other.to_string(), Vec::new())),
        }
    }
    lines
        .into_iter()
        .map(|(name, pids)| match pids.as_slice() {
            [] => name,
            [pid] => Reason::Process { name, pid: *pid }.to_string(),
            many => format!("{name} ({} processes)", many.len()),
        })
        .collect()
}

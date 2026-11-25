//! The application: state, key handling and rendering. It talks to the outside
//! world only through the [`System`] it is given.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState, Wrap};

use crate::inventory::{self, Package};
use crate::system::System;

pub struct App<S: System> {
    #[allow(dead_code)]
    system: S,
    packages: Vec<Package>,
    selected: usize,
    load_error: Option<String>,
    quit: bool,
}

impl<S: System> App<S> {
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
            Row::new(vec![
                p.name.clone(),
                p.kind.label().to_string(),
                p.version.clone(),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Fill(1),
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
            Line::default(),
        ];
        if let Some(desc) = &package.desc {
            lines.push(Line::raw(desc.clone()));
        }
        Paragraph::new(lines).block(block).wrap(Wrap { trim: true })
    }
}

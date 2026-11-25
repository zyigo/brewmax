//! The application: state, key handling and rendering. It talks to the outside
//! world only through the [`System`] it is given.

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Row, Table, TableState, Tabs, Wrap};

use crate::in_use::{self, InUse, Reason, Target};
use crate::inventory::{self, Package};
use crate::operation::{self, BREWMAX, Event, Held, Plan};
use crate::outdated::{self, Refresh};
use crate::system::System;

/// How often the In Use scan repeats.
const SCAN_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Installed,
    Outdated,
}

impl Tab {
    const ALL: [Tab; 2] = [Tab::Installed, Tab::Outdated];

    fn title(self) -> &'static str {
        match self {
            Tab::Installed => "Installed",
            Tab::Outdated => "Outdated",
        }
    }

    fn next(self) -> Tab {
        match self {
            Tab::Installed => Tab::Outdated,
            Tab::Outdated => Tab::Installed,
        }
    }
}

/// The once-per-launch refresh of available versions.
enum RefreshState {
    NotStarted,
    Running(Receiver<Refresh>),
    Done(Refresh),
}

/// Where the current operation has got to.
enum Stage {
    /// Working out the Plan (dry run and In Use check).
    Planning,
    /// Waiting for the user to confirm a Plan with more than one Package.
    Confirming,
    /// Running `brew`.
    Running,
    /// Left unchanged because something in the Plan is In Use.
    Held(Held),
    /// Finished, or refused before starting; `ok` says whether it worked.
    Done { ok: bool, message: String },
}

/// The one operation brewmax is running or last ran.
struct Operation {
    /// What the operation is, e.g. "Update ffmpeg".
    title: String,
    plan: Option<Plan>,
    stage: Stage,
    /// `brew` output and progress notes.
    log: Vec<String>,
    events: Option<Receiver<Event>>,
}

impl Operation {
    fn refused(title: String, message: String) -> Self {
        Operation {
            title,
            plan: None,
            stage: Stage::Done { ok: false, message },
            log: Vec::new(),
            events: None,
        }
    }

    /// Whether brew may be changing things right now (so nothing else may
    /// start, and brewmax must not quit).
    fn busy(&self) -> bool {
        matches!(self.stage, Stage::Planning | Stage::Running)
    }
}

/// One row of the visible list: a group label or a Package (by index).
enum Item {
    Label(&'static str),
    Package(usize),
}

pub struct App<S: System + Clone + Send + 'static> {
    system: S,
    packages: Vec<Package>,
    load_error: Option<String>,
    quit: bool,

    tab: Tab,
    /// Index into the selectable (Package) rows of the visible list.
    selected: usize,
    filter: String,
    /// Whether keys are currently being typed into the filter.
    filtering: bool,
    help: bool,

    refresh: RefreshState,

    in_use: InUse,
    /// Why the most recent In Use scan failed, if it did.
    scan_error: Option<String>,
    /// When the last In Use scan started.
    last_scan: Option<Instant>,
    /// The In Use scan running in the background, if any.
    scan_in_flight: Option<Receiver<anyhow::Result<InUse>>>,

    operation: Option<Operation>,
    /// A short message for the user, shown in the footer until the next key.
    notice: Option<String>,
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
            load_error,
            quit: false,
            tab: Tab::Installed,
            selected: 0,
            filter: String::new(),
            filtering: false,
            help: false,
            refresh: RefreshState::NotStarted,
            in_use: InUse::new(),
            scan_error: None,
            last_scan: None,
            scan_in_flight: None,
            operation: None,
            notice: None,
        }
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    // ---- background work -------------------------------------------------

    /// Advance the app's clock: collect finished background work, start the
    /// launch refresh if it hasn't started, and start a new In Use scan when
    /// the last one started `SCAN_INTERVAL` ago.
    pub fn tick(&mut self, now: Instant) {
        self.collect_background(false);
        if matches!(self.refresh, RefreshState::NotStarted) {
            self.start_refresh();
        }
        let due = self
            .last_scan
            .is_none_or(|last| now.duration_since(last) >= SCAN_INTERVAL);
        if due && self.scan_in_flight.is_none() {
            self.start_scan(now);
        }
    }

    /// Block until all background work in flight finishes, and apply it.
    pub fn wait_for_background(&mut self) {
        self.collect_background(true);
    }

    fn collect_background(&mut self, block: bool) {
        self.collect_scan(block);
        self.collect_refresh(block);
        // An operation can move straight on to its next stage (e.g. a
        // one-Package Plan starts running), so keep collecting until it
        // stops or waits for the user.
        while self.collect_operation(block) && block {}
    }

    // ---- operations ------------------------------------------------------

    fn targets(&self) -> Vec<Target> {
        self.packages.iter().map(Target::from).collect()
    }

    /// Start updating the selected Package.
    fn update_selected(&mut self) {
        if let Some(operation) = &self.operation
            && operation.busy()
        {
            self.notice = Some(format!("Busy: wait for “{}” to finish", operation.title));
            return;
        }
        let Some(package) = self.selected_package().cloned() else {
            return;
        };
        let title = format!("Update {}", package.name);
        if package.name == BREWMAX {
            self.operation = Some(Operation {
                title,
                plan: None,
                stage: Stage::Held(vec![(BREWMAX.to_string(), vec![Reason::Brewmax])]),
                log: Vec::new(),
                events: None,
            });
            return;
        }
        if package.pinned {
            let message = format!("{} is Pinned; unpin it to update it", package.name);
            self.operation = Some(Operation::refused(title, message));
            return;
        }
        if self
            .outdated()
            .is_some_and(|o| !o.contains_key(&package.name))
        {
            let message = format!("{} is already up to date", package.name);
            self.operation = Some(Operation::refused(title, message));
            return;
        }

        let system = self.system.clone();
        let targets = self.targets();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(Event::Planned(operation::plan(&system, &package, &targets)));
        });
        self.operation = Some(Operation {
            title,
            plan: None,
            stage: Stage::Planning,
            log: Vec::new(),
            events: Some(rx),
        });
    }

    /// Run the current operation's Plan in the background.
    fn execute(&mut self) {
        let targets = self.targets();
        let Some(operation) = &mut self.operation else {
            return;
        };
        let Some(plan) = operation.plan.clone() else {
            return;
        };
        let system = self.system.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || operation::execute(&system, &plan, &targets, &tx));
        operation.log.push(format!(
            "$ {}",
            operation.plan.as_ref().unwrap().upgrade_command()
        ));
        operation.stage = Stage::Running;
        operation.events = Some(rx);
    }

    /// Apply the current operation's pending events. Returns whether its
    /// background work is still in flight.
    fn collect_operation(&mut self, block: bool) -> bool {
        let Some(rx) = self.operation.as_mut().and_then(|o| o.events.take()) else {
            return false;
        };
        loop {
            let event = if block {
                match rx.recv() {
                    Ok(event) => event,
                    Err(_) => return self.operation_in_flight(),
                }
            } else {
                match rx.try_recv() {
                    Ok(event) => event,
                    Err(mpsc::TryRecvError::Empty) => {
                        if let Some(operation) = &mut self.operation {
                            operation.events = Some(rx);
                        }
                        return true;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => return self.operation_in_flight(),
                }
            };
            self.apply(event);
            // Applying the event may have started the next stage with its own
            // receiver (a one-Package Plan starts running straight away). This
            // stage's receiver is then finished with: stop reading it, and
            // never put it back over the new one.
            if self.operation_in_flight() {
                return true;
            }
        }
    }

    fn operation_in_flight(&self) -> bool {
        self.operation.as_ref().is_some_and(|o| o.events.is_some())
    }

    fn apply(&mut self, event: Event) {
        let Some(operation) = &mut self.operation else {
            return;
        };
        match event {
            Event::Planned(Err(error)) => {
                operation.stage = Stage::Done {
                    ok: false,
                    message: error,
                };
            }
            Event::Planned(Ok((plan, held))) => {
                let needs_confirmation = plan.needs_confirmation();
                operation.plan = Some(plan);
                if !held.is_empty() {
                    operation.stage = Stage::Held(held);
                } else if needs_confirmation {
                    operation.stage = Stage::Confirming;
                } else {
                    self.execute();
                }
            }
            Event::Held(held) => operation.stage = Stage::Held(held),
            Event::Line(line) => operation.log.push(line),
            Event::Finished { success, error } => {
                let message = match (&error, success) {
                    (Some(error), _) => error.clone(),
                    (None, true) => format!("{}: done", operation.title),
                    (None, false) => format!("{}: brew reported a failure", operation.title),
                };
                operation.stage = Stage::Done {
                    ok: success && error.is_none(),
                    message,
                };
            }
            Event::Reloaded { packages, outdated } => {
                if let Ok(packages) = packages {
                    self.packages = packages;
                }
                if let (Ok(outdated), RefreshState::Done(refresh)) = (outdated, &mut self.refresh) {
                    refresh.outdated = Ok(outdated);
                }
                self.clamp_selection();
            }
        }
    }

    fn confirm(&mut self, yes: bool) {
        let Some(operation) = &mut self.operation else {
            return;
        };
        if yes {
            self.execute();
        } else {
            operation.stage = Stage::Done {
                ok: false,
                message: format!("{}: cancelled, nothing was changed", operation.title),
            };
        }
    }

    fn start_refresh(&mut self) {
        let system = self.system.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(outdated::refresh(&system));
        });
        self.refresh = RefreshState::Running(rx);
    }

    fn collect_refresh(&mut self, block: bool) {
        let RefreshState::Running(rx) = &self.refresh else {
            return;
        };
        let result = match receive(rx, block) {
            None => return,
            Some(Ok(refresh)) => refresh,
            Some(Err(())) => Refresh {
                update_error: None,
                outdated: Err("refresh stopped unexpectedly".to_string()),
            },
        };
        self.refresh = RefreshState::Done(result);
        self.clamp_selection();
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
        let Some(result) = receive(rx, block) else {
            return;
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

    // ---- keys ------------------------------------------------------------

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.notice = None;
        if let Some(Operation {
            stage: Stage::Confirming,
            ..
        }) = &self.operation
        {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => self.confirm(true),
                KeyCode::Char('n') | KeyCode::Esc => self.confirm(false),
                _ => {}
            }
            return;
        }
        if self.help {
            if matches!(key.code, KeyCode::Char('?') | KeyCode::Esc) {
                self.help = false;
            }
            return;
        }
        if self.filtering {
            match key.code {
                KeyCode::Esc => {
                    self.filter.clear();
                    self.filtering = false;
                }
                KeyCode::Enter => self.filtering = false,
                KeyCode::Backspace => {
                    self.filter.pop();
                }
                KeyCode::Char(c) => self.filter.push(c),
                KeyCode::Down => self.move_selection(1),
                KeyCode::Up => self.move_selection(-1),
                _ => {}
            }
            self.clamp_selection();
            return;
        }
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Tab | KeyCode::BackTab => {
                self.tab = self.tab.next();
                self.selected = 0;
            }
            KeyCode::Char('1') => self.switch_tab(Tab::Installed),
            KeyCode::Char('2') => self.switch_tab(Tab::Outdated),
            KeyCode::Char('/') => self.filtering = true,
            KeyCode::Esc => {
                self.filter.clear();
                self.clamp_selection();
            }
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('u') => self.update_selected(),
            KeyCode::Char('q') => match &self.operation {
                Some(operation) if operation.busy() => {
                    self.notice = Some(format!(
                        "Can't quit while “{}” is running; wait for it to finish",
                        operation.title
                    ));
                }
                _ => self.quit = true,
            },
            _ => {}
        }
    }

    fn switch_tab(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.selected = 0;
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let count = self.selectable().len();
        if count == 0 {
            return;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(count - 1);
    }

    fn clamp_selection(&mut self) {
        let count = self.selectable().len();
        self.selected = self.selected.min(count.saturating_sub(1));
    }

    // ---- what's visible --------------------------------------------------

    fn outdated(&self) -> Option<&outdated::Outdated> {
        match &self.refresh {
            RefreshState::Done(Refresh {
                outdated: Ok(outdated),
                ..
            }) => Some(outdated),
            _ => None,
        }
    }

    fn matches_filter(&self, package: &Package) -> bool {
        package
            .name
            .to_lowercase()
            .contains(&self.filter.to_lowercase())
    }

    /// The rows of the current tab's list, after filtering.
    fn items(&self) -> Vec<Item> {
        let visible = |i: &usize| self.matches_filter(&self.packages[*i]);
        let all = 0..self.packages.len();
        match self.tab {
            Tab::Installed => all.filter(visible).map(Item::Package).collect(),
            Tab::Outdated => {
                let Some(outdated) = self.outdated() else {
                    return Vec::new();
                };
                let is_outdated = |i: &usize| outdated.contains_key(&self.packages[*i].name);
                let (self_updating, regular): (Vec<usize>, Vec<usize>) = all
                    .filter(is_outdated)
                    .filter(visible)
                    .partition(|i| self.packages[*i].self_updating);
                let mut items: Vec<Item> = regular.into_iter().map(Item::Package).collect();
                if !self_updating.is_empty() {
                    items.push(Item::Label("── Self-Updating"));
                    items.extend(self_updating.into_iter().map(Item::Package));
                }
                items
            }
        }
    }

    /// Package indices of the selectable rows, in display order.
    fn selectable(&self) -> Vec<usize> {
        self.items()
            .into_iter()
            .filter_map(|item| match item {
                Item::Package(i) => Some(i),
                Item::Label(_) => None,
            })
            .collect()
    }

    fn selected_package(&self) -> Option<&Package> {
        self.selectable()
            .get(self.selected)
            .map(|i| &self.packages[*i])
    }

    // ---- rendering -------------------------------------------------------

    pub fn render(&self, frame: &mut Frame) {
        let [header_area, main_area, footer_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(frame.area());
        let log_height = if self.operation.is_some() { 9 } else { 0 };
        let [main_area, log_area] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(log_height)])
                .areas(main_area);
        let [list_area, detail_area] =
            Layout::horizontal([Constraint::Percentage(65), Constraint::Percentage(35)])
                .areas(main_area);

        self.render_header(frame, header_area);
        self.render_list(frame, list_area);
        frame.render_widget(self.detail(), detail_area);
        frame.render_widget(self.footer(), footer_area);
        if let Some(operation) = &self.operation {
            render_operation(frame, log_area, operation);
            if matches!(operation.stage, Stage::Confirming) {
                render_confirmation(frame, operation);
            }
        }
        if self.help {
            render_help(frame);
        }
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let [tabs_area, status_area] =
            Layout::horizontal([Constraint::Length(24), Constraint::Fill(1)]).areas(area);
        let selected = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0);
        let tabs = Tabs::new(Tab::ALL.iter().map(|t| t.title()))
            .select(selected)
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        frame.render_widget(tabs, tabs_area);

        let status = match &self.refresh {
            RefreshState::NotStarted | RefreshState::Running(_) => "⟳ Refreshing…".to_string(),
            RefreshState::Done(Refresh {
                update_error: Some(error),
                ..
            }) => format!("⚠ Could not refresh, showing last known versions: {error}"),
            RefreshState::Done(_) => String::new(),
        };
        frame.render_widget(Paragraph::new(status).right_aligned(), status_area);
    }

    fn render_list(&self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered().title(format!(" {} ", self.tab.title()));
        if let Some(message) = self.list_message() {
            let message = Paragraph::new(message)
                .block(block)
                .wrap(Wrap { trim: true });
            frame.render_widget(message, area);
            return;
        }

        let items = self.items();
        let selected_package = self.selectable().get(self.selected).copied();
        let mut selected_row = None;
        let rows: Vec<Row> = items
            .iter()
            .enumerate()
            .map(|(row, item)| match item {
                Item::Label(label) => Row::new(vec![label.to_string()])
                    .style(Style::new().add_modifier(Modifier::DIM)),
                Item::Package(i) => {
                    if Some(*i) == selected_package {
                        selected_row = Some(row);
                    }
                    self.package_row(&self.packages[*i])
                }
            })
            .collect();
        let versions_width = if self.tab == Tab::Outdated { 30 } else { 16 };
        let table = Table::new(
            rows,
            [
                Constraint::Min(18),
                Constraint::Length(15),
                Constraint::Length(7),
                Constraint::Length(versions_width),
            ],
        )
        .block(block)
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        let mut state = TableState::default().with_selected(selected_row);
        frame.render_stateful_widget(table, area, &mut state);
    }

    /// A message to show instead of the list, if the list can't be shown.
    fn list_message(&self) -> Option<String> {
        if let Some(error) = &self.load_error {
            return Some(format!("Could not read installed packages:\n{error}"));
        }
        if self.tab == Tab::Outdated {
            match &self.refresh {
                RefreshState::NotStarted | RefreshState::Running(_) => {
                    return Some("Checking for updates…".to_string());
                }
                RefreshState::Done(Refresh {
                    outdated: Err(error),
                    ..
                }) => return Some(format!("Could not check for updates:\n{error}")),
                RefreshState::Done(_) => {}
            }
        }
        None
    }

    fn package_row(&self, package: &Package) -> Row<'static> {
        let mut tags = Vec::new();
        if self.in_use.contains_key(&package.name) {
            tags.push("● In Use");
        }
        if package.pinned {
            tags.push("Pinned");
        }
        let versions = match (self.tab, self.available(package)) {
            (Tab::Outdated, Some(available)) => format!("{} → {available}", package.version),
            _ => package.version.clone(),
        };
        Row::new(vec![
            package.name.clone(),
            tags.join(" "),
            package.kind.label().to_string(),
            versions,
        ])
    }

    fn available(&self, package: &Package) -> Option<&String> {
        self.outdated()?.get(&package.name)
    }

    fn detail(&self) -> Paragraph<'_> {
        let block = Block::bordered().title(" Details ");
        let Some(package) = self.selected_package() else {
            return Paragraph::new("No package selected").block(block);
        };
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let field = |label: &'static str, value: String| {
            Line::from(vec![Span::styled(label, bold), Span::raw(value)])
        };
        let mut lines = vec![
            Line::styled(package.name.clone(), bold),
            field("Kind:      ", package.kind.label().to_string()),
            field("Version:   ", package.version.clone()),
        ];
        if let Some(available) = self.available(package) {
            lines.push(field("Available: ", available.clone()));
        }
        if package.pinned {
            lines.push(Line::raw("Pinned: not updated until unpinned"));
        }
        if package.self_updating {
            lines.push(Line::raw("Self-Updating: updates itself"));
        }
        if let Some(reasons) = self.in_use.get(&package.name) {
            lines.push(Line::default());
            lines.push(Line::styled("In Use by:", bold));
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

    fn footer(&self) -> Paragraph<'_> {
        if let Some(notice) = &self.notice {
            return Paragraph::new(format!(" {notice}"));
        }
        if self.filtering {
            return Paragraph::new(format!(" Filter: {}▏  Enter keep · Esc clear", self.filter));
        }
        let filter = if self.filter.is_empty() {
            String::new()
        } else {
            format!("  [filter: {}]", self.filter)
        };
        Paragraph::new(format!(
            " ↑↓/jk move · Tab switch tab · u update · / filter · ? help · q quit{filter}"
        ))
    }
}

fn render_help(frame: &mut Frame) {
    let area = frame.area();
    let width = 50.min(area.width);
    let height = 14.min(area.height);
    let popup = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    let lines = [
        ("↑ ↓  j k", "Move the selection"),
        ("Tab  1 2", "Switch between Installed and Outdated"),
        ("u", "Update the selected Package"),
        ("/", "Filter the list by name"),
        ("Enter", "Keep the filter, back to the list"),
        ("Esc", "Clear the filter"),
        ("?", "Open or close this help"),
        ("q", "Quit"),
    ]
    .into_iter()
    .map(|(keys, action)| {
        Line::from(vec![
            Span::styled(
                format!(" {keys:<10}"),
                Style::new().add_modifier(Modifier::BOLD),
            ),
            Span::raw(action),
        ])
    })
    .collect::<Vec<_>>();
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(" Help ")),
        popup,
    );
}

/// Take a finished result from a background worker: `None` if it isn't done
/// yet (when not blocking), `Some(Err(()))` if the worker died without one.
fn receive<T>(rx: &Receiver<T>, block: bool) -> Option<Result<T, ()>> {
    if block {
        return Some(rx.recv().map_err(|_| ()));
    }
    match rx.try_recv() {
        Ok(value) => Some(Ok(value)),
        Err(mpsc::TryRecvError::Empty) => None,
        Err(mpsc::TryRecvError::Disconnected) => Some(Err(())),
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

/// The bottom pane: what the operation is doing and `brew`'s latest output.
fn render_operation(frame: &mut Frame, area: Rect, operation: &Operation) {
    let status = match &operation.stage {
        Stage::Planning => "Working out what would change…".to_string(),
        Stage::Confirming => "Waiting for confirmation".to_string(),
        Stage::Running => "Running…".to_string(),
        Stage::Held(held) => held_message(held),
        Stage::Done { ok: true, message } => format!("✔ {message}"),
        Stage::Done { ok: false, message } => format!("✘ {message}"),
    };
    let visible = area.height.saturating_sub(3) as usize;
    let mut lines = vec![Line::styled(
        status,
        Style::new().add_modifier(Modifier::BOLD),
    )];
    let skip = operation.log.len().saturating_sub(visible);
    lines.extend(
        operation
            .log
            .iter()
            .skip(skip)
            .map(|l| Line::raw(l.clone())),
    );
    frame.render_widget(
        Paragraph::new(lines).block(Block::bordered().title(format!(" {} ", operation.title))),
        area,
    );
}

fn held_message(held: &Held) -> String {
    if held.iter().any(|(name, _)| name == BREWMAX) {
        return "Held: brewmax can't update itself while running. Quit and run `brew upgrade brewmax`."
            .to_string();
    }
    let parts: Vec<String> = held
        .iter()
        .map(|(name, reasons)| format!("{name} is In Use by {}", summarise(reasons).join(", ")))
        .collect();
    format!("Held, nothing was changed: {}", parts.join("; "))
}

fn render_confirmation(frame: &mut Frame, operation: &Operation) {
    let Some(plan) = &operation.plan else {
        return;
    };
    let mut lines = vec![
        Line::raw(format!("This will change {} packages:", plan.steps.len())),
        Line::default(),
    ];
    lines.extend(
        plan.steps
            .iter()
            .map(|step| Line::raw(format!("  {:<20} {} → {}", step.name, step.from, step.to))),
    );
    lines.push(Line::default());
    lines.push(Line::styled(
        "Continue? y / n",
        Style::new().add_modifier(Modifier::BOLD),
    ));
    let area = frame.area();
    let width = 60.min(area.width);
    let height = (lines.len() as u16 + 2).min(area.height);
    let popup = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(format!(" Confirm: {} ", operation.title))),
        popup,
    );
}

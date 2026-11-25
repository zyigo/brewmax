use brewmax::app::App;
use brewmax::system::MacSystem;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

fn main() -> anyhow::Result<()> {
    let app = App::new(MacSystem);
    // `ratatui::init` installs a panic hook that restores the terminal.
    let terminal = ratatui::init();
    let result = run(terminal, app);
    ratatui::restore();
    result
}

fn run(mut terminal: DefaultTerminal, mut app: App<MacSystem>) -> anyhow::Result<()> {
    while !app.should_quit() {
        terminal.draw(|frame| app.render(frame))?;
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            app.handle_key(key);
        }
    }
    Ok(())
}

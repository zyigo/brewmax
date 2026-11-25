use std::time::{Duration, Instant};

use brewmax::app::App;
use brewmax::system::MacSystem;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

const USAGE: &str = "\
Usage: brewmax [--version | --help]

A terminal UI for managing Homebrew Packages without disrupting the ones
that are In Use. Run with no arguments to start it; press ? inside for keys.

Options:
  -V, --version  Print the version and exit
  -h, --help     Print this help and exit";

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        [] => {}
        ["-V" | "--version"] => {
            // The Homebrew formula's test checks this exact output.
            println!("brewmax {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        ["-h" | "--help"] => {
            println!("{USAGE}");
            return Ok(());
        }
        _ => {
            eprintln!("brewmax: unexpected arguments: {}\n\n{USAGE}", args.join(" "));
            std::process::exit(2);
        }
    }

    let app = App::new(MacSystem);
    // `ratatui::init` installs a panic hook that restores the terminal.
    let terminal = ratatui::init();
    let result = run(terminal, app);
    ratatui::restore();
    result
}

fn run(mut terminal: DefaultTerminal, mut app: App<MacSystem>) -> anyhow::Result<()> {
    while !app.should_quit() {
        app.tick(Instant::now());
        terminal.draw(|frame| app.render(frame))?;
        // Wake regularly so background results (In Use scans) get drawn.
        if event::poll(Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            app.handle_key(key);
        }
    }
    Ok(())
}

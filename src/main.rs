use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};
use std::io;
use tokio::signal;

mod app;
mod config;
mod database;
mod kitty;
mod preview;
mod terminal;
mod ui;

use app::App;

#[tokio::main]
async fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut app = App::new().await?;

    // Setup Ctrl+C handler
    let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())?;

    // Run the app
    let res = tokio::select! {
        app_res = run_app(&mut terminal, &mut app) => app_res,
        _ = sigint.recv() => {
            // Graceful shutdown on Ctrl+C
            Ok(())
        }
    };

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    let mut last_tick = std::time::Instant::now();
    let tick_rate = std::time::Duration::from_millis(250);

    loop {
        // Draw the UI
        terminal.draw(|f| ui::draw(f, app))?;

        // Handle events with timeout
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if crossterm::event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char('j') | KeyCode::Down => app.next().await,
                        KeyCode::Char('k') | KeyCode::Up => app.previous().await,
                        KeyCode::Char('h') | KeyCode::Left => app.go_back().await,
                        KeyCode::Char('l') | KeyCode::Right => app.enter().await,
                        KeyCode::Char(' ') => app.toggle_favorite(),
                        KeyCode::Char('f') => app.toggle_favorite(),
                        KeyCode::Char('r') => {
                            if let Err(e) = app.set_random_wallpaper().await {
                                eprintln!("Error: {:?}", e);
                            }
                        }
                        KeyCode::Char('R') => app.show_recent_wallpapers(),
                        KeyCode::Enter => {
                            if let Err(e) = app.set_wallpaper().await {
                                eprintln!("Error: {:?}", e);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Handle tick events
        if last_tick.elapsed() >= tick_rate {
            app.on_tick();
            last_tick = std::time::Instant::now();
        }
    }
}

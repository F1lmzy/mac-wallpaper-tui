use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
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
    execute!(stdout, EnterAlternateScreen, event::EnableMouseCapture)?;
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
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        event::DisableMouseCapture
    )?;
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
    let double_click_threshold = std::time::Duration::from_millis(500);

    // Store browser area for mouse click detection
    let mut browser_area = ratatui::layout::Rect::default();

    loop {
        // Draw the UI and capture the browser area
        terminal.draw(|f| {
            let chunks = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Vertical)
                .constraints([
                    ratatui::layout::Constraint::Min(0),
                    ratatui::layout::Constraint::Length(3),
                ])
                .split(f.area());

            let main_chunks = ratatui::layout::Layout::default()
                .direction(ratatui::layout::Direction::Horizontal)
                .constraints([
                    ratatui::layout::Constraint::Percentage(30),
                    ratatui::layout::Constraint::Percentage(70),
                ])
                .split(chunks[0]);

            browser_area = main_chunks[0];
            ui::draw(f, app);
        })?;

        // Check if preview area changed and reload if necessary
        app.check_and_reload_preview().await;

        // Handle events with timeout
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if crossterm::event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
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
                Event::Mouse(mouse) => {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            app.scroll_up();
                            app.update_preview().await;
                        }
                        MouseEventKind::ScrollDown => {
                            app.scroll_down();
                            app.update_preview().await;
                        }
                        MouseEventKind::Down(MouseButton::Left) => {
                            // Detect double click
                            let now = std::time::Instant::now();
                            let clicked_index = if mouse.row >= browser_area.y + 2
                                && mouse.row < browser_area.y + browser_area.height - 1
                            {
                                Some((mouse.row - browser_area.y - 2) as usize)
                            } else {
                                None
                            };

                            let is_double_click = if let Some(idx) = clicked_index {
                                app.last_click_index == Some(idx)
                                    && now.duration_since(app.last_click_time)
                                        < double_click_threshold
                            } else {
                                false
                            };

                            if is_double_click {
                                // Double click - set wallpaper
                                app.handle_mouse_click(mouse.row, browser_area, true).await;
                                app.last_click_index = None; // Reset to prevent triple click
                            } else {
                                // Single click - preview
                                app.handle_mouse_click(mouse.row, browser_area, false).await;
                                app.last_click_time = now;
                                app.last_click_index = clicked_index;
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        // Handle tick events
        if last_tick.elapsed() >= tick_rate {
            app.on_tick().await;
            last_tick = std::time::Instant::now();
        }
    }
}

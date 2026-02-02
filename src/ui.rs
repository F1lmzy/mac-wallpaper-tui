use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, StatefulWidget, Wrap},
    Frame,
};
use ratatui_image::StatefulImage;

use crate::app::App;

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(f.area());

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(chunks[0]);

    // Left panel - file browser
    draw_browser(f, app, main_chunks[0]);

    // Right panel - preview/info
    draw_preview(f, app, main_chunks[1]);

    // Bottom panel - help/status
    draw_status_bar(f, app, chunks[1]);
}

fn draw_browser(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .items
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let is_selected = i == app.selected_index;
            let is_favorite = app.is_favorite(path);

            let base_style = if is_selected {
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let icon = if path.is_dir() {
                "[DIR]"
            } else if is_favorite {
                "[FAV]"
            } else {
                "[IMG]"
            };

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string_lossy().to_string());

            let line = Line::from(vec![
                Span::raw(format!("{} ", icon)),
                Span::styled(name, base_style),
            ]);

            ListItem::new(line).style(base_style)
        })
        .collect();

    let title = if app.show_recent {
        " Recent Wallpapers ".to_string()
    } else {
        format!(" {} ", app.current_dir.display())
    };

    let browser = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );

    f.render_widget(browser, area);
}

fn draw_preview(f: &mut Frame, app: &mut App, area: Rect) {
    // Split the preview area into image and info sections
    let preview_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    // Image display area
    let image_block = Block::default()
        .title(" Preview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let image_inner = image_block.inner(preview_chunks[0]);
    
    // Store the preview area for image loading
    app.preview_area = Some(image_inner);

    f.render_widget(image_block, preview_chunks[0]);

    // Render image if available
    if let Some(ref mut protocol) = app.current_protocol {
        let image_widget = StatefulImage::default();
        // Use StatefulWidget to render the image (protocol is passed by mutable reference)
        StatefulWidget::render(image_widget, image_inner, f.buffer_mut(), protocol);
    } else if let Some(selected) = app.selected_item() {
        if selected.is_dir() {
            let text = Paragraph::new("Directory\n\nPress l or right arrow to open")
                .alignment(Alignment::Center);
            f.render_widget(text, image_inner);
        } else if app.config.is_valid_image(selected) {
            // Fast loading - show brief loading message
            let text = Paragraph::new("Loading...").alignment(Alignment::Center);
            f.render_widget(text, image_inner);
        }
    }

    // Info area
    draw_info_panel(f, app, preview_chunks[1]);
}

fn draw_info_panel(f: &mut Frame, app: &App, area: Rect) {
    let mut text = vec![];

    if let Some(selected) = app.selected_item() {
        let name = selected
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| selected.to_string_lossy().to_string());

        text.push(Line::from(vec![
            Span::styled("Name: ", Style::default().fg(Color::Yellow)),
            Span::raw(name),
        ]));

        if selected.is_file() {
            if let Ok(metadata) = std::fs::metadata(selected) {
                let size = metadata.len();
                let size_str = if size > 1024 * 1024 {
                    format!("{:.2} MB", size as f64 / (1024.0 * 1024.0))
                } else if size > 1024 {
                    format!("{:.2} KB", size as f64 / 1024.0)
                } else {
                    format!("{} B", size)
                };

                text.push(Line::from(vec![
                    Span::styled("Size: ", Style::default().fg(Color::Yellow)),
                    Span::raw(size_str),
                ]));
            }

            if let Some(ref cached) = app.current_preview {
                text.push(Line::from(vec![
                    Span::styled("Dimensions: ", Style::default().fg(Color::Yellow)),
                    Span::raw(format!("{}x{}", cached.dimensions.0, cached.dimensions.1)),
                ]));
            }

            if app.is_favorite(selected) {
                text.push(Line::from(vec![Span::styled(
                    "* Favorite",
                    Style::default().fg(Color::Yellow),
                )]));
            }
        }
    }

    let info = Paragraph::new(Text::from(text))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(info, area);
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    // Show status message if present, otherwise show help
    let content = if let Some(ref msg) = app.status_message {
        Paragraph::new(Text::from(vec![Line::from(vec![Span::styled(
            msg.clone(),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )])]))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green)),
        )
    } else {
        let help_text = vec![
            Line::from(vec![
                Span::styled(
                    "j/k",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("=nav "),
                Span::styled(
                    "l/→",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("=enter "),
                Span::styled(
                    "h/←",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("=back "),
                Span::styled(
                    "f",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("=fav "),
                Span::styled(
                    "↵",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("=set "),
                Span::styled(
                    "q",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("=quit"),
            ]),
            Line::from(vec![
                Span::styled(
                    "r",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("=random "),
                Span::styled(
                    "R",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("=recent "),
                Span::styled(
                    "?",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("=help"),
            ]),
        ];

        Paragraph::new(Text::from(help_text))
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
    };

    f.render_widget(content, area);
}

use crate::app::App;
use crate::config::{AgentStatus, SyncStrategy};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table, Wrap},
};

pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(10),   // Central Workspace
            Constraint::Length(6), // Footer Status Table (increased height slightly)
            Constraint::Length(1), // Status Message
            Constraint::Length(1), // Key Hints
        ])
        .split(f.area());

    // Header
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Project: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(app.paths.project_agents.parent().unwrap().to_string_lossy()),
        ]),
        Line::from(vec![
            Span::styled("Global:  ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(app.paths.global_rules.to_string_lossy()),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Agent Context Manager"),
    );
    f.render_widget(header, chunks[0]);

    // Central Workspace (Split View)
    let workspace_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    let global_rules = Paragraph::new(app.global_content.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Global Rules (USER_RULES.md)"),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(global_rules, workspace_chunks[0]);

    let project_rules = Paragraph::new(app.project_content.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Project Rules (AGENTS.md)"),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(project_rules, workspace_chunks[1]);

    // Footer Status Table
    let rows: Vec<Row> = app
        .agents
        .iter()
        .map(|agent| {
            let (status_text, status_style) = match agent.status {
                AgentStatus::Ok => ("OK", Style::default().fg(Color::Green)),
                AgentStatus::Missing => ("MISSING", Style::default().fg(Color::Red)),
                AgentStatus::Drift => ("DRIFT", Style::default().fg(Color::Yellow)),
            };
            let strategy_text = match agent.strategy {
                SyncStrategy::Merge => "Merge",
                SyncStrategy::Symlink => "Symlink",
            };
            Row::new(vec![
                agent.name.clone(),
                status_text.to_string(),
                strategy_text.to_string(),
                agent
                    .target_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ])
            .style(status_style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(10), // Agent
            Constraint::Length(10), // Status
            Constraint::Length(10), // Strategy
            Constraint::Min(10),    // Target File
        ],
    )
    .header(
        Row::new(vec!["Agent", "Status", "Strategy", "Target File"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Agent Status Audit"),
    );
    f.render_widget(table, chunks[2]);

    // Status Message
    if let Some((msg, _)) = &app.status_message {
        f.render_widget(
            Paragraph::new(Span::styled(
                msg,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            chunks[3],
        );
    }

    let hints = Line::from(vec![
        Span::styled("[q]", Style::default().fg(Color::Cyan)),
        Span::raw(" Quit | "),
        Span::styled("[s]", Style::default().fg(Color::Cyan)),
        Span::raw(" Sync/Fix | "),
        Span::styled("[e]", Style::default().fg(Color::Cyan)),
        Span::raw(" Edit Project Rules | "),
        Span::styled("[g]", Style::default().fg(Color::Cyan)),
        Span::raw(" Edit Global Rules"),
    ]);
    f.render_widget(Paragraph::new(hints), chunks[4]);
}

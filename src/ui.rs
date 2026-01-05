use crate::app::{App, AppMode};
use crate::config::{AgentStatus, SyncStrategy};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Row, Table, Wrap},
    Frame,
};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

fn highlight_markdown(content: &str) -> Vec<Line<'_>> {
    let ps = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let syntax = ps
        .find_syntax_by_extension("md")
        .unwrap_or_else(|| ps.find_syntax_plain_text());
    let theme = &ts.themes["base16-ocean.dark"];
    let mut highlighter = HighlightLines::new(syntax, theme);

    content
        .lines()
        .map(|line| {
            let ranges = highlighter.highlight_line(line, &ps).unwrap_or_default();
            let spans: Vec<Span> = ranges
                .iter()
                .map(|(style, text)| {
                    Span::styled(
                        text.to_string(),
                        Style::default().fg(Color::Rgb(
                            style.foreground.r,
                            style.foreground.g,
                            style.foreground.b,
                        )),
                    )
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

pub fn render(f: &mut Frame, app: &App) {
    match app.mode {
        AppMode::Help => {
            render_help(f);
            return;
        }
        AppMode::ViewDiff => {
            render_diff(f, app);
            return;
        }
        AppMode::ViewBackups => {
            render_backups(f, app);
            return;
        }
        AppMode::Search => {
            render_main(f, app);
            render_search_dialog(f, app);
            return;
        }
        AppMode::ConfirmSync | AppMode::ConfirmSyncAll => {
            render_main(f, app);
            render_confirm_dialog(f, app);
            return;
        }
        _ => {}
    }

    if app.show_error_log {
        render_error_log(f, app);
    } else {
        render_main(f, app);
    }
}

fn render_main(f: &mut Frame, app: &App) {
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

    let drifted_agents = app.paths.check_global_rules_drift();
    let sync_indicator = if drifted_agents.is_empty() {
        Span::styled(" ✓", Style::default().fg(Color::Green))
    } else {
        Span::styled(
            format!(" ⚠ {} agents out of sync", drifted_agents.len()),
            Style::default().fg(Color::Yellow),
        )
    };

    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Project: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(
                app.paths
                    .project_agents
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Unknown".to_string()),
            ),
        ]),
        Line::from(vec![
            Span::styled("Global: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(app.paths.global_rules_primary.display().to_string()),
            sync_indicator,
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("MooAgent - Agent Context Manager"),
    );
    f.render_widget(header, chunks[0]);

    let workspace_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    let global_content = if app.paths.global_rules_primary.exists() {
        std::fs::read_to_string(&app.paths.global_rules_primary)
            .unwrap_or_else(|_| "Error reading global rules".to_string())
    } else {
        String::new()
    };

    let global_title = "Global Rules (Press 'g' to edit - syncs to all agents)".to_string();
    let global_lines = highlight_markdown(&global_content);

    let global_rules = Paragraph::new(global_lines)
        .block(Block::default().borders(Borders::ALL).title(global_title))
        .wrap(Wrap { trim: true });
    f.render_widget(global_rules, workspace_chunks[0]);

    let project_lines: Vec<Line> = highlight_markdown(&app.project_content)
        .into_iter()
        .skip(app.project_scroll)
        .collect();

    let project_rules = Paragraph::new(project_lines)
        .block(Block::default().borders(Borders::ALL).title(format!(
            "Project Rules (AGENTS.md) [Scroll: {}]",
            app.project_scroll
        )))
        .wrap(Wrap { trim: true });
    f.render_widget(project_rules, workspace_chunks[1]);

    let visible_agents = app.get_visible_agents();
    let rows: Vec<Row> = visible_agents
        .iter()
        .map(|agent| {
            let idx = app
                .agents
                .iter()
                .position(|a| a.name == agent.name)
                .unwrap_or(0);
            let (status_text, mut status_style) = match agent.status {
                AgentStatus::Ok => ("OK", Style::default().fg(Color::Green)),
                AgentStatus::Missing => ("MISSING", Style::default().fg(Color::Red)),
                AgentStatus::Drift => ("DRIFT", Style::default().fg(Color::Yellow)),
            };
            let strategy_text = match agent.strategy {
                SyncStrategy::Merge => "Merge",
                SyncStrategy::Symlink => "Symlink",
            };

            if idx == app.selected_agent {
                status_style = status_style.add_modifier(Modifier::REVERSED);
            }

            Row::new(vec![
                agent.name.clone(),
                status_text.to_string(),
                strategy_text.to_string(),
                agent
                    .target_path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Unknown".to_string()),
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
            .title(if app.search_query.is_empty() {
                "Agent Status Audit".to_string()
            } else {
                format!("Agent Status Audit (filtered: {})", visible_agents.len())
            }),
    );
    f.render_widget(table, chunks[2]);

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

    let auto_sync_indicator = if app.auto_sync { " [AUTO-SYNC ON]" } else { "" };
    let search_indicator = if !app.search_query.is_empty() {
        format!(" [SEARCH: {}]", app.search_query)
    } else {
        String::new()
    };

    let hints = Line::from(vec![
        Span::styled("[?]", Style::default().fg(Color::Cyan)),
        Span::raw(" Help | "),
        Span::styled("[g]", Style::default().fg(Color::Cyan)),
        Span::raw(" Edit Global | "),
        Span::styled("[e]", Style::default().fg(Color::Cyan)),
        Span::raw(" Edit Project | "),
        Span::styled("[s]", Style::default().fg(Color::Cyan)),
        Span::raw(" Sync | "),
        Span::styled("[q]", Style::default().fg(Color::Cyan)),
        Span::raw(" Quit"),
        Span::styled(
            auto_sync_indicator,
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            &search_indicator,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    f.render_widget(Paragraph::new(hints), chunks[4]);
}

fn render_search_dialog(f: &mut Frame, app: &App) {
    let area = f.area();

    let popup_width = 60;
    let popup_height = 5;
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = ratatui::layout::Rect {
        x,
        y,
        width: popup_width,
        height: popup_height,
    };

    let text = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("Search: "),
            Span::styled(
                &app.search_query,
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
        ]),
        Line::from(""),
        Line::from("[Esc] Cancel | [Enter] Apply | [Backspace] Delete | Type to search"),
    ];

    let dialog = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Search Agents")
            .style(Style::default().bg(Color::Black)),
    );

    f.render_widget(Clear, popup_area);
    f.render_widget(dialog, popup_area);
}

fn render_error_log(f: &mut Frame, app: &App) {
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let mut log_lines = Vec::new();
    log_lines.push(Line::from(vec![
        Span::styled(
            "Status Log",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        ),
        Span::raw(format!(" ({} entries)", app.status_log.len())),
    ]));
    log_lines.push(Line::from(""));

    for (msg, time) in app.status_log.iter().rev().take(50) {
        let elapsed = time.elapsed().as_secs();
        let time_str = if elapsed < 60 {
            format!("{}s ago", elapsed)
        } else if elapsed < 3600 {
            format!("{}m ago", elapsed / 60)
        } else {
            format!("{}h ago", elapsed / 3600)
        };

        log_lines.push(Line::from(vec![
            Span::styled(
                format!("[{}] ", time_str),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(msg),
        ]));
    }

    let log = Paragraph::new(log_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Error/Status Log"),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(log, chunks[0]);

    let hint = Line::from(vec![
        Span::styled("[v/Esc]", Style::default().fg(Color::Cyan)),
        Span::raw(" Close Log"),
    ]);
    f.render_widget(Paragraph::new(hint), chunks[1]);
}

fn render_help(f: &mut Frame) {
    let area = f.area();

    let help_text = vec![
        Line::from(vec![Span::styled(
            "MooAgent - Help",
            Style::default()
                .add_modifier(Modifier::BOLD)
                .fg(Color::Cyan),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Navigation:",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  j/k or ↓/↑        - Navigate agents (updates global view)"),
        Line::from("  h/l or H/L        - Scroll project rules up/down"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Actions:",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  s                 - Sync all agents (with confirmation)"),
        Line::from("  Enter             - Sync selected agent (with confirmation)"),
        Line::from("  d                 - View diff for selected agent"),
        Line::from("  b                 - View backups for selected agent"),
        Line::from("  e                 - Edit project rules (AGENTS.md)"),
        Line::from("  g/G               - Edit global rules (syncs to all agents)"),
        Line::from("  c                 - Edit config file (.mooagent.toml)"),
        Line::from("  a                 - Toggle auto-sync mode"),
        Line::from("  /                 - Search agents by name/path"),
        Line::from("  v                 - Toggle error/status log"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Other:",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  ?                 - Show this help"),
        Line::from("  q or Esc          - Quit / Close dialog"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Status Indicators:",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("OK", Style::default().fg(Color::Green)),
            Span::raw("      - Agent is in sync"),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("DRIFT", Style::default().fg(Color::Yellow)),
            Span::raw("   - Agent config differs from expected"),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("MISSING", Style::default().fg(Color::Red)),
            Span::raw(" - Agent config file doesn't exist"),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Features:",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  • Automatic backups before sync (timestamped .bak files)"),
        Line::from("  • File watching - auto-refresh when files change"),
        Line::from("  • Logging to ~/.local/share/mooagent/mooagent.log"),
        Line::from(""),
        Line::from("Press any key to close..."),
    ];

    let help = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL).title("Help"))
        .wrap(Wrap { trim: true });

    f.render_widget(Clear, area);
    f.render_widget(help, area);
}

fn render_confirm_dialog(f: &mut Frame, app: &App) {
    let area = f.area();

    let popup_width = 60;
    let popup_height = 9;
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = ratatui::layout::Rect {
        x,
        y,
        width: popup_width,
        height: popup_height,
    };

    let message = match app.mode {
        AppMode::ConfirmSyncAll => "Sync all agents?",
        AppMode::ConfirmSync => {
            if app.agents.is_empty() {
                "No agents to sync"
            } else {
                "Sync selected agent?"
            }
        }
        _ => "Confirm?",
    };

    let text = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            message,
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from("This will backup and overwrite existing files."),
        Line::from(""),
        Line::from(vec![
            Span::styled("[y]", Style::default().fg(Color::Green)),
            Span::raw(" Yes   "),
            Span::styled("[n/Esc]", Style::default().fg(Color::Red)),
            Span::raw(" No"),
        ]),
    ];

    let dialog = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Confirmation")
                .style(Style::default().bg(Color::Black)),
        )
        .wrap(Wrap { trim: true });

    f.render_widget(Clear, popup_area);
    f.render_widget(dialog, popup_area);
}

fn render_diff(f: &mut Frame, app: &App) {
    let area = f.area();

    let diff_content = if app.agents.is_empty() {
        "No agents available".to_string()
    } else {
        app.paths
            .get_diff(app.selected_agent)
            .unwrap_or_else(|| "No diff available (agent is in sync or missing)".to_string())
    };

    let diff = Paragraph::new(diff_content)
        .block(Block::default().borders(Borders::ALL).title(format!(
                "Diff - {}",
                app.agents
                    .get(app.selected_agent)
                    .map(|a| a.name.as_str())
                    .unwrap_or("Unknown")
            )))
        .wrap(Wrap { trim: true });

    f.render_widget(Clear, area);
    f.render_widget(diff, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let hint = Line::from(vec![
        Span::styled("[Esc/q]", Style::default().fg(Color::Cyan)),
        Span::raw(" Close"),
    ]);
    f.render_widget(Paragraph::new(hint), chunks[1]);
}

fn render_backups(f: &mut Frame, app: &App) {
    let area = f.area();

    let backups = app.paths.list_backups(app.selected_agent);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Backups for ", Style::default()),
            Span::styled(
                app.agents
                    .get(app.selected_agent)
                    .map(|a| a.name.as_str())
                    .unwrap_or("Unknown"),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
    ];

    if backups.is_empty() {
        lines.push(Line::from("No backups found"));
    } else {
        for (idx, backup) in backups.iter().enumerate() {
            let name = backup
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown");
            lines.push(Line::from(format!("  {}. {}", idx + 1, name)));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(
            "Note: Restore functionality requires manual file operations",
        ));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("[Esc/q]", Style::default().fg(Color::Cyan)),
        Span::raw(" Close"),
    ]));

    let backup_list = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Backup Files"))
        .wrap(Wrap { trim: true });

    f.render_widget(Clear, area);
    f.render_widget(backup_list, area);
}

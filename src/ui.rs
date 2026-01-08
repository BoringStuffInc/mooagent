use crate::app::{ActiveTab, App, AppMode, Focus, McpAuthType, McpFieldFocus, PrefEditorFocus};
use crate::config::{AgentStatus, SyncStrategy};
use crate::credentials::TokenStatus;
use crate::preferences::McpAuth;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Row, Table, Tabs, Wrap},
};
use std::sync::LazyLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

fn format_auth_details<'a>(details: &mut Vec<Line<'a>>, auth: &'a McpAuth) {
    match auth {
        McpAuth::None => {}
        McpAuth::Bearer { .. } => {
            details.push(Line::from(""));
            details.push(Line::from(vec![
                Span::styled("Auth: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled("Bearer token", Style::default().fg(Color::Green)),
            ]));
        }
        McpAuth::OAuth {
            client_id, scopes, ..
        } => {
            details.push(Line::from(""));
            details.push(Line::from(vec![
                Span::styled("Auth: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled("OAuth 2.1", Style::default().fg(Color::Yellow)),
            ]));
            details.push(Line::from(vec![
                Span::styled("Client ID: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(client_id.as_str()),
            ]));
            if !scopes.is_empty() {
                details.push(Line::from(vec![
                    Span::styled("Scopes: ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(scopes.join(" ")),
                ]));
            }
        }
    }
}

fn format_oauth_status(details: &mut Vec<Line<'_>>, app: &App, url: &str, auth: &McpAuth) {
    if !matches!(auth, McpAuth::OAuth { .. }) {
        return;
    }

    let status = app.credentials.token_status(url);
    details.push(Line::from(""));

    let (status_text, status_color) = match status {
        TokenStatus::Valid => ("Authenticated", Color::Green),
        TokenStatus::ExpiresSoon => ("Token expires soon", Color::Yellow),
        TokenStatus::Expired => ("Token expired", Color::Red),
        TokenStatus::None => ("Not authenticated", Color::Red),
    };

    details.push(Line::from(vec![
        Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(status_text, Style::default().fg(status_color)),
    ]));

    details.push(Line::from(""));
    match status {
        TokenStatus::Valid => {
            details.push(Line::from(vec![
                Span::styled("[o]", Style::default().fg(Color::Cyan)),
                Span::raw(" Logout"),
            ]));
        }
        _ => {
            details.push(Line::from(vec![
                Span::styled("[o]", Style::default().fg(Color::Cyan)),
                Span::raw(" Login with OAuth"),
            ]));
        }
    }
}

fn highlight_markdown(content: &str) -> Vec<Line<'_>> {
    let ps = &SYNTAX_SET;
    let syntax = ps
        .find_syntax_by_extension("md")
        .unwrap_or_else(|| ps.find_syntax_plain_text());
    let theme = &THEME_SET.themes["base16-ocean.dark"];
    let mut highlighter = HighlightLines::new(syntax, theme);

    content
        .lines()
        .map(|line| {
            let ranges = highlighter.highlight_line(line, ps).unwrap_or_default();
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
            render_help(f, app);
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
            if app.active_tab == ActiveTab::Dashboard {
                render_main(f, app);
                render_search_dialog(f, app);
            }
            return;
        }
        AppMode::ConfirmSync | AppMode::ConfirmSyncAll => {
            render_main(f, app);
            render_confirm_dialog(f, app);
            return;
        }
        AppMode::AddTool => {
            render_preferences(f, app);
            render_add_tool_dialog(f, app);
            return;
        }
        AppMode::EditMcp => {
            render_mcp_servers(f, app);
            render_mcp_edit_dialog(f, app);
            return;
        }
        AppMode::Normal => match app.active_tab {
            ActiveTab::Dashboard => render_main(f, app),
            ActiveTab::Preferences => render_preferences(f, app),
            ActiveTab::McpServers => render_mcp_servers(f, app),
        },
    }

    if app.show_error_log {
        render_error_log(f, app);
    }
}

fn render_tabs(f: &mut Frame, app: &App, area: Rect) {
    let titles = vec!["[1] Dashboard", "[2] Preferences", "[3] MCP Servers"];
    let index = match app.active_tab {
        ActiveTab::Dashboard => 0,
        ActiveTab::Preferences => 1,
        ActiveTab::McpServers => 2,
    };

    let tabs = Tabs::new(titles)
        .select(index)
        .block(Block::default().borders(Borders::BOTTOM))
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::raw("|"));

    f.render_widget(tabs, area);
}

fn render_main(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(6),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    render_tabs(f, app, chunks[0]);

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
    f.render_widget(header, chunks[1]);

    let workspace_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[2]);

    let global_lines: Vec<Line> = highlight_markdown(&app.global_content)
        .into_iter()
        .skip(app.global_scroll)
        .collect();

    let global_title = if app.focus == Focus::Global {
        format!("Global Rules (Focused) [Line: {}]", app.global_scroll)
    } else {
        format!("Global Rules [Line: {}]", app.global_scroll)
    };

    let global_rules = Paragraph::new(global_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(global_title)
                .border_style(if app.focus == Focus::Global {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                }),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(global_rules, workspace_chunks[0]);

    let project_lines: Vec<Line> = highlight_markdown(&app.project_content)
        .into_iter()
        .skip(app.project_scroll)
        .collect();

    let project_title = if app.focus == Focus::Project {
        format!("Project Rules (Focused) [Line: {}]", app.project_scroll)
    } else {
        format!("Project Rules [Line: {}]", app.project_scroll)
    };

    let project_rules = Paragraph::new(project_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(project_title)
                .border_style(if app.focus == Focus::Project {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default()
                }),
        )
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

    let table_title = match (app.focus == Focus::Agents, app.search_query.is_empty()) {
        (true, true) => "Agent Status Audit (Focused)".to_string(),
        (true, false) => format!(
            "Agent Status Audit (Focused) (filtered: {})",
            visible_agents.len()
        ),
        (false, true) => "Agent Status Audit".to_string(),
        (false, false) => format!("Agent Status Audit (filtered: {})", visible_agents.len()),
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Length(10),
            Constraint::Min(10),
        ],
    )
    .header(
        Row::new(vec!["Agent", "Status", "Strategy", "Target File"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(table_title)
            .border_style(if app.focus == Focus::Agents {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            }),
    );
    f.render_widget(table, chunks[3]);

    if let Some((msg, _)) = &app.status_message {
        f.render_widget(
            Paragraph::new(Span::styled(
                msg,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            chunks[4],
        );
    }

    let auto_sync_indicator = if app.auto_sync { " [AUTO-SYNC ON]" } else { "" };
    let search_indicator = if !app.search_query.is_empty() {
        format!(" [SEARCH: {}]", app.search_query)
    } else {
        String::new()
    };

    let hints = Line::from(vec![
        Span::styled("[1/2/3]", Style::default().fg(Color::Cyan)),
        Span::raw(" Tabs | "),
        Span::styled("[?]", Style::default().fg(Color::Cyan)),
        Span::raw(" Help | "),
        Span::styled("[Tab]", Style::default().fg(Color::Cyan)),
        Span::raw(" Focus | "),
        Span::styled("[/]", Style::default().fg(Color::Cyan)),
        Span::raw(" Search | "),
        Span::styled("[s]", Style::default().fg(Color::Cyan)),
        Span::raw(" Sync All | "),
        Span::styled("[Enter]", Style::default().fg(Color::Cyan)),
        Span::raw(" Sync Sel | "),
        Span::styled("[Esc]", Style::default().fg(Color::Cyan)),
        Span::raw(" Clear Msg | "),
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
    f.render_widget(Paragraph::new(hints), chunks[5]);
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
        Line::from("[Esc] Cancel | [Enter] Apply | [Bksp] Del"),
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

fn render_help(f: &mut Frame, app: &App) {
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
        Line::from("  Tab / Ctrl+w      - Cycle focus between panes"),
        Line::from("  h / l or ← / →    - Move focus between Global and Project rules"),
        Line::from("  j / k or ↓ / ↑    - Navigate/Scroll focused pane"),
        Line::from("  gg / G            - Jump to Top / Bottom of focused pane"),
        Line::from("  Ctrl+u / Ctrl+d   - Half-page Up / Down focused pane"),
        Line::from("  Mouse Scroll      - Scroll focused pane"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Actions:",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  s                 - Sync all agents (with confirmation)"),
        Line::from("  Enter             - Sync selected agent (with confirmation)"),
        Line::from("  d                 - View diff for selected agent"),
        Line::from("  b                 - View backups for selected agent"),
        Line::from("  Ctrl+g            - Edit global rules (syncs to all agents)"),
        Line::from("  Ctrl+e            - Edit project rules (AGENTS.md)"),
        Line::from("  Ctrl+c            - Edit config file (.mooagent.toml)"),
        Line::from("  Ctrl+p            - Open Preference Editor (Tool permissions, MCP)"),
        Line::from("  a                 - Toggle auto-sync mode"),
        Line::from("  /                 - Search agents by name/path"),
        Line::from("  v                 - Toggle error/status log"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "MCP Servers (Tab 3):",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  a                 - Add new MCP server"),
        Line::from("  e / Enter         - Edit selected server"),
        Line::from("  d                 - Delete selected server"),
        Line::from("  o                 - OAuth login/logout (for OAuth servers)"),
        Line::from("  m                 - Add default MCP servers (magic setup)"),
        Line::from("  s                 - Sync preferences to all agents"),
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
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Help [Scroll: j/k] [Line: {}]", app.detail_scroll)),
        )
        .wrap(Wrap { trim: true })
        .scroll((app.detail_scroll as u16, 0));

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
                "Diff - {} [Scroll: j/k] [Line: {}]",
                app.agents
                    .get(app.selected_agent)
                    .map(|a| a.name.as_str())
                    .unwrap_or("Unknown"),
                app.detail_scroll
            )))
        .wrap(Wrap { trim: true })
        .scroll((app.detail_scroll as u16, 0));

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
        .block(Block::default().borders(Borders::ALL).title(format!(
            "Backup Files [Scroll: j/k] [Line: {}]",
            app.detail_scroll
        )))
        .wrap(Wrap { trim: true })
        .scroll((app.detail_scroll as u16, 0));

    f.render_widget(Clear, area);
    f.render_widget(backup_list, area);
}

fn render_preferences(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(4),
            Constraint::Min(5),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    render_tabs(f, app, chunks[0]);

    let drift_status = if app.preference_drift {
        Span::styled(
            " [DRIFT DETECTED - Sync Recommended]",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(" [Synced]", Style::default().fg(Color::Green))
    };

    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "Preference Editor: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("Global preferences (synced to all agents)"),
            drift_status,
        ]),
        Line::from(vec![
            Span::styled("Path: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(app.paths.preferences.global_path.display().to_string()),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("MooAgent Preferences"),
    );
    f.render_widget(header, chunks[1]);

    let editor_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(chunks[2]);

    render_presets_panel(f, app, editor_chunks[0]);

    render_tools_panel(f, app, editor_chunks[1]);

    render_general_panel(f, app, editor_chunks[2]);

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

    let mut hint_vec = vec![
        Span::styled("[1/2]", Style::default().fg(Color::Cyan)),
        Span::raw(" Tabs | "),
        Span::styled("[Tab]", Style::default().fg(Color::Cyan)),
        Span::raw(" Next Panel | "),
        Span::styled("[Space]", Style::default().fg(Color::Cyan)),
        Span::raw(" Toggle | "),
    ];

    if app.pref_editor_state.focus == PrefEditorFocus::IndividualTools {
        hint_vec.push(Span::styled("[a]", Style::default().fg(Color::Cyan)));
        hint_vec.push(Span::raw(" Add Tool | "));
    }

    hint_vec.push(Span::styled("[s]", Style::default().fg(Color::Cyan)));
    hint_vec.push(Span::raw(" Sync Configs | "));
    hint_vec.push(Span::styled("[q/Esc]", Style::default().fg(Color::Cyan)));
    hint_vec.push(Span::raw(" Back to Main"));

    let hints = Line::from(hint_vec);
    f.render_widget(Paragraph::new(hints), chunks[4]);
}

fn render_add_tool_dialog(f: &mut Frame, app: &App) {
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
            Span::raw("Tool Name: "),
            Span::styled(
                &app.new_tool_input,
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
        ]),
        Line::from(""),
        Line::from("[Esc] Cancel | [Enter] Add Tool"),
    ];

    let dialog = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Add Individual Tool")
            .style(Style::default().bg(Color::Black)),
    );

    f.render_widget(Clear, popup_area);
    f.render_widget(dialog, popup_area);
}

fn render_presets_panel(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let is_focused = app.pref_editor_state.focus == PrefEditorFocus::Presets;
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Tool Presets")
        .border_style(if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        });

    let mut lines = Vec::new();

    for (idx, preset) in app.pref_editor_state.preset_list.iter().enumerate() {
        let state = app.get_preset_state(preset);
        let check = match state {
            crate::app::PresetState::All => "[x]",
            crate::app::PresetState::None => "[ ]",
            crate::app::PresetState::Partial => "[-]",
        };

        let style = if is_focused && idx == app.pref_editor_state.selected_preset {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            match state {
                crate::app::PresetState::None => Style::default().fg(Color::DarkGray),
                _ => Style::default().fg(Color::Green),
            }
        };

        lines.push(Line::from(vec![Span::styled(
            format!("{} {}", check, preset),
            style,
        )]));
    }

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_tools_panel(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let is_focused = app.pref_editor_state.focus == PrefEditorFocus::IndividualTools;
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Individual Tools")
        .border_style(if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        });

    let mut lines = Vec::new();
    let mgr = &app.paths.preferences;

    let mut section = "";

    for (idx, tool) in app
        .pref_editor_state
        .individual_tool_list
        .iter()
        .enumerate()
    {
        let is_llm = crate::app::is_llm_tool(tool);
        let current_section = if is_llm {
            "LLM Specific"
        } else {
            "Regular Programs"
        };

        if current_section != section {
            lines.push(Line::from(vec![Span::styled(
                format!("--- {} ---", current_section),
                Style::default().fg(Color::Blue),
            )]));
            section = current_section;
        }

        let enabled = *mgr
            .global_prefs
            .individual_tools
            .get(tool)
            .unwrap_or(&false);
        let check = if enabled { "[x]" } else { "[ ]" };
        let style = if is_focused && idx == app.pref_editor_state.selected_tool {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else if enabled {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        lines.push(Line::from(vec![Span::styled(
            format!("{} {}", check, tool),
            style,
        )]));
    }

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_general_panel(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let is_focused = app.pref_editor_state.focus == PrefEditorFocus::GeneralSettings;
    let block = Block::default()
        .borders(Borders::ALL)
        .title("General Settings")
        .border_style(if is_focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        });

    let mut lines = Vec::new();
    let mgr = &app.paths.preferences;
    let general_prefs = &mgr.global_prefs.general;

    let settings = [
        (
            "Auto-Accept Tools",
            general_prefs.auto_accept_tools.unwrap_or(true),
        ),
        (
            "Enable Logging",
            general_prefs.enable_logging.unwrap_or(true),
        ),
        (
            "Sandboxed Mode",
            general_prefs.sandboxed_mode.unwrap_or(true),
        ),
    ];

    for (idx, (name, val)) in settings.iter().enumerate() {
        let check = if *val { "[x]" } else { "[ ]" };
        let style = if is_focused && idx == app.pref_editor_state.selected_general {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else if *val {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        lines.push(Line::from(vec![Span::styled(
            format!("{} {}", check, name),
            style,
        )]));
    }

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_mcp_servers(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(f.area());

    render_tabs(f, app, chunks[0]);

    let header = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            "Global MCP Servers: ",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw("Define once, sync to all agents (Claude, Gemini, OpenCode)"),
    ])])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("MooAgent MCP Config"),
    );
    f.render_widget(header, chunks[1]);

    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(chunks[2]);

    let servers: Vec<Line> = if app.mcp_editor_state.server_list.is_empty() {
        vec![Line::from(Span::styled(
            "  No MCP servers configured",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        app.mcp_editor_state
            .server_list
            .iter()
            .enumerate()
            .map(|(idx, server)| {
                let is_disabled = app
                    .paths
                    .preferences
                    .project_prefs
                    .as_ref()
                    .map(|p| p.disabled_mcp_servers.contains(server))
                    .unwrap_or(false);

                let (style, text) = if idx == app.mcp_editor_state.selected_server_idx {
                    let s = Style::default().fg(Color::Black).bg(Color::Cyan);
                    if is_disabled {
                        (s, format!("  {} (Disabled in Project)", server))
                    } else {
                        (s, format!("  {}", server))
                    }
                } else if is_disabled {
                    (
                        Style::default().fg(Color::Red),
                        format!("  {} (Disabled)", server),
                    )
                } else {
                    (Style::default().fg(Color::White), format!("  {}", server))
                };
                Line::from(vec![Span::styled(text, style)])
            })
            .collect()
    };

    let servers_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title("MCP Servers [j/k]");
    f.render_widget(Paragraph::new(servers).block(servers_block), main_chunks[0]);

    let mut details = Vec::new();
    if !app.mcp_editor_state.server_list.is_empty() {
        let server_name =
            &app.mcp_editor_state.server_list[app.mcp_editor_state.selected_server_idx];

        let is_disabled = app
            .paths
            .preferences
            .project_prefs
            .as_ref()
            .map(|p| p.disabled_mcp_servers.contains(server_name))
            .unwrap_or(false);

        if let Some(config) = app
            .paths
            .preferences
            .global_prefs
            .mcp_servers
            .get(server_name)
        {
            details.push(Line::from(vec![
                Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(server_name),
            ]));

            if is_disabled {
                details.push(Line::from(""));
                details.push(Line::from(vec![Span::styled(
                    "⚠️  Disabled in current project",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )]));
            }

            details.push(Line::from(""));

            match config {
                crate::preferences::McpServerConfig::Stdio {
                    command,
                    args,
                    env,
                    disabled_tools,
                    auto_allow,
                } => {
                    details.push(Line::from(vec![
                        Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled("local (stdio)", Style::default().fg(Color::Green)),
                    ]));
                    details.push(Line::from(""));

                    details.push(Line::from(vec![
                        Span::styled("Command: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(command),
                    ]));
                    details.push(Line::from(""));

                    if !args.is_empty() {
                        details.push(Line::from(vec![
                            Span::styled("Args: ", Style::default().add_modifier(Modifier::BOLD)),
                            Span::raw(args.join(" ")),
                        ]));
                        details.push(Line::from(""));
                    }

                    if !env.is_empty() {
                        details.push(Line::from(vec![Span::styled(
                            "Environment:",
                            Style::default().add_modifier(Modifier::BOLD),
                        )]));
                        for (k, v) in env {
                            details.push(Line::from(format!("  {}={}", k, v)));
                        }
                        details.push(Line::from(""));
                    }

                    if *auto_allow {
                        details.push(Line::from(vec![Span::styled(
                            "Auto-allow tools: Yes",
                            Style::default().fg(Color::Yellow),
                        )]));
                        details.push(Line::from(""));
                    }

                    if !disabled_tools.is_empty() {
                        details.push(Line::from(vec![Span::styled(
                            "Disabled Tools:",
                            Style::default().add_modifier(Modifier::BOLD),
                        )]));
                        for tool in disabled_tools {
                            details.push(Line::from(format!("  - {}", tool)));
                        }
                        details.push(Line::from(""));
                    }
                }
                crate::preferences::McpServerConfig::Sse {
                    url,
                    auth,
                    disabled_tools,
                    auto_allow,
                } => {
                    details.push(Line::from(vec![
                        Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled("remote (SSE)", Style::default().fg(Color::Blue)),
                    ]));
                    details.push(Line::from(""));

                    details.push(Line::from(vec![
                        Span::styled("URL: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(url),
                    ]));

                    format_auth_details(&mut details, auth);
                    format_oauth_status(&mut details, app, url, auth);

                    if *auto_allow {
                        details.push(Line::from(vec![Span::styled(
                            "Auto-allow tools: Yes",
                            Style::default().fg(Color::Yellow),
                        )]));
                        details.push(Line::from(""));
                    }

                    if !disabled_tools.is_empty() {
                        details.push(Line::from(vec![Span::styled(
                            "Disabled Tools:",
                            Style::default().add_modifier(Modifier::BOLD),
                        )]));
                        for tool in disabled_tools {
                            details.push(Line::from(format!("  - {}", tool)));
                        }
                        details.push(Line::from(""));
                    }
                }
                crate::preferences::McpServerConfig::Http {
                    http_url,
                    auth,
                    disabled_tools,
                    auto_allow,
                } => {
                    details.push(Line::from(vec![
                        Span::styled("Type: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::styled("remote (HTTP)", Style::default().fg(Color::Cyan)),
                    ]));
                    details.push(Line::from(""));

                    details.push(Line::from(vec![
                        Span::styled("URL: ", Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(http_url),
                    ]));

                    format_auth_details(&mut details, auth);
                    format_oauth_status(&mut details, app, http_url, auth);

                    if *auto_allow {
                        details.push(Line::from(vec![Span::styled(
                            "Auto-allow tools: Yes",
                            Style::default().fg(Color::Yellow),
                        )]));
                        details.push(Line::from(""));
                    }

                    if !disabled_tools.is_empty() {
                        details.push(Line::from(vec![Span::styled(
                            "Disabled Tools:",
                            Style::default().add_modifier(Modifier::BOLD),
                        )]));
                        for tool in disabled_tools {
                            details.push(Line::from(format!("  - {}", tool)));
                        }
                        details.push(Line::from(""));
                    }
                }
            }

            details.push(Line::from(""));
            details.push(Line::from(vec![Span::styled(
                "Syncs to: Claude, Gemini, OpenCode",
                Style::default().fg(Color::DarkGray),
            )]));
        }
    } else {
        details.push(Line::from(""));
        details.push(Line::from("No MCP servers configured."));
        details.push(Line::from(""));
        details.push(Line::from(vec![Span::styled(
            "Press [a] to add a new server, or [m] for default servers.",
            Style::default().fg(Color::DarkGray),
        )]));
    }

    let details_block = Block::default().borders(Borders::ALL).title("Details");
    f.render_widget(Paragraph::new(details).block(details_block), main_chunks[1]);

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

    let mut hint_spans = vec![
        Span::styled("[j/k]", Style::default().fg(Color::Cyan)),
        Span::raw(" Nav | "),
        Span::styled("[Space]", Style::default().fg(Color::Cyan)),
        Span::raw(" Toggle | "),
        Span::styled("[a]", Style::default().fg(Color::Cyan)),
        Span::raw(" Add | "),
        Span::styled("[e]", Style::default().fg(Color::Cyan)),
        Span::raw(" Edit | "),
        Span::styled("[d]", Style::default().fg(Color::Cyan)),
        Span::raw(" Del | "),
    ];

    if app.mcp_requires_oauth() {
        hint_spans.push(Span::styled("[o]", Style::default().fg(Color::Yellow)));
        hint_spans.push(Span::raw(" OAuth | "));
    }

    hint_spans.extend(vec![
        Span::styled("[m]", Style::default().fg(Color::Cyan)),
        Span::raw(" Magic | "),
        Span::styled("[s]", Style::default().fg(Color::Cyan)),
        Span::raw(" Sync | "),
        Span::styled("[q]", Style::default().fg(Color::Cyan)),
        Span::raw(" Quit"),
    ]);

    let hints = Line::from(hint_spans);
    f.render_widget(Paragraph::new(hints), chunks[4]);
}

fn render_mcp_edit_dialog(f: &mut Frame, app: &App) {
    let area = f.area();
    let is_remote = app.mcp_editor_state.is_remote_server();
    let auth_type = app.mcp_editor_state.editing_auth_type;

    let height = if is_remote {
        match auth_type {
            McpAuthType::None => 18,
            McpAuthType::Bearer => 21,
            McpAuthType::OAuth => 30,
        }
    } else {
        18
    };

    let width = 80;
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;

    let dialog_area = Rect {
        x,
        y,
        width,
        height,
    };

    f.render_widget(Clear, dialog_area);

    let title = if app.mcp_editor_state.is_new {
        "Add MCP Server"
    } else {
        "Edit MCP Server"
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(Style::default().bg(Color::Black));

    f.render_widget(block, dialog_area);

    let draw_input = |f: &mut Frame,
                      title: &str,
                      content: &str,
                      focus: McpFieldFocus,
                      target: McpFieldFocus,
                      area: Rect| {
        let style = if focus == target {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(style);

        let text = if focus == target {
            format!("{}_", content)
        } else {
            content.to_string()
        };

        f.render_widget(Paragraph::new(text).block(block), area);
    };

    let draw_selector = |f: &mut Frame,
                         title: &str,
                         options: &[&str],
                         selected: usize,
                         focus: McpFieldFocus,
                         target: McpFieldFocus,
                         area: Rect| {
        let style = if focus == target {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(style);

        let display: Vec<Span> = options
            .iter()
            .enumerate()
            .flat_map(|(i, opt)| {
                let sep = if i > 0 { " | " } else { "" };
                let opt_style = if i == selected {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                vec![Span::raw(sep), Span::styled(*opt, opt_style)]
            })
            .collect();

        f.render_widget(Paragraph::new(Line::from(display)).block(block), area);
    };

    if is_remote {
        let mut constraints = vec![
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
        ];

        match auth_type {
            McpAuthType::None => {
                constraints.push(Constraint::Min(3));
            }
            McpAuthType::Bearer => {
                constraints.push(Constraint::Length(3));
                constraints.push(Constraint::Min(3));
            }
            McpAuthType::OAuth => {
                constraints.push(Constraint::Length(3));
                constraints.push(Constraint::Length(3));
                constraints.push(Constraint::Length(3));
                constraints.push(Constraint::Length(3));
                constraints.push(Constraint::Min(3));
            }
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(constraints)
            .split(dialog_area);

        draw_input(
            f,
            "Name (Unique ID)",
            &app.mcp_editor_state.editing_name,
            app.mcp_editor_state.focus,
            McpFieldFocus::Name,
            chunks[0],
        );
        draw_input(
            f,
            "URL (SSE/HTTP endpoint)",
            &app.mcp_editor_state.editing_command,
            app.mcp_editor_state.focus,
            McpFieldFocus::Command,
            chunks[1],
        );

        let auth_idx = match auth_type {
            McpAuthType::None => 0,
            McpAuthType::Bearer => 1,
            McpAuthType::OAuth => 2,
        };
        draw_selector(
            f,
            "Auth Type [Space/h/l to cycle]",
            &["None", "Bearer", "OAuth"],
            auth_idx,
            app.mcp_editor_state.focus,
            McpFieldFocus::AuthType,
            chunks[2],
        );

        let help_idx = match auth_type {
            McpAuthType::None => 3,
            McpAuthType::Bearer => {
                draw_input(
                    f,
                    "Bearer Token",
                    &app.mcp_editor_state.editing_bearer_token,
                    app.mcp_editor_state.focus,
                    McpFieldFocus::BearerToken,
                    chunks[3],
                );
                4
            }
            McpAuthType::OAuth => {
                draw_input(
                    f,
                    "Client ID (required)",
                    &app.mcp_editor_state.editing_oauth_client_id,
                    app.mcp_editor_state.focus,
                    McpFieldFocus::OAuthClientId,
                    chunks[3],
                );
                draw_input(
                    f,
                    "Client Secret (optional)",
                    &app.mcp_editor_state.editing_oauth_client_secret,
                    app.mcp_editor_state.focus,
                    McpFieldFocus::OAuthClientSecret,
                    chunks[4],
                );
                draw_input(
                    f,
                    "Scopes (space separated, optional)",
                    &app.mcp_editor_state.editing_oauth_scopes,
                    app.mcp_editor_state.focus,
                    McpFieldFocus::OAuthScopes,
                    chunks[5],
                );
                draw_input(
                    f,
                    "Auth Server URL (optional, auto-discovered)",
                    &app.mcp_editor_state.editing_oauth_auth_server_url,
                    app.mcp_editor_state.focus,
                    McpFieldFocus::OAuthAuthServerUrl,
                    chunks[6],
                );
                7
            }
        };

        let help = vec![
            Line::from(vec![
                Span::styled("[Tab]", Style::default().fg(Color::Cyan)),
                Span::raw(" Next | "),
                Span::styled("[Enter]", Style::default().fg(Color::Cyan)),
                Span::raw(" Save | "),
                Span::styled("[Esc]", Style::default().fg(Color::Cyan)),
                Span::raw(" Cancel"),
            ]),
            Line::from(vec![
                Span::styled("OAuth: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("After saving, press 'o' to authenticate"),
            ]),
        ];
        f.render_widget(Paragraph::new(help), chunks[help_idx]);
    } else {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(3),
            ])
            .split(dialog_area);

        draw_input(
            f,
            "Name (Unique ID)",
            &app.mcp_editor_state.editing_name,
            app.mcp_editor_state.focus,
            McpFieldFocus::Name,
            chunks[0],
        );
        draw_input(
            f,
            "Command",
            &app.mcp_editor_state.editing_command,
            app.mcp_editor_state.focus,
            McpFieldFocus::Command,
            chunks[1],
        );
        draw_input(
            f,
            "Args (space separated)",
            &app.mcp_editor_state.editing_args,
            app.mcp_editor_state.focus,
            McpFieldFocus::Args,
            chunks[2],
        );
        draw_input(
            f,
            "Env (KEY=VAL,KEY=VAL)",
            &app.mcp_editor_state.editing_env,
            app.mcp_editor_state.focus,
            McpFieldFocus::Env,
            chunks[3],
        );

        let help = vec![
            Line::from(vec![
                Span::styled("[Tab]", Style::default().fg(Color::Cyan)),
                Span::raw(" Next | "),
                Span::styled("[Enter]", Style::default().fg(Color::Cyan)),
                Span::raw(" Save | "),
                Span::styled("[Esc]", Style::default().fg(Color::Cyan)),
                Span::raw(" Cancel"),
            ]),
            Line::from(vec![
                Span::styled("Tip: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw("Use http(s):// URL for remote SSE servers"),
            ]),
        ];
        f.render_widget(Paragraph::new(help), chunks[4]);
    }
}

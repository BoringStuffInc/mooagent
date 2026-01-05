mod app;
mod config;
mod ui;

use crate::app::{App, AppMode};
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use notify::{RecursiveMode, Watcher};
use ratatui::{Terminal, backend::CrosstermBackend};
use simplelog::*;
use std::{fs::File, io, path::Path, sync::mpsc};

fn main() -> Result<()> {
    let log_dir = directories::ProjectDirs::from("", "", "mooagent")
        .map(|p| p.data_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir);
    
    std::fs::create_dir_all(&log_dir)?;
    let log_file = log_dir.join("mooagent.log");
    
    let _ = WriteLogger::init(
        LevelFilter::Info,
        Config::default(),
        File::create(log_file)?,
    );
    
    log::info!("Starting MooAgent");
    
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx, rx) = mpsc::channel();

    let mut app = App::new(Some(rx))?;

    let mut watcher = notify::recommended_watcher(move |res| match res {
        Ok(_) => {
            let _ = tx.send(());
        }
        Err(e) => eprintln!("watch error: {:?}", e),
    })?;

    if app.paths.project_agents.exists() {
        watcher.watch(&app.paths.project_agents, RecursiveMode::NonRecursive)?;
    }
    
    if app.paths.config_file.exists() {
        watcher.watch(&app.paths.config_file, RecursiveMode::NonRecursive)?;
    }
    
    if app.paths.global_rules_primary.exists() {
        watcher.watch(&app.paths.global_rules_primary, RecursiveMode::NonRecursive)?;
    }
    
    for agent_def in &app.paths.agent_configs {
        if let Some(global_file) = &agent_def.global_file
            && global_file.exists()
        {
            let _ = watcher.watch(global_file, RecursiveMode::NonRecursive);
        }
    }

    let res = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{:?}", err);
    }

    Ok(())
}

fn run_app<B: ratatui::backend::Backend + std::io::Write>(
    terminal: &mut Terminal<B>,

    app: &mut App,
) -> Result<()>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    loop {
        app.tick();

        terminal.draw(|f| ui::render(f, app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => {
                                match app.mode {
                                    AppMode::Help | AppMode::ViewDiff | AppMode::ViewBackups => {
                                        match key.code {
                                            KeyCode::Char('q') | KeyCode::Esc => {
                                                app.mode = AppMode::Normal;
                                            }
                                            KeyCode::Char('j') | KeyCode::Down => {
                                                app.scroll_detail_down();
                                            }
                                            KeyCode::Char('k') | KeyCode::Up => {
                                                app.scroll_detail_up();
                                            }
                                            _ => {}
                                        }
                                    }
                                    AppMode::Search => {
                    
                            match key.code {
                                KeyCode::Esc => {
                                    app.clear_search();
                                    app.mode = AppMode::Normal;
                                }
                                KeyCode::Enter => {
                                    app.mode = AppMode::Normal;
                                }
                                KeyCode::Backspace => {
                                    app.backspace_search();
                                }
                                KeyCode::Char(c) => {
                                    app.add_search_char(c);
                                }
                                _ => {}
                            }
                        }
                        AppMode::ConfirmSync => {
                            match key.code {
                                KeyCode::Char('y') | KeyCode::Char('Y') => {
                                    let _ = app.sync_selected();
                                    app.mode = AppMode::Normal;
                                }
                                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                    app.mode = AppMode::Normal;
                                }
                                _ => {}
                            }
                        }
                        AppMode::ConfirmSyncAll => {
                            match key.code {
                                KeyCode::Char('y') | KeyCode::Char('Y') => {
                                    let _ = app.sync();
                                    app.mode = AppMode::Normal;
                                }
                                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                                    app.mode = AppMode::Normal;
                                }
                                _ => {}
                            }
                        }
                        AppMode::Normal => {
                            if app.show_error_log {
                                match key.code {
                                    KeyCode::Char('v') | KeyCode::Esc | KeyCode::Char('q') => {
                                        app.toggle_error_log();
                                    }
                                    _ => {}
                                }
                            } else {
                                match key.code {
                                    KeyCode::Char('q') => return Ok(()),

                                    KeyCode::Char('?') => {
                                        app.detail_scroll = 0;
                                        app.mode = AppMode::Help;
                                    }

                                    KeyCode::Char('s') => {
                                        app.mode = AppMode::ConfirmSyncAll;
                                    }

                                    KeyCode::Enter => {
                                        app.mode = AppMode::ConfirmSync;
                                    }

                                    KeyCode::Char('d') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        app.detail_scroll = 0;
                                        app.mode = AppMode::ViewDiff;
                                    }

                                    KeyCode::Char('b') => {
                                        app.detail_scroll = 0;
                                        app.mode = AppMode::ViewBackups;
                                    }

                                    KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        suspend_and_run_editor(terminal, &app.paths.project_agents)?;
                                        app.refresh();
                                    }

                                    KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        let primary_path = app.paths.global_rules_primary.clone();
                                        suspend_and_run_editor(terminal, &primary_path)?;
                                        let _ = app.sync_global_rules();
                                    }

                                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        suspend_and_run_editor(terminal, &app.paths.config_file)?;
                                        app.refresh();
                                    }

                                    KeyCode::Char('g') => {
                                        if app.pending_g {
                                            app.scroll_to_top();
                                            app.pending_g = false;
                                        } else {
                                            app.pending_g = true;
                                        }
                                    }

                                    KeyCode::Char('G') => {
                                        app.scroll_to_bottom();
                                    }

                                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        match app.focus {
                                            app::Focus::Agents => {
                                                for _ in 0..5 { app.prev_agent(); }
                                            }
                                            app::Focus::Global => {
                                                app.global_scroll = app.global_scroll.saturating_sub(10);
                                            }
                                            app::Focus::Project => {
                                                app.scroll_project_page_up();
                                            }
                                        }
                                    }

                                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        match app.focus {
                                            app::Focus::Agents => {
                                                for _ in 0..5 { app.next_agent(); }
                                            }
                                            app::Focus::Global => {
                                                app.global_scroll += 10;
                                                // Simplified, refresh will clamp if needed or we could do it here
                                            }
                                            app::Focus::Project => {
                                                app.scroll_project_page_down();
                                            }
                                        }
                                    }

                                    KeyCode::Char('a') => {
                                        app.toggle_auto_sync();
                                    }

                                    KeyCode::Char('/') => {
                                        app.mode = AppMode::Search;
                                    }

                                    KeyCode::Char('v') => {
                                        app.toggle_error_log();
                                    }

                                    KeyCode::Tab => {
                                        app.next_focus();
                                    }

                                    KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                        app.next_focus();
                                    }

                                    KeyCode::Char('j') | KeyCode::Down => match app.focus {
                                        app::Focus::Agents => app.next_agent(),
                                        app::Focus::Global => app.scroll_global_down(),
                                        app::Focus::Project => app.scroll_project_down(),
                                    },

                                    KeyCode::Char('k') | KeyCode::Up => match app.focus {
                                        app::Focus::Agents => app.prev_agent(),
                                        app::Focus::Global => app.scroll_global_up(),
                                        app::Focus::Project => app.scroll_project_up(),
                                    },

                                    KeyCode::Char('h') | KeyCode::Left => {
                                        app.focus_left();
                                    }

                                    KeyCode::Char('l') | KeyCode::Right => {
                                        app.focus_right();
                                    }

                                    KeyCode::PageUp => {
                                        app.scroll_project_page_up();
                                    }

                                    KeyCode::PageDown => {
                                        app.scroll_project_page_down();
                                    }

                                    KeyCode::Home => {
                                        app.scroll_project_home();
                                    }

                                    KeyCode::End => {
                                        app.scroll_project_end();
                                    }

                                    KeyCode::Esc => {
                                        app.status_message = None;
                                    }

                                    _ => {
                                        app.pending_g = false;
                                    }
                                }
                            }
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    if app.mode == AppMode::Normal && !app.show_error_log {
                        match mouse.kind {
                            MouseEventKind::ScrollDown => match app.focus {
                                app::Focus::Agents => app.next_agent(),
                                app::Focus::Global => app.scroll_global_down(),
                                app::Focus::Project => app.scroll_project_down(),
                            },
                            MouseEventKind::ScrollUp => match app.focus {
                                app::Focus::Agents => app.prev_agent(),
                                app::Focus::Global => app.scroll_global_up(),
                                app::Focus::Project => app.scroll_project_up(),
                            },
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn suspend_and_run_editor<B: ratatui::backend::Backend + std::io::Write>(
    terminal: &mut Terminal<B>,

    path: &Path,
) -> Result<()>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    disable_raw_mode()?;

    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;

    terminal.show_cursor()?;

    let editor_cmd = std::env::var("EDITOR").unwrap_or_else(|_| "nvim".to_string());

    if std::process::Command::new(&editor_cmd)
        .arg(path)
        .status()
        .is_err()
    {
        let fallbacks = ["nvim", "vim", "vi", "nano"];

        for fallback in fallbacks {
            if fallback == editor_cmd {
                continue;
            }

            if std::process::Command::new(fallback)
                .arg(path)
                .status()
                .is_ok()
            {
                break;
            }
        }
    }

    enable_raw_mode()?;

    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;

    terminal.clear()?;

    Ok(())
}

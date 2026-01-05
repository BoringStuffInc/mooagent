mod app;
mod config;
mod ui;

use crate::app::{App, AppMode};
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
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

        if event::poll(std::time::Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            match app.mode {
                AppMode::Help | AppMode::ViewDiff | AppMode::ViewBackups => {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.mode = AppMode::Normal;
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
                                app.mode = AppMode::Help;
                            }

                            KeyCode::Char('s') => {
                                app.mode = AppMode::ConfirmSyncAll;
                            }

                            KeyCode::Enter => {
                                app.mode = AppMode::ConfirmSync;
                            }

                            KeyCode::Char('d') => {
                                app.mode = AppMode::ViewDiff;
                            }

                            KeyCode::Char('b') => {
                                app.mode = AppMode::ViewBackups;
                            }

                            KeyCode::Char('e') => {
                                suspend_and_run_editor(terminal, &app.paths.project_agents)?;
                                app.refresh();
                            }

                            KeyCode::Char('g') | KeyCode::Char('G') => {
                                let primary_path = app.paths.global_rules_primary.clone();
                                suspend_and_run_editor(terminal, &primary_path)?;
                                let _ = app.sync_global_rules();
                            }

                            KeyCode::Char('c') => {
                                suspend_and_run_editor(terminal, &app.paths.config_file)?;
                                app.refresh();
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

                            KeyCode::Char('j') | KeyCode::Down => {
                                app.next_agent();
                            }

                            KeyCode::Char('k') | KeyCode::Up => {
                                app.prev_agent();
                            }

                            KeyCode::Char('h') | KeyCode::Char('H') => {
                                app.scroll_project_up();
                            }

                            KeyCode::Char('l') | KeyCode::Char('L') => {
                                app.scroll_project_down();
                            }

                            _ => {}
                        }
                    }
                }
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

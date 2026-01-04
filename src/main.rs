mod app;
mod config;
mod ui;

use crate::app::App;
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use notify::{RecursiveMode, Watcher};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{io, path::Path, sync::mpsc};

fn main() -> Result<()> {
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

    if app.paths.global_rules.exists() {
        watcher.watch(&app.paths.global_rules, RecursiveMode::NonRecursive)?;
    }
    if app.paths.project_agents.exists() {
        watcher.watch(&app.paths.project_agents, RecursiveMode::NonRecursive)?;
    }
    if app.paths.config_file.exists() {
        watcher.watch(&app.paths.config_file, RecursiveMode::NonRecursive)?;
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
            match key.code {
                KeyCode::Char('q') => return Ok(()),

                KeyCode::Char('s') => {
                    let _ = app.sync();
                }

                KeyCode::Char('e') => {
                    suspend_and_run_editor(terminal, &app.paths.project_agents)?;

                    app.refresh();
                }

                KeyCode::Char('g') => {
                    suspend_and_run_editor(terminal, &app.paths.global_rules)?;

                    app.refresh();
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

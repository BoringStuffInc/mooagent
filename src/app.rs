use crate::config::{AgentInfo, ConfigPaths};
use anyhow::Result;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Normal,
    Help,
    ConfirmSync,
    ConfirmSyncAll,
    ViewDiff,
    ViewBackups,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Agents,
    Global,
    Project,
}

pub struct App {
    pub paths: ConfigPaths,
    pub agents: Vec<AgentInfo>,
    pub project_content: String,
    pub status_message: Option<(String, Instant)>,
    pub event_rx: Option<Receiver<()>>,
    pub selected_agent: usize,
    pub project_scroll: usize,
    pub global_scroll: usize,
    pub detail_scroll: usize,
    pub mode: AppMode,
    pub focus: Focus,
    pub pending_g: bool,
    pub status_log: Vec<(String, Instant)>,
    pub search_query: String,
    pub status_message_timeout: u64,
    pub auto_sync: bool,
    pub filtered_agents: Vec<usize>,
    pub show_error_log: bool,
}

impl App {
    pub fn new(event_rx: Option<Receiver<()>>) -> Result<Self> {
        let paths = ConfigPaths::new()?;
        paths.ensure_files_exist()?;
        let project_content = paths.read_project_content();
        let agents = paths.get_agents();

        let filtered_agents: Vec<usize> = (0..agents.len()).collect();
        
        Ok(Self {
            paths,
            agents,
            project_content,
            status_message: None,
            event_rx,
            selected_agent: 0,
            project_scroll: 0,
            global_scroll: 0,
            detail_scroll: 0,
            mode: AppMode::Normal,
            focus: Focus::Agents,
            pending_g: false,
            status_log: Vec::new(),
            search_query: String::new(),
            status_message_timeout: 5,
            auto_sync: false,
            filtered_agents,
            show_error_log: false,
        })
    }

    pub fn refresh(&mut self) {
        self.project_content = self.paths.read_project_content();
        self.agents = self.paths.get_agents();
        
        self.update_filter();
        
        if self.selected_agent >= self.agents.len() && !self.agents.is_empty() {
            self.selected_agent = self.agents.len() - 1;
        }
        
        if self.auto_sync {
            let _ = self.sync();
        }
    }

    pub fn sync(&mut self) -> Result<()> {
        match self.paths.sync() {
            Ok(msg) => {
                self.set_status(msg);
                self.refresh();
                Ok(())
            }
            Err(e) => {
                self.set_status(format!("Error: {}", e));
                Err(e)
            }
        }
    }

    pub fn set_status(&mut self, msg: String) {
        log::info!("{}", msg);
        self.status_message = Some((msg.clone(), Instant::now()));
        self.status_log.push((msg, Instant::now()));
        
        if self.status_log.len() > 100 {
            self.status_log.drain(0..1);
        }
    }

    pub fn tick(&mut self) {
        if let Some(rx) = &self.event_rx {
            let mut changed = false;
            while rx.try_recv().is_ok() {
                changed = true;
            }
            if changed {
                self.refresh();
            }
        }

        if let Some((_, time)) = self.status_message
            && time.elapsed() > Duration::from_secs(self.status_message_timeout)
        {
            self.status_message = None;
        }
    }
    
    pub fn sync_selected(&mut self) -> Result<()> {
        if self.agents.is_empty() {
            self.set_status("No agents to sync".to_string());
            return Ok(());
        }
        
        match self.paths.sync_agent(self.selected_agent) {
            Ok(msg) => {
                self.set_status(msg);
                self.refresh();
                Ok(())
            }
            Err(e) => {
                self.set_status(format!("Error: {}", e));
                Err(e)
            }
        }
    }
    
    pub fn next_agent(&mut self) {
        if !self.filtered_agents.is_empty() {
            let current_pos = self.filtered_agents
                .iter()
                .position(|&i| i == self.selected_agent)
                .unwrap_or(0);
            let next_pos = (current_pos + 1) % self.filtered_agents.len();
            self.selected_agent = self.filtered_agents[next_pos];
        }
    }
    
    pub fn prev_agent(&mut self) {
        if !self.filtered_agents.is_empty() {
            let current_pos = self.filtered_agents
                .iter()
                .position(|&i| i == self.selected_agent)
                .unwrap_or(0);
            let prev_pos = if current_pos == 0 {
                self.filtered_agents.len() - 1
            } else {
                current_pos - 1
            };
            self.selected_agent = self.filtered_agents[prev_pos];
        }
    }
    
    pub fn scroll_project_down(&mut self) {
        let line_count = self.project_content.lines().count();
        if self.project_scroll < line_count.saturating_sub(1) {
            self.project_scroll += 1;
        }
    }
    
    pub fn scroll_project_up(&mut self) {
        if self.project_scroll > 0 {
            self.project_scroll -= 1;
        }
    }

    pub fn scroll_project_page_down(&mut self) {
        let line_count = self.project_content.lines().count();
        self.project_scroll = (self.project_scroll + 10).min(line_count.saturating_sub(1));
    }

    pub fn scroll_project_page_up(&mut self) {
        self.project_scroll = self.project_scroll.saturating_sub(10);
    }

    pub fn scroll_project_home(&mut self) {
        self.project_scroll = 0;
    }

    pub fn scroll_project_end(&mut self) {
        let line_count = self.project_content.lines().count();
        self.project_scroll = line_count.saturating_sub(1);
    }

    pub fn scroll_global_down(&mut self) {
        let global_content = if self.paths.global_rules_primary.exists() {
            std::fs::read_to_string(&self.paths.global_rules_primary).unwrap_or_default()
        } else {
            String::new()
        };
        let line_count = global_content.lines().count();
        if self.global_scroll < line_count.saturating_sub(1) {
            self.global_scroll += 1;
        }
    }

    pub fn scroll_global_up(&mut self) {
        if self.global_scroll > 0 {
            self.global_scroll -= 1;
        }
    }

    pub fn scroll_detail_down(&mut self) {
        self.detail_scroll += 1;
    }

    pub fn scroll_detail_up(&mut self) {
        if self.detail_scroll > 0 {
            self.detail_scroll -= 1;
        }
    }

    pub fn scroll_to_top(&mut self) {
        match self.focus {
            Focus::Agents => self.selected_agent = 0,
            Focus::Global => self.global_scroll = 0,
            Focus::Project => self.project_scroll = 0,
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        match self.focus {
            Focus::Agents => {
                if !self.filtered_agents.is_empty() {
                    self.selected_agent = self.filtered_agents[self.filtered_agents.len() - 1];
                }
            }
            Focus::Global => {
                let content = if self.paths.global_rules_primary.exists() {
                    std::fs::read_to_string(&self.paths.global_rules_primary).unwrap_or_default()
                } else {
                    String::new()
                };
                self.global_scroll = content.lines().count().saturating_sub(1);
            }
            Focus::Project => {
                self.project_scroll = self.project_content.lines().count().saturating_sub(1);
            }
        }
    }
    
    pub fn toggle_auto_sync(&mut self) {
        self.auto_sync = !self.auto_sync;
        let status = if self.auto_sync { "enabled" } else { "disabled" };
        self.set_status(format!("Auto-sync {}", status));
    }
    
    pub fn sync_global_rules(&mut self) -> Result<()> {
        match self.paths.sync_global_rules() {
            Ok(()) => {
                self.set_status("Global rules synced to all agents".to_string());
                self.refresh();
                Ok(())
            }
            Err(e) => {
                self.set_status(format!("Error syncing global rules: {}", e));
                Err(e)
            }
        }
    }
    
    pub fn update_filter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_agents = (0..self.agents.len()).collect();
        } else {
            self.filtered_agents = self.agents
                .iter()
                .enumerate()
                .filter(|(_, agent)| {
                    agent.name.to_lowercase().contains(&self.search_query.to_lowercase())
                        || agent.target_path.to_string_lossy().to_lowercase().contains(&self.search_query.to_lowercase())
                })
                .map(|(i, _)| i)
                .collect();
        }

        if !self.filtered_agents.is_empty() && !self.filtered_agents.contains(&self.selected_agent) {
            self.selected_agent = self.filtered_agents[0];
        }
    }
    
    pub fn add_search_char(&mut self, c: char) {
        self.search_query.push(c);
        self.update_filter();
    }
    
    pub fn backspace_search(&mut self) {
        self.search_query.pop();
        self.update_filter();
    }
    
    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.update_filter();
    }
    
    pub fn toggle_error_log(&mut self) {
        self.show_error_log = !self.show_error_log;
    }

    pub fn next_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Agents => Focus::Global,
            Focus::Global => Focus::Project,
            Focus::Project => Focus::Agents,
        };
    }

    pub fn focus_left(&mut self) {
        self.focus = match self.focus {
            Focus::Project => Focus::Global,
            _ => self.focus,
        };
    }

    pub fn focus_right(&mut self) {
        self.focus = match self.focus {
            Focus::Global => Focus::Project,
            _ => self.focus,
        };
    }
    
    pub fn get_visible_agents(&self) -> Vec<&AgentInfo> {
        self.filtered_agents
            .iter()
            .filter_map(|&idx| self.agents.get(idx))
            .collect()
    }
}

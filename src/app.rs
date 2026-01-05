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

pub struct App {
    pub paths: ConfigPaths,
    pub agents: Vec<AgentInfo>,
    pub project_content: String,
    pub status_message: Option<(String, Instant)>,
    pub event_rx: Option<Receiver<()>>,
    pub selected_agent: usize,
    pub project_scroll: usize,
    pub mode: AppMode,
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
            mode: AppMode::Normal,
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
        if !self.agents.is_empty() {
            self.selected_agent = (self.selected_agent + 1) % self.agents.len();
        }
    }
    
    pub fn prev_agent(&mut self) {
        if !self.agents.is_empty() {
            if self.selected_agent == 0 {
                self.selected_agent = self.agents.len() - 1;
            } else {
                self.selected_agent -= 1;
            }
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
    
    pub fn get_visible_agents(&self) -> Vec<&AgentInfo> {
        self.filtered_agents
            .iter()
            .filter_map(|&idx| self.agents.get(idx))
            .collect()
    }
}

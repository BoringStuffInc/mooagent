use crate::config::{AgentInfo, ConfigPaths};
use anyhow::Result;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

pub struct App {
    pub paths: ConfigPaths,
    pub agents: Vec<AgentInfo>,
    pub global_content: String,
    pub project_content: String,
    pub status_message: Option<(String, Instant)>,
    pub event_rx: Option<Receiver<()>>,
}

impl App {
    pub fn new(event_rx: Option<Receiver<()>>) -> Result<Self> {
        let paths = ConfigPaths::new()?;
        paths.ensure_files_exist()?;
        let (global_content, project_content) = paths.read_contents();
        let agents = paths.get_agents();

        Ok(Self {
            paths,
            agents,
            global_content,
            project_content,
            status_message: None,
            event_rx,
        })
    }

    pub fn refresh(&mut self) {
        let (global, project) = self.paths.read_contents();
        self.global_content = global;
        self.project_content = project;
        self.agents = self.paths.get_agents();
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
        self.status_message = Some((msg, Instant::now()));
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
            && time.elapsed() > Duration::from_secs(5)
        {
            self.status_message = None;
        }
    }
}

use anyhow::{Context, Result};
use chrono::Local;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub struct ConfigPaths {
    pub global_rules: PathBuf,
    pub project_agents: PathBuf,
    pub config_file: PathBuf,
    pub agent_configs: Vec<AgentDefinition>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum AgentStatus {
    Ok,
    Missing,
    Drift,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncStrategy {
    Symlink,
    Merge,
}

#[derive(Debug, Clone)]
pub struct AgentDefinition {
    pub name: String,
    pub target_path: PathBuf,
    pub strategy: SyncStrategy,
}

#[derive(Debug, Deserialize)]
struct ExternalConfig {
    #[serde(default)]
    agents: Vec<ExternalAgent>,
}

#[derive(Debug, Deserialize)]
struct ExternalAgent {
    name: String,
    path: String,
    strategy: Option<SyncStrategy>,
}

pub struct AgentInfo {
    pub name: String,
    pub target_path: PathBuf,
    pub status: AgentStatus,
    pub strategy: SyncStrategy,
}

impl ConfigPaths {
    pub fn new() -> Result<Self> {
        let project_dirs = ProjectDirs::from("", "", "agent-sync")
            .context("Could not determine config directory")?;
        let global_config_dir = project_dirs.config_dir();

        fs::create_dir_all(global_config_dir)?;

        let cwd = std::env::current_dir()?;
        let config_file = cwd.join(".mooagent.toml");

        let mut agent_configs = Vec::new();

        if config_file.exists() {
            let content = fs::read_to_string(&config_file)?;
            let external: ExternalConfig =
                toml::from_str(&content).context("Failed to parse .mooagent.toml")?;

            for ea in external.agents {
                agent_configs.push(AgentDefinition {
                    name: ea.name,
                    target_path: cwd.join(ea.path),
                    strategy: ea.strategy.unwrap_or(SyncStrategy::Merge),
                });
            }
        }

        // Add defaults if nothing configured or as base
        if agent_configs.is_empty() {
            agent_configs.push(AgentDefinition {
                name: "Claude".to_string(),
                target_path: cwd.join("CLAUDE.md"),
                strategy: SyncStrategy::Merge,
            });
            agent_configs.push(AgentDefinition {
                name: "Gemini".to_string(),
                target_path: cwd.join("GEMINI.md"),
                strategy: SyncStrategy::Merge,
            });
            agent_configs.push(AgentDefinition {
                name: "OpenCode".to_string(),
                target_path: cwd.join(".opencode").join("rules.md"),
                strategy: SyncStrategy::Merge,
            });
        }

        Ok(Self {
            global_rules: global_config_dir.join("USER_RULES.md"),
            project_agents: cwd.join("AGENTS.md"),
            config_file,
            agent_configs,
        })
    }

    pub fn ensure_files_exist(&self) -> Result<()> {
        if !self.global_rules.exists() {
            fs::write(
                &self.global_rules,
                "# Global User Rules\n\nAdd your global coding preferences here.",
            )?;
        }
        if !self.project_agents.exists() {
            fs::write(
                &self.project_agents,
                "# Project Agent Rules\n\nAdd your project-specific instructions here.",
            )?;
        }
        Ok(())
    }

    pub fn read_contents(&self) -> (String, String) {
        let global = fs::read_to_string(&self.global_rules)
            .unwrap_or_else(|_| "Error reading global rules".to_string());
        let project = fs::read_to_string(&self.project_agents)
            .unwrap_or_else(|_| "Error reading project rules".to_string());
        (global, project)
    }

    pub fn get_agents(&self) -> Vec<AgentInfo> {
        let (global_content, project_content) = self.read_contents();
        let merged_content = format!("{}\n\n{}", global_content, project_content);

        self.agent_configs
            .iter()
            .map(|def| AgentInfo {
                name: def.name.clone(),
                target_path: def.target_path.clone(),
                status: get_agent_status(
                    &def.target_path,
                    &self.project_agents,
                    &merged_content,
                    def.strategy,
                ),
                strategy: def.strategy,
            })
            .collect()
    }

    pub fn sync(&self) -> Result<String> {
        self.ensure_files_exist()?;
        let (global_content, project_content) = self.read_contents();
        let merged_content = format!("{}\n\n{}", global_content, project_content);

        let agents = self.get_agents();
        let mut synced_count = 0;

        for agent in agents {
            if agent.status != AgentStatus::Ok {
                if agent.target_path.exists() {
                    let timestamp = Local::now().format("%Y%m%d_%H%M%S");
                    let mut backup = agent.target_path.clone();
                    if let Some(ext) = agent.target_path.extension() {
                        let mut ext_str = ext.to_os_string();
                        ext_str.push(format!(".bak.{}", timestamp));
                        backup.set_extension(ext_str);
                    } else {
                        backup.set_extension(format!("bak.{}", timestamp));
                    }
                    let _ = fs::rename(&agent.target_path, &backup);
                }

                if let Some(parent) = agent.target_path.parent() {
                    fs::create_dir_all(parent)?;
                }

                match agent.strategy {
                    SyncStrategy::Merge => {
                        fs::write(&agent.target_path, &merged_content)?;
                    }
                    SyncStrategy::Symlink => {
                        let target_dir = agent.target_path.parent().unwrap_or(Path::new("."));
                        let relative_source =
                            pathdiff::diff_paths(&self.project_agents, target_dir)
                                .unwrap_or_else(|| self.project_agents.clone());

                        #[cfg(unix)]
                        std::os::unix::fs::symlink(&relative_source, &agent.target_path)?;

                        #[cfg(windows)]
                        std::os::windows::fs::symlink_file(&relative_source, &agent.target_path)?;
                    }
                }
                synced_count += 1;
            }
        }

        if synced_count == 0 {
            Ok("All agents already in sync.".to_string())
        } else {
            Ok(format!("Successfully synced {} agent(s).", synced_count))
        }
    }
}

pub fn get_agent_status(
    target: &Path,
    source: &Path,
    expected_content: &str,
    strategy: SyncStrategy,
) -> AgentStatus {
    if !target.exists() {
        return AgentStatus::Missing;
    }

    match strategy {
        SyncStrategy::Merge => match fs::read_to_string(target) {
            Ok(content) => {
                if content == expected_content {
                    AgentStatus::Ok
                } else {
                    AgentStatus::Drift
                }
            }
            Err(_) => AgentStatus::Drift,
        },
        SyncStrategy::Symlink => match fs::read_link(target) {
            Ok(link_target) => {
                let target_dir = target.parent().unwrap_or(Path::new("."));
                let resolved_link = target_dir.join(link_target);

                let source_canonical = fs::canonicalize(source).ok();
                let target_canonical = fs::canonicalize(resolved_link).ok();

                if source_canonical.is_some() && source_canonical == target_canonical {
                    AgentStatus::Ok
                } else {
                    AgentStatus::Drift
                }
            }
            Err(_) => AgentStatus::Drift,
        },
    }
}

use crate::credentials::CredentialManager;
use crate::preferences::PreferenceManager;
use anyhow::{Context, Result};
use chrono::Local;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub struct ConfigPaths {
    pub project_agents: PathBuf,
    pub config_file: PathBuf,
    pub agent_configs: Vec<AgentDefinition>,
    pub global_rules_primary: PathBuf,
    pub backup_dir: PathBuf,
    pub project_id: String,
    pub preferences: PreferenceManager,
    pub config_dir: PathBuf,
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
    pub global_file: Option<PathBuf>,
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
    global_file: Option<String>,
}

pub struct AgentInfo {
    pub name: String,
    pub target_path: PathBuf,
    pub status: AgentStatus,
    pub strategy: SyncStrategy,
}

impl ConfigPaths {
    pub fn new() -> Result<Self> {
        let project_dirs = ProjectDirs::from("", "", "mooagent")
            .context("Could not determine config directory")?;
        let global_config_dir = project_dirs.config_dir();
        let backup_dir = project_dirs.data_dir().join("backups");

        fs::create_dir_all(global_config_dir)?;
        fs::create_dir_all(&backup_dir)?;

        let cwd = std::env::current_dir()?;
        let config_file = cwd.join(".mooagent.toml");

        let project_id = cwd
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let mut agent_configs = Vec::new();

        if config_file.exists() {
            let content = fs::read_to_string(&config_file)?;
            let external: ExternalConfig =
                toml::from_str(&content).context("Failed to parse .mooagent.toml")?;

            for ea in external.agents {
                let global_file = ea.global_file.map(|p| {
                    let path = PathBuf::from(shellexpand::tilde(&p).to_string());
                    if path.is_absolute() {
                        path
                    } else {
                        cwd.join(path)
                    }
                });

                agent_configs.push(AgentDefinition {
                    name: ea.name,
                    target_path: cwd.join(ea.path),
                    strategy: ea.strategy.unwrap_or(SyncStrategy::Merge),
                    global_file,
                });
            }
        }

        if agent_configs.is_empty() {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

            agent_configs.push(AgentDefinition {
                name: "Claude".to_string(),
                target_path: cwd.join("CLAUDE.md"),
                strategy: SyncStrategy::Merge,
                global_file: Some(home.join(".claude/CLAUDE.md")),
            });
            agent_configs.push(AgentDefinition {
                name: "Gemini".to_string(),
                target_path: cwd.join("GEMINI.md"),
                strategy: SyncStrategy::Merge,
                global_file: Some(home.join(".gemini/GEMINI.md")),
            });
            agent_configs.push(AgentDefinition {
                name: "OpenCode".to_string(),
                target_path: cwd.join(".opencode").join("rules.md"),
                strategy: SyncStrategy::Merge,
                global_file: Some(home.join(".config/opencode/AGENTS.md")),
            });
        }

        let mut preferences = PreferenceManager::new(global_config_dir);
        let _ = preferences.load_global();
        let _ = preferences.load_project(&config_file);

        Ok(Self {
            project_agents: cwd.join("AGENTS.md"),
            config_file,
            agent_configs,
            global_rules_primary: global_config_dir.join("GLOBAL_RULES.md"),
            backup_dir,
            project_id,
            preferences,
            config_dir: global_config_dir.to_path_buf(),
        })
    }

    pub fn ensure_files_exist(&self) -> Result<()> {
        if !self.project_agents.exists() {
            fs::write(
                &self.project_agents,
                "# Project-Specific Agent Instructions\n\nAdd project-specific instructions here.",
            )?;
        }

        if !self.global_rules_primary.exists() {
            fs::write(
                &self.global_rules_primary,
                "# Global User Rules\n\nAdd your global coding preferences here.",
            )?;
        }

        Ok(())
    }

    pub fn sync_global_rules(&self) -> Result<()> {
        log::info!("Syncing global rules to all agent files");

        if !self.global_rules_primary.exists() {
            anyhow::bail!(
                "Primary global rules file does not exist: {}",
                self.global_rules_primary.display()
            );
        }
        let primary_content = fs::read_to_string(&self.global_rules_primary)?;

        for agent_def in &self.agent_configs {
            if let Some(global_file) = &agent_def.global_file {
                let needs_sync = if global_file.exists() {
                    fs::read_to_string(global_file).ok() != Some(primary_content.clone())
                } else {
                    true
                };

                if needs_sync {
                    self.backup_if_needed(global_file)?;
                    fs::write(global_file, &primary_content)?;
                    log::info!("Synced global rules to {}", global_file.display());
                }
            }
        }

        Ok(())
    }

    pub fn check_global_rules_drift(&self) -> Vec<String> {
        let mut drifted = Vec::new();

        let primary_content = if self.global_rules_primary.exists() {
            fs::read_to_string(&self.global_rules_primary).ok()
        } else {
            None
        };

        for agent_def in &self.agent_configs {
            if let Some(global_file) = &agent_def.global_file
                && global_file.exists()
            {
                let agent_content = fs::read_to_string(global_file).ok();
                if agent_content != primary_content {
                    drifted.push(agent_def.name.clone());
                }
            }
        }

        drifted
    }

    pub fn read_project_content(&self) -> String {
        fs::read_to_string(&self.project_agents)
            .unwrap_or_else(|_| "Error reading project rules".to_string())
    }

    pub fn get_merged_content(&self, _agent_def: &AgentDefinition) -> String {
        self.read_project_content()
    }

    pub fn get_agents(&self) -> Vec<AgentInfo> {
        self.agent_configs
            .iter()
            .map(|def| {
                let merged_content = self.get_merged_content(def);
                AgentInfo {
                    name: def.name.clone(),
                    target_path: def.target_path.clone(),
                    status: get_agent_status(
                        &def.target_path,
                        &self.project_agents,
                        &merged_content,
                        def.strategy,
                    ),
                    strategy: def.strategy,
                }
            })
            .collect()
    }

    pub fn sync(&self) -> Result<String> {
        self.ensure_files_exist()?;
        let agents = self.get_agents();
        let mut synced_count = 0;

        for (idx, agent) in agents.iter().enumerate() {
            if agent.status != AgentStatus::Ok {
                let agent_def = &self.agent_configs[idx];
                let merged_content = self.get_merged_content(agent_def);

                self.backup_if_needed(&agent.target_path)?;

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

    pub fn sync_agent(&self, agent_index: usize) -> Result<String> {
        self.ensure_files_exist()?;

        if agent_index >= self.agent_configs.len() {
            return Ok("Invalid agent index".to_string());
        }

        let agent_def = &self.agent_configs[agent_index];
        let merged_content = self.get_merged_content(agent_def);
        let agents = self.get_agents();
        let agent = &agents[agent_index];

        if agent.status == AgentStatus::Ok {
            return Ok(format!("{} already in sync", agent.name));
        }

        self.backup_if_needed(&agent.target_path)?;

        if let Some(parent) = agent.target_path.parent() {
            fs::create_dir_all(parent)?;
        }

        match agent.strategy {
            SyncStrategy::Merge => {
                fs::write(&agent.target_path, &merged_content)?;
            }
            SyncStrategy::Symlink => {
                let target_dir = agent.target_path.parent().unwrap_or(Path::new("."));
                let relative_source = pathdiff::diff_paths(&self.project_agents, target_dir)
                    .unwrap_or_else(|| self.project_agents.clone());

                #[cfg(unix)]
                std::os::unix::fs::symlink(&relative_source, &agent.target_path)?;

                #[cfg(windows)]
                std::os::windows::fs::symlink_file(&relative_source, &agent.target_path)?;
            }
        }

        Ok(format!("Successfully synced {}", agent.name))
    }

    pub fn sync_preferences(&self) -> Result<String> {
        let merged_prefs = self.preferences.get_merged();
        let home = dirs::home_dir().context("Could not determine home directory")?;

        let mut credentials = CredentialManager::new(&self.config_dir);
        let _ = credentials.load();

        let generators: Vec<Box<dyn crate::preferences::ConfigGenerator>> = vec![
            Box::new(crate::preferences::ClaudeConfigGenerator {
                config_dir: home.join(".claude"),
                user_config_path: home.join(".claude.json"),
            }),
            Box::new(crate::preferences::GeminiConfigGenerator {
                config_dir: home.join(".gemini"),
            }),
            Box::new(crate::preferences::OpenCodeConfigGenerator {
                config_dir: home.join(".config/opencode"),
            }),
        ];

        let mut synced_count = 0;

        for generator in generators {
            let files = generator.generate(&merged_prefs, Some(&credentials))?;
            for (path, content) in files {
                let needs_sync = if path.exists() {
                    fs::read_to_string(&path).unwrap_or_default() != content
                } else {
                    true
                };

                if needs_sync {
                    self.backup_if_needed(&path)?;

                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }

                    fs::write(&path, &content)?;
                    synced_count += 1;
                    log::info!(
                        "[{}] Synced config to {}",
                        generator.agent_name(),
                        path.display()
                    );
                }
            }
        }

        if synced_count == 0 {
            Ok("Preferences already in sync.".to_string())
        } else {
            Ok(format!("Synced {} preference files.", synced_count))
        }
    }

    pub fn check_preference_drift(&self) -> bool {
        let merged_prefs = self.preferences.get_merged();
        let home = match dirs::home_dir() {
            Some(h) => h,
            None => return false,
        };

        let mut credentials = CredentialManager::new(&self.config_dir);
        let _ = credentials.load();

        let generators: Vec<Box<dyn crate::preferences::ConfigGenerator>> = vec![
            Box::new(crate::preferences::ClaudeConfigGenerator {
                config_dir: home.join(".claude"),
                user_config_path: home.join(".claude.json"),
            }),
            Box::new(crate::preferences::GeminiConfigGenerator {
                config_dir: home.join(".gemini"),
            }),
            Box::new(crate::preferences::OpenCodeConfigGenerator {
                config_dir: home.join(".config/opencode"),
            }),
        ];

        for generator in generators {
            if let Ok(files) = generator.generate(&merged_prefs, Some(&credentials)) {
                for (path, content) in files {
                    if path.exists() {
                        if fs::read_to_string(&path).unwrap_or_default() != content {
                            return true;
                        }
                    } else {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn backup_if_needed(&self, target_path: &Path) -> Result<()> {
        if target_path.exists() {
            let timestamp = Local::now().format("%Y%m%d_%H%M%S");
            let filename = target_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");

            let is_global = target_path.starts_with(dirs::home_dir().unwrap_or_default())
                && target_path != self.project_agents;

            let backup_name = if is_global {
                format!("global_{}.{}", filename, timestamp)
            } else {
                format!("{}_{}.{}", self.project_id, filename, timestamp)
            };

            let backup = self.backup_dir.join(backup_name);
            fs::copy(target_path, &backup)?;
            log::info!("Created backup: {}", backup.display());
        }
        Ok(())
    }

    pub fn list_backups(&self, agent_index: usize) -> Vec<PathBuf> {
        let agents = self.get_agents();
        if agent_index >= agents.len() {
            return Vec::new();
        }

        let agent = &agents[agent_index];
        let filename = agent
            .target_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        let is_global = agent
            .target_path
            .starts_with(dirs::home_dir().unwrap_or_default())
            && agent.target_path != self.project_agents;

        let prefix = if is_global {
            format!("global_{}", filename)
        } else {
            format!("{}_{}", self.project_id, filename)
        };

        let mut backups = Vec::new();

        if let Ok(entries) = fs::read_dir(&self.backup_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && name.starts_with(&prefix)
                {
                    backups.push(path);
                }
            }
        }

        backups.sort_by(|a, b| b.cmp(a));
        backups
    }

    #[allow(dead_code)]
    pub fn restore_backup(&self, backup_path: &Path, target_path: &Path) -> Result<()> {
        if !backup_path.exists() {
            anyhow::bail!("Backup file does not exist");
        }

        self.backup_if_needed(target_path)?;

        fs::copy(backup_path, target_path)?;
        log::info!("Restored backup from {}", backup_path.display());

        Ok(())
    }

    pub fn get_diff(&self, agent_index: usize) -> Option<String> {
        if agent_index >= self.agent_configs.len() {
            return None;
        }

        let agent_def = &self.agent_configs[agent_index];
        let expected_content = self.get_merged_content(agent_def);

        let agents = self.get_agents();
        let agent = &agents[agent_index];

        if agent.status != AgentStatus::Drift {
            return None;
        }

        let actual_content = fs::read_to_string(&agent.target_path).ok()?;

        Some(format!(
            "Expected:\n{}\n\nActual:\n{}",
            expected_content, actual_content
        ))
    }

    #[allow(dead_code)]
    pub fn validate_markdown(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.project_agents.exists()
            && let Ok(content) = fs::read_to_string(&self.project_agents)
            && content.trim().is_empty()
        {
            warnings.push("Project agents file is empty".to_string());
        }

        for agent_def in &self.agent_configs {
            if let Some(global_file) = &agent_def.global_file
                && global_file.exists()
                && let Ok(content) = fs::read_to_string(global_file)
                && content.trim().is_empty()
            {
                warnings.push(format!("{} global rules file is empty", agent_def.name));
            }
        }

        warnings
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

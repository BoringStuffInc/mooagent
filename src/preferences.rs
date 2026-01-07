use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum McpServerConfig {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    Sse {
        url: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentPreferences {
    #[serde(default)]
    pub general: GeneralPreferences,
    #[serde(default)]
    pub tool_presets: HashMap<String, PresetGroup>,
    #[serde(default)]
    pub individual_tools: HashMap<String, bool>,
    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerConfig>,
    #[serde(default)]
    pub agent_specific: HashMap<String, AgentSpecificPrefs>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GeneralPreferences {
    pub auto_accept_tools: Option<bool>,
    pub enable_logging: Option<bool>,
    pub sandboxed_mode: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetGroup {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSpecificPrefs {
    #[serde(default)]
    pub ui_settings: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerConfig>,
    #[serde(default)]
    pub plugins: HashMap<String, bool>,
}

pub struct PreferenceManager {
    pub global_path: PathBuf,
    pub global_prefs: AgentPreferences,
    pub project_prefs: Option<AgentPreferences>,
}

#[derive(Deserialize)]
struct ProjectConfigWrapper {
    preferences: Option<AgentPreferences>,
}

pub trait ConfigGenerator {
    fn agent_name(&self) -> &str;
    fn generate(&self, prefs: &AgentPreferences) -> Result<Vec<(PathBuf, String)>>;
}

impl PreferenceManager {
    pub fn new(config_dir: &Path) -> Self {
        let global_path = config_dir.join("preferences.toml");

        let mut mgr = Self {
            global_path,
            global_prefs: AgentPreferences::default(),
            project_prefs: None,
        };

        mgr.global_prefs = mgr.default_preferences();
        mgr
    }

    pub fn load_global(&mut self) -> Result<()> {
        if self.global_path.exists() {
            let content = fs::read_to_string(&self.global_path)
                .context("Failed to read global preferences")?;
            let loaded: AgentPreferences =
                toml::from_str(&content).context("Failed to parse global preferences")?;

            let defaults = self.default_preferences();
            self.global_prefs = self.merge_prefs(defaults, Some(loaded));
        } else {
            self.global_prefs = self.default_preferences();
            self.save_global()?;
        }
        Ok(())
    }

    pub fn load_project(&mut self, config_file: &Path) -> Result<()> {
        if config_file.exists() {
            let content =
                fs::read_to_string(config_file).context("Failed to read project config")?;

            let wrapper: ProjectConfigWrapper =
                toml::from_str(&content).context("Failed to parse project preferences")?;

            self.project_prefs = wrapper.preferences;
        } else {
            self.project_prefs = None;
        }
        Ok(())
    }

    pub fn get_merged(&self) -> AgentPreferences {
        self.merge_prefs(self.global_prefs.clone(), self.project_prefs.clone())
    }

    fn merge_prefs(
        &self,
        base: AgentPreferences,
        override_prefs: Option<AgentPreferences>,
    ) -> AgentPreferences {
        let Some(over) = override_prefs else {
            return base;
        };

        let mut merged = base;

        if let Some(val) = over.general.auto_accept_tools {
            merged.general.auto_accept_tools = Some(val);
        }
        if let Some(val) = over.general.enable_logging {
            merged.general.enable_logging = Some(val);
        }
        if let Some(val) = over.general.sandboxed_mode {
            merged.general.sandboxed_mode = Some(val);
        }

        for (k, v) in over.tool_presets {
            merged.tool_presets.insert(k, v);
        }

        for (k, v) in over.individual_tools {
            merged.individual_tools.insert(k, v);
        }

        for (k, v) in over.mcp_servers {
            merged.mcp_servers.insert(k, v);
        }

        for (agent_name, agent_conf) in over.agent_specific {
            let entry = merged.agent_specific.entry(agent_name).or_default();

            for (k, v) in agent_conf.ui_settings {
                entry.ui_settings.insert(k, v);
            }

            for (k, v) in agent_conf.mcp_servers {
                entry.mcp_servers.insert(k, v);
            }

            for (k, v) in agent_conf.plugins {
                entry.plugins.insert(k, v);
            }
        }

        merged
    }

    fn default_preferences(&self) -> AgentPreferences {
        let mut tool_presets = HashMap::new();
        tool_presets.insert("core_unix_tools".to_string(), PresetGroup { enabled: true });
        tool_presets.insert("file_operations".to_string(), PresetGroup { enabled: true });
        tool_presets.insert("code_search".to_string(), PresetGroup { enabled: true });
        tool_presets.insert("network_tools".to_string(), PresetGroup { enabled: false });
        tool_presets.insert(
            "development_tools".to_string(),
            PresetGroup { enabled: true },
        );
        tool_presets.insert("web_access".to_string(), PresetGroup { enabled: false });

        AgentPreferences {
            general: GeneralPreferences {
                auto_accept_tools: Some(true),
                enable_logging: Some(true),
                sandboxed_mode: Some(true),
            },
            tool_presets,
            individual_tools: HashMap::new(),
            mcp_servers: HashMap::new(),
            agent_specific: HashMap::new(),
        }
    }

    pub fn save_global(&self) -> Result<()> {
        let content = toml::to_string_pretty(&self.global_prefs)?;
        if let Some(parent) = self.global_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.global_path, content)?;
        Ok(())
    }
}

pub fn expand_tools(prefs: &AgentPreferences) -> HashMap<String, bool> {
    let mut tools = HashMap::new();

    for (group_name, group) in &prefs.tool_presets {
        if group.enabled
            && let Some(preset_tools) = get_preset_tools(group_name)
        {
            for tool in preset_tools {
                tools.insert(tool.to_string(), true);
            }
        }
    }

    for (tool_name, enabled) in &prefs.individual_tools {
        tools.insert(tool_name.clone(), *enabled);
    }

    tools
}

pub fn get_preset_tools(group_name: &str) -> Option<Vec<&'static str>> {
    match group_name {
        "core_unix_tools" => Some(vec![
            "ls", "cd", "pwd", "cat", "grep", "find", "sed", "awk", "head", "tail", "sort", "uniq",
            "cp", "mv", "rm", "mkdir", "touch",
        ]),
        "file_operations" => Some(vec!["Read", "Write", "Edit", "Glob"]),
        "code_search" => Some(vec!["grep", "find", "Glob", "Search"]),
        "network_tools" => Some(vec!["curl", "wget", "ping"]),
        "development_tools" => Some(vec!["git", "npm", "cargo", "docker", "just"]),
        "web_access" => Some(vec!["WebFetch", "WebSearch"]),
        _ => None,
    }
}

fn read_json_or_empty(path: &Path) -> serde_json::Value {
    if path.exists()
        && let Ok(content) = fs::read_to_string(path)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&content)
        && val.is_object()
    {
        return val;
    }
    serde_json::Value::Object(serde_json::Map::new())
}

pub struct ClaudeConfigGenerator {
    pub config_dir: PathBuf,
    pub user_config_path: PathBuf,
}

impl ConfigGenerator for ClaudeConfigGenerator {
    fn agent_name(&self) -> &str {
        "Claude"
    }

    fn generate(&self, prefs: &AgentPreferences) -> Result<Vec<(PathBuf, String)>> {
        let agent_prefs = prefs.agent_specific.get("Claude");

        if agent_prefs.is_none() && prefs.mcp_servers.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();

        if let Some(ap) = agent_prefs
            && (!ap.ui_settings.is_empty() || !ap.plugins.is_empty())
        {
            let settings_path = self.config_dir.join("settings.json");
            let mut settings_value = read_json_or_empty(&settings_path);
            if !settings_value.is_object() {
                settings_value = serde_json::Value::Object(serde_json::Map::new());
            }
            let settings_map = settings_value.as_object_mut().unwrap();

            for (k, v) in &ap.ui_settings {
                settings_map.insert(k.clone(), v.clone());
            }

            if !ap.plugins.is_empty() {
                let plugins_obj = settings_map
                    .entry("enabledPlugins")
                    .or_insert(serde_json::Value::Object(serde_json::Map::new()));

                if let Some(plugins_map) = plugins_obj.as_object_mut() {
                    for (k, v) in &ap.plugins {
                        plugins_map.insert(k.clone(), serde_json::Value::Bool(*v));
                    }
                }
            }

            results.push((
                settings_path,
                serde_json::to_string_pretty(&settings_value)?,
            ));
        }

        if !prefs.mcp_servers.is_empty() {
            let mut user_config = read_json_or_empty(&self.user_config_path);
            if !user_config.is_object() {
                user_config = serde_json::Value::Object(serde_json::Map::new());
            }
            let user_map = user_config.as_object_mut().unwrap();

            let mut servers = serde_json::Map::new();
            for (name, config) in &prefs.mcp_servers {
                let mut server_def = serde_json::Map::new();
                match config {
                    McpServerConfig::Stdio { command, args, env } => {
                        server_def.insert(
                            "type".to_string(),
                            serde_json::Value::String("stdio".to_string()),
                        );
                        server_def.insert(
                            "command".to_string(),
                            serde_json::Value::String(command.clone()),
                        );
                        server_def.insert(
                            "args".to_string(),
                            serde_json::Value::Array(
                                args.iter()
                                    .map(|s| serde_json::Value::String(s.clone()))
                                    .collect(),
                            ),
                        );
                        server_def.insert("env".to_string(), serde_json::to_value(env)?);
                    }
                    McpServerConfig::Sse { url } => {
                        server_def.insert(
                            "type".to_string(),
                            serde_json::Value::String("sse".to_string()),
                        );
                        server_def
                            .insert("url".to_string(), serde_json::Value::String(url.clone()));
                    }
                }
                servers.insert(name.clone(), serde_json::Value::Object(server_def));
            }
            user_map.insert("mcpServers".to_string(), serde_json::Value::Object(servers));

            results.push((
                self.user_config_path.clone(),
                serde_json::to_string_pretty(&user_config)?,
            ));
        }

        Ok(results)
    }
}

pub struct GeminiConfigGenerator {
    pub config_dir: PathBuf,
}

impl ConfigGenerator for GeminiConfigGenerator {
    fn agent_name(&self) -> &str {
        "Gemini"
    }

    fn generate(&self, prefs: &AgentPreferences) -> Result<Vec<(PathBuf, String)>> {
        let agent_prefs = prefs.agent_specific.get("Gemini");
        let mut results = Vec::new();

        let settings_path = self.config_dir.join("settings.json");
        let mut settings_value = read_json_or_empty(&settings_path);
        if !settings_value.is_object() {
            settings_value = serde_json::Value::Object(serde_json::Map::new());
        }
        let settings_map = settings_value.as_object_mut().unwrap();

        if let Some(auto_accept) = prefs.general.auto_accept_tools {
            let tools_obj = settings_map
                .entry("tools")
                .or_insert(serde_json::Value::Object(serde_json::Map::new()));
            if let Some(tools_map) = tools_obj.as_object_mut() {
                tools_map.insert(
                    "autoAccept".to_string(),
                    serde_json::Value::Bool(auto_accept),
                );
            }
        }

        if let Some(ap) = agent_prefs
            && !ap.ui_settings.is_empty()
        {
            let ui_obj = settings_map
                .entry("ui")
                .or_insert(serde_json::Value::Object(serde_json::Map::new()));

            if let Some(ui_map) = ui_obj.as_object_mut() {
                for (k, v) in &ap.ui_settings {
                    ui_map.insert(k.clone(), v.clone());
                }
            }
        }

        let mut servers = serde_json::Map::new();
        for (name, config) in &prefs.mcp_servers {
            let mut server_def = serde_json::Map::new();
            match config {
                McpServerConfig::Stdio { command, args, env } => {
                    server_def.insert(
                        "command".to_string(),
                        serde_json::Value::String(command.clone()),
                    );
                    server_def.insert(
                        "args".to_string(),
                        serde_json::Value::Array(
                            args.iter()
                                .map(|s| serde_json::Value::String(s.clone()))
                                .collect(),
                        ),
                    );
                    server_def.insert("env".to_string(), serde_json::to_value(env)?);
                }
                McpServerConfig::Sse { url } => {
                    server_def.insert("url".to_string(), serde_json::Value::String(url.clone()));
                }
            }
            servers.insert(name.clone(), serde_json::Value::Object(server_def));
        }
        settings_map.insert("mcpServers".to_string(), serde_json::Value::Object(servers));
        results.push((
            settings_path,
            serde_json::to_string_pretty(&settings_value)?,
        ));

        let enabled_tools = expand_tools(prefs);
        let tools_path = self.config_dir.join("tools.json");
        let mut tools_value = read_json_or_empty(&tools_path);
        if !tools_value.is_object() {
            tools_value = serde_json::Value::Object(serde_json::Map::new());
        }
        let tools_map = tools_value.as_object_mut().unwrap();

        for (tool, enabled) in enabled_tools {
            tools_map.insert(tool, serde_json::Value::Bool(enabled));
        }

        results.push((tools_path, serde_json::to_string_pretty(&tools_value)?));

        Ok(results)
    }
}

pub struct OpenCodeConfigGenerator {
    pub config_dir: PathBuf,
}

impl ConfigGenerator for OpenCodeConfigGenerator {
    fn agent_name(&self) -> &str {
        "OpenCode"
    }

    fn generate(&self, prefs: &AgentPreferences) -> Result<Vec<(PathBuf, String)>> {
        let mut results = Vec::new();

        let config_path = self.config_dir.join("opencode.json");
        let mut config_value = read_json_or_empty(&config_path);
        if !config_value.is_object() {
            config_value = serde_json::Value::Object(serde_json::Map::new());
        }
        let config_map = config_value.as_object_mut().unwrap();

        config_map.insert(
            "$schema".to_string(),
            serde_json::Value::String("https://opencode.ai/config.json".to_string()),
        );

        let mut mcp_servers = serde_json::Map::new();
        for (name, config) in &prefs.mcp_servers {
            let mut server_def = serde_json::Map::new();
            match config {
                McpServerConfig::Stdio { command, args, env } => {
                    server_def.insert(
                        "type".to_string(),
                        serde_json::Value::String("local".to_string()),
                    );
                    let mut cmd_array = vec![serde_json::Value::String(command.clone())];
                    cmd_array.extend(args.iter().map(|s| serde_json::Value::String(s.clone())));
                    server_def.insert("command".to_string(), serde_json::Value::Array(cmd_array));
                    if !env.is_empty() {
                        server_def.insert("environment".to_string(), serde_json::to_value(env)?);
                    }
                }
                McpServerConfig::Sse { url } => {
                    server_def.insert(
                        "type".to_string(),
                        serde_json::Value::String("remote".to_string()),
                    );
                    server_def.insert("url".to_string(), serde_json::Value::String(url.clone()));
                }
            }
            mcp_servers.insert(name.clone(), serde_json::Value::Object(server_def));
        }
        config_map.insert("mcp".to_string(), serde_json::Value::Object(mcp_servers));

        let enabled_tools = expand_tools(prefs);
        if !enabled_tools.is_empty() {
            let tools_obj = config_map
                .entry("tools")
                .or_insert(serde_json::Value::Object(serde_json::Map::new()));

            if let Some(tools_map) = tools_obj.as_object_mut() {
                for (tool, enabled) in enabled_tools {
                    tools_map.insert(tool, serde_json::Value::Bool(enabled));
                }
            }
        }

        results.push((config_path, serde_json::to_string_pretty(&config_value)?));

        Ok(results)
    }
}

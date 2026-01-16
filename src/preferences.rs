use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpAuth {
    #[default]
    None,
    Bearer {
        token: String,
    },
    OAuth {
        client_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        client_secret: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        auth_server_url: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        scopes: Vec<String>,
    },
}

impl McpAuth {
    pub fn is_none(&self) -> bool {
        matches!(self, McpAuth::None)
    }

    pub fn requires_oauth(&self) -> bool {
        matches!(self, McpAuth::OAuth { .. })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum McpServerConfig {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
        #[serde(default)]
        disabled_tools: Vec<String>,
        #[serde(default)]
        auto_allow: bool,
    },
    Sse {
        url: String,
        #[serde(default, skip_serializing_if = "McpAuth::is_none")]
        auth: McpAuth,
        #[serde(default)]
        disabled_tools: Vec<String>,
        #[serde(default)]
        auto_allow: bool,
    },
    Http {
        #[serde(rename = "httpUrl")]
        http_url: String,
        #[serde(default, skip_serializing_if = "McpAuth::is_none")]
        auth: McpAuth,
        #[serde(default)]
        disabled_tools: Vec<String>,
        #[serde(default)]
        auto_allow: bool,
    },
}

impl McpServerConfig {
    #[allow(dead_code)]
    pub fn url(&self) -> Option<&str> {
        match self {
            McpServerConfig::Sse { url, .. } => Some(url),
            McpServerConfig::Http { http_url, .. } => Some(http_url),
            McpServerConfig::Stdio { .. } => None,
        }
    }

    pub fn auth(&self) -> Option<&McpAuth> {
        match self {
            McpServerConfig::Sse { auth, .. } => Some(auth),
            McpServerConfig::Http { auth, .. } => Some(auth),
            McpServerConfig::Stdio { .. } => None,
        }
    }

    pub fn requires_oauth(&self) -> bool {
        self.auth().map(|a| a.requires_oauth()).unwrap_or(false)
    }

    pub fn disabled_tools(&self) -> &[String] {
        match self {
            McpServerConfig::Stdio { disabled_tools, .. } => disabled_tools,
            McpServerConfig::Sse { disabled_tools, .. } => disabled_tools,
            McpServerConfig::Http { disabled_tools, .. } => disabled_tools,
        }
    }

    pub fn auto_allow(&self) -> bool {
        match self {
            McpServerConfig::Stdio { auto_allow, .. } => *auto_allow,
            McpServerConfig::Sse { auto_allow, .. } => *auto_allow,
            McpServerConfig::Http { auto_allow, .. } => *auto_allow,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolPermissions {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub ask: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
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
    pub tool_permissions: ToolPermissions,
    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerConfig>,
    #[serde(default)]
    pub disabled_mcp_servers: Vec<String>,
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

use crate::credentials::CredentialManager;

pub trait ConfigGenerator {
    fn agent_name(&self) -> &str;
    fn generate(
        &self,
        prefs: &AgentPreferences,
        credentials: Option<&CredentialManager>,
    ) -> Result<Vec<(PathBuf, String)>>;
}

fn get_auth_headers(
    config: &McpServerConfig,
    credentials: Option<&CredentialManager>,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let (url, auth) = match config {
        McpServerConfig::Sse { url, auth, .. } => (url, auth),
        McpServerConfig::Http { http_url, auth, .. } => (http_url, auth),
        McpServerConfig::Stdio { .. } => return None,
    };

    match auth {
        McpAuth::Bearer { token } => {
            let mut headers = serde_json::Map::new();
            headers.insert(
                "Authorization".to_string(),
                serde_json::Value::String(format!("Bearer {}", token)),
            );
            Some(headers)
        }
        McpAuth::OAuth { .. } => {
            if let Some(creds) = credentials
                && let Some(token) = creds.get_valid_token(url)
            {
                let mut headers = serde_json::Map::new();
                headers.insert(
                    "Authorization".to_string(),
                    serde_json::Value::String(format!("Bearer {}", token.access_token)),
                );
                Some(headers)
            } else {
                None
            }
        }
        McpAuth::None => None,
    }
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

        merged
            .disabled_mcp_servers
            .extend(over.disabled_mcp_servers);

        for name in &merged.disabled_mcp_servers {
            merged.mcp_servers.remove(name);
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
            tool_permissions: ToolPermissions::default(),
            mcp_servers: HashMap::new(),
            disabled_mcp_servers: Vec::new(),
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

    pub fn save_project(&self, config_file: &Path) -> Result<()> {
        let Some(prefs) = &self.project_prefs else {
            return Ok(());
        };

        let mut existing_content = String::new();
        if config_file.exists() {
            existing_content = fs::read_to_string(config_file).unwrap_or_default();
        }

        let mut toml_val: toml::Value = if !existing_content.is_empty() {
            toml::from_str(&existing_content)
                .unwrap_or_else(|_| toml::Value::Table(toml::map::Map::new()))
        } else {
            toml::Value::Table(toml::map::Map::new())
        };

        let prefs_val = toml::Value::try_from(prefs.clone())?;

        // Preserve other fields in .mooagent.toml (e.g. agents)
        if let toml::Value::Table(ref mut map) = toml_val {
            map.insert("preferences".to_string(), prefs_val);
        }

        let content = toml::to_string_pretty(&toml_val)?;

        if let Some(parent) = config_file.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(config_file, content)?;
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

fn read_json_or_empty(path: &Path) -> serde_json::Map<String, serde_json::Value> {
    if path.exists()
        && let Ok(content) = fs::read_to_string(path)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&content)
        && let serde_json::Value::Object(map) = val
    {
        return map;
    }
    serde_json::Map::new()
}

pub struct ClaudeConfigGenerator {
    pub config_dir: PathBuf,
    pub user_config_path: PathBuf,
}

impl ConfigGenerator for ClaudeConfigGenerator {
    fn agent_name(&self) -> &str {
        "Claude"
    }

    fn generate(
        &self,
        prefs: &AgentPreferences,
        credentials: Option<&CredentialManager>,
    ) -> Result<Vec<(PathBuf, String)>> {
        let agent_prefs = prefs.agent_specific.get("Claude");
        let mut results = Vec::new();

        let settings_path = self.config_dir.join("settings.json");
        let mut settings_map = read_json_or_empty(&settings_path);

        if let Some(ap) = agent_prefs {
            for (k, v) in &ap.ui_settings {
                settings_map.insert(k.clone(), v.clone());
            }

            if !ap.plugins.is_empty() {
                let plugins_obj = settings_map
                    .entry("enabledPlugins".to_string())
                    .or_insert(serde_json::Value::Object(serde_json::Map::new()));

                if let Some(plugins_map) = plugins_obj.as_object_mut() {
                    for (k, v) in &ap.plugins {
                        plugins_map.insert(k.clone(), serde_json::Value::Bool(*v));
                    }
                }
            }
        }

        let permissions = build_claude_permissions(prefs);
        let mut perm_map = serde_json::Map::new();

        if !permissions.allow.is_empty() {
            perm_map.insert(
                "allow".to_string(),
                serde_json::Value::Array(
                    permissions
                        .allow
                        .iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                ),
            );
        }

        if !permissions.ask.is_empty() {
            perm_map.insert(
                "ask".to_string(),
                serde_json::Value::Array(
                    permissions
                        .ask
                        .iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                ),
            );
        }

        if !permissions.deny.is_empty() {
            perm_map.insert(
                "deny".to_string(),
                serde_json::Value::Array(
                    permissions
                        .deny
                        .iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                ),
            );
        }

        if !perm_map.is_empty() {
            settings_map.insert("permissions".to_string(), serde_json::Value::Object(perm_map));
        }

        results.push((settings_path, serde_json::to_string_pretty(&settings_map)?));

        let mut user_map = read_json_or_empty(&self.user_config_path);

        if !prefs.mcp_servers.is_empty() {
            let mut servers = serde_json::Map::new();
            for (name, config) in &prefs.mcp_servers {
                let mut server_def = serde_json::Map::new();
                match config {
                    McpServerConfig::Stdio {
                        command,
                        args,
                        env,
                        auto_allow,
                        ..
                    } => {
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
                        if *auto_allow {
                            server_def
                                .insert("autoAllow".to_string(), serde_json::Value::Bool(true));
                        }
                    }
                    McpServerConfig::Sse {
                        url,
                        auto_allow,
                        ..
                    } => {
                        server_def.insert(
                            "type".to_string(),
                            serde_json::Value::String("sse".to_string()),
                        );
                        server_def
                            .insert("url".to_string(), serde_json::Value::String(url.clone()));
                        if let Some(headers) = get_auth_headers(config, credentials) {
                            server_def
                                .insert("headers".to_string(), serde_json::Value::Object(headers));
                        }
                        if *auto_allow {
                            server_def
                                .insert("autoAllow".to_string(), serde_json::Value::Bool(true));
                        }
                    }
                    McpServerConfig::Http {
                        http_url,
                        auto_allow,
                        ..
                    } => {
                        server_def.insert(
                            "type".to_string(),
                            serde_json::Value::String("http".to_string()),
                        );
                        server_def.insert(
                            "url".to_string(),
                            serde_json::Value::String(http_url.clone()),
                        );
                        if let Some(headers) = get_auth_headers(config, credentials) {
                            server_def
                                .insert("headers".to_string(), serde_json::Value::Object(headers));
                        }
                        if *auto_allow {
                            server_def
                                .insert("autoAllow".to_string(), serde_json::Value::Bool(true));
                        }
                    }
                }
                servers.insert(name.clone(), serde_json::Value::Object(server_def));
            }
            user_map.insert("mcpServers".to_string(), serde_json::Value::Object(servers));
        }

        results.push((
            self.user_config_path.clone(),
            serde_json::to_string_pretty(&user_map)?,
        ));

        Ok(results)
    }
}

fn build_claude_permissions(prefs: &AgentPreferences) -> ToolPermissions {
    let mut result = ToolPermissions::default();

    result.allow.extend(prefs.tool_permissions.allow.clone());
    result.ask.extend(prefs.tool_permissions.ask.clone());
    result.deny.extend(prefs.tool_permissions.deny.clone());

    let enabled_tools = expand_tools(prefs);
    for (tool, enabled) in &enabled_tools {
        if let Some(pattern) = format_tool_permission(tool)
            && *enabled
            && !result.allow.contains(&pattern)
            && !result.ask.contains(&pattern)
            && !result.deny.contains(&pattern)
        {
            result.allow.push(pattern);
        }
    }

    for (server_name, config) in &prefs.mcp_servers {
        if config.auto_allow() {
            let pattern = format!("mcp__{}__*", server_name);
            if !result.allow.contains(&pattern) {
                result.allow.push(pattern);
            }
        }

        for tool in config.disabled_tools() {
            let pattern = format!("mcp__{}__{}", server_name, tool);
            if !result.deny.contains(&pattern) {
                result.deny.push(pattern);
            }
        }
    }

    result.allow.sort();
    result.ask.sort();
    result.deny.sort();

    result
}

fn format_tool_permission(tool: &str) -> Option<String> {
    // Only generate permissions for Bash commands - these use prefix matching with :*
    // Other tools either:
    // - Don't need permissions (Glob, Grep - read-only)
    // - Don't support wildcards (WebSearch)
    // - Need specific patterns users should configure manually (Read, Edit, WebFetch)
    // - Are internal/special (Task, TodoWrite, etc.)

    let skip_tools = [
        "Read", "Write", "Edit", "Glob", "Grep", "Search",
        "WebFetch", "WebSearch", "Task", "TodoWrite",
        "NotebookEdit", "LSP", "AskFollowupQuestion",
    ];

    if skip_tools.contains(&tool) {
        return None;
    }

    // For regular CLI tools, generate Bash permission with prefix matching
    Some(format!("Bash({}:*)", tool))
}

pub struct GeminiConfigGenerator {
    pub config_dir: PathBuf,
}

impl ConfigGenerator for GeminiConfigGenerator {
    fn agent_name(&self) -> &str {
        "Gemini"
    }

    fn generate(
        &self,
        prefs: &AgentPreferences,
        credentials: Option<&CredentialManager>,
    ) -> Result<Vec<(PathBuf, String)>> {
        let agent_prefs = prefs.agent_specific.get("Gemini");
        let mut results = Vec::new();

        let settings_path = self.config_dir.join("settings.json");
        let mut settings_map = read_json_or_empty(&settings_path);

        if let Some(auto_accept) = prefs.general.auto_accept_tools {
            let tools_obj = settings_map
                .entry("tools".to_string())
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
                .entry("ui".to_string())
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
                McpServerConfig::Stdio {
                    command,
                    args,
                    env,
                    auto_allow,
                    ..
                } => {
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
                    if *auto_allow {
                        server_def
                            .insert("autoAllow".to_string(), serde_json::Value::Bool(true));
                    }
                }
                McpServerConfig::Sse {
                    url,
                    auto_allow,
                    ..
                } => {
                    server_def.insert("url".to_string(), serde_json::Value::String(url.clone()));
                    if let Some(headers) = get_auth_headers(config, credentials) {
                        server_def
                            .insert("headers".to_string(), serde_json::Value::Object(headers));
                    }
                    if *auto_allow {
                        server_def
                            .insert("autoAllow".to_string(), serde_json::Value::Bool(true));
                    }
                }
                McpServerConfig::Http {
                    http_url,
                    auto_allow,
                    ..
                } => {
                    server_def.insert(
                        "httpUrl".to_string(),
                        serde_json::Value::String(http_url.clone()),
                    );
                    if let Some(headers) = get_auth_headers(config, credentials) {
                        server_def
                            .insert("headers".to_string(), serde_json::Value::Object(headers));
                    }
                    if *auto_allow {
                        server_def
                            .insert("autoAllow".to_string(), serde_json::Value::Bool(true));
                    }
                }
            }
            servers.insert(name.clone(), serde_json::Value::Object(server_def));
        }
        settings_map.insert("mcpServers".to_string(), serde_json::Value::Object(servers));
        results.push((settings_path, serde_json::to_string_pretty(&settings_map)?));

        let mut enabled_tools = expand_tools(prefs);

        // Apply disabled tools from MCP servers
        for config in prefs.mcp_servers.values() {
            for tool in config.disabled_tools() {
                enabled_tools.insert(tool.clone(), false);
            }
        }

        let tools_path = self.config_dir.join("tools.json");
        let mut tools_map = read_json_or_empty(&tools_path);

        for (tool, enabled) in enabled_tools {
            tools_map.insert(tool, serde_json::Value::Bool(enabled));
        }

        results.push((tools_path, serde_json::to_string_pretty(&tools_map)?));

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

    fn generate(
        &self,
        prefs: &AgentPreferences,
        credentials: Option<&CredentialManager>,
    ) -> Result<Vec<(PathBuf, String)>> {
        let mut results = Vec::new();

        let config_path = self.config_dir.join("opencode.json");
        let mut config_map = read_json_or_empty(&config_path);

        config_map.insert(
            "$schema".to_string(),
            serde_json::Value::String("https://opencode.ai/config.json".to_string()),
        );

        let mut mcp_servers = serde_json::Map::new();
        for (name, config) in &prefs.mcp_servers {
            let mut server_def = serde_json::Map::new();
            match config {
                McpServerConfig::Stdio {
                    command,
                    args,
                    env,
                    auto_allow,
                    ..
                } => {
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
                    if *auto_allow {
                        server_def
                            .insert("autoAllow".to_string(), serde_json::Value::Bool(true));
                    }
                }
                McpServerConfig::Sse {
                    url,
                    auto_allow,
                    ..
                } => {
                    server_def.insert(
                        "type".to_string(),
                        serde_json::Value::String("remote".to_string()),
                    );
                    server_def.insert("url".to_string(), serde_json::Value::String(url.clone()));
                    if let Some(headers) = get_auth_headers(config, credentials) {
                        server_def
                            .insert("headers".to_string(), serde_json::Value::Object(headers));
                    }
                    if *auto_allow {
                        server_def
                            .insert("autoAllow".to_string(), serde_json::Value::Bool(true));
                    }
                }
                McpServerConfig::Http {
                    http_url,
                    auto_allow,
                    ..
                } => {
                    server_def.insert(
                        "type".to_string(),
                        serde_json::Value::String("remote".to_string()),
                    );
                    server_def.insert(
                        "url".to_string(),
                        serde_json::Value::String(http_url.clone()),
                    );
                    if let Some(headers) = get_auth_headers(config, credentials) {
                        server_def
                            .insert("headers".to_string(), serde_json::Value::Object(headers));
                    }
                    if *auto_allow {
                        server_def
                            .insert("autoAllow".to_string(), serde_json::Value::Bool(true));
                    }
                }
            }
            mcp_servers.insert(name.clone(), serde_json::Value::Object(server_def));
        }
        config_map.insert("mcp".to_string(), serde_json::Value::Object(mcp_servers));

        let mut enabled_tools = expand_tools(prefs);

        // Apply disabled tools from MCP servers
        for config in prefs.mcp_servers.values() {
            for tool in config.disabled_tools() {
                enabled_tools.insert(tool.clone(), false);
            }
        }

        if !enabled_tools.is_empty() {
            let tools_obj = config_map
                .entry("tools".to_string())
                .or_insert(serde_json::Value::Object(serde_json::Map::new()));

            if let Some(tools_map) = tools_obj.as_object_mut() {
                for (tool, enabled) in enabled_tools {
                    tools_map.insert(tool, serde_json::Value::Bool(enabled));
                }
            }
        }

        results.push((config_path, serde_json::to_string_pretty(&config_map)?));

        Ok(results)
    }
}

use crate::config::{AgentInfo, ConfigPaths};
use crate::credentials::{CredentialManager, TokenStatus};
use crate::preferences::{McpAuth, McpServerConfig};
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
    AddTool,
    EditMcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Dashboard,
    Preferences,
    McpServers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Agents,
    Global,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefEditorFocus {
    Presets,
    IndividualTools,
    GeneralSettings,
}

pub struct PreferenceEditorState {
    pub focus: PrefEditorFocus,
    pub selected_preset: usize,
    pub selected_tool: usize,
    pub selected_general: usize,
    pub preset_list: Vec<String>,
    pub individual_tool_list: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum McpAuthType {
    #[default]
    None,
    Bearer,
    OAuth,
}

impl McpAuthType {
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Bearer,
            Self::Bearer => Self::OAuth,
            Self::OAuth => Self::None,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::None => Self::OAuth,
            Self::Bearer => Self::None,
            Self::OAuth => Self::Bearer,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpFieldFocus {
    Name,
    Command,
    Args,
    Env,
    AuthType,
    BearerToken,
    OAuthClientId,
    OAuthClientSecret,
    OAuthScopes,
    OAuthAuthServerUrl,
}

pub struct McpEditorState {
    pub selected_server_idx: usize,
    pub server_list: Vec<String>,

    pub is_new: bool,
    pub editing_name: String,
    pub editing_command: String,
    pub editing_args: String,
    pub editing_env: String,
    pub focus: McpFieldFocus,

    pub editing_auth_type: McpAuthType,
    pub editing_bearer_token: String,
    pub editing_oauth_client_id: String,
    pub editing_oauth_client_secret: String,
    pub editing_oauth_scopes: String,
    pub editing_oauth_auth_server_url: String,
}

impl Default for PreferenceEditorState {
    fn default() -> Self {
        Self {
            focus: PrefEditorFocus::Presets,
            selected_preset: 0,
            selected_tool: 0,
            selected_general: 0,
            preset_list: vec![
                "core_unix_tools".to_string(),
                "file_operations".to_string(),
                "code_search".to_string(),
                "network_tools".to_string(),
                "development_tools".to_string(),
                "web_access".to_string(),
            ],
            individual_tool_list: vec![
                "ls".to_string(),
                "cd".to_string(),
                "pwd".to_string(),
                "grep".to_string(),
                "find".to_string(),
                "Read".to_string(),
                "Write".to_string(),
                "Edit".to_string(),
                "Glob".to_string(),
                "curl".to_string(),
                "wget".to_string(),
                "git".to_string(),
                "npm".to_string(),
                "cargo".to_string(),
                "WebFetch".to_string(),
                "WebSearch".to_string(),
            ],
        }
    }
}

impl Default for McpEditorState {
    fn default() -> Self {
        Self {
            selected_server_idx: 0,
            server_list: Vec::new(),
            is_new: false,
            editing_name: String::new(),
            editing_command: String::new(),
            editing_args: String::new(),
            editing_env: String::new(),
            focus: McpFieldFocus::Name,
            editing_auth_type: McpAuthType::None,
            editing_bearer_token: String::new(),
            editing_oauth_client_id: String::new(),
            editing_oauth_client_secret: String::new(),
            editing_oauth_scopes: String::new(),
            editing_oauth_auth_server_url: String::new(),
        }
    }
}

impl McpEditorState {
    pub fn clear_auth_fields(&mut self) {
        self.editing_auth_type = McpAuthType::None;
        self.editing_bearer_token.clear();
        self.editing_oauth_client_id.clear();
        self.editing_oauth_client_secret.clear();
        self.editing_oauth_scopes.clear();
        self.editing_oauth_auth_server_url.clear();
    }

    pub fn is_remote_server(&self) -> bool {
        let cmd = self.editing_command.trim();
        cmd.starts_with("http://") || cmd.starts_with("https://")
    }
}

pub struct OAuthFlowConfig {
    pub server_url: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub scopes: Vec<String>,
    pub auth_server_url: Option<String>,
}

pub struct App {
    pub paths: ConfigPaths,
    pub agents: Vec<AgentInfo>,
    pub project_content: String,
    pub global_content: String,
    pub status_message: Option<(String, Instant)>,
    pub event_rx: Option<Receiver<()>>,
    pub selected_agent: usize,
    pub project_scroll: usize,
    pub global_scroll: usize,
    pub detail_scroll: usize,
    pub mode: AppMode,
    pub active_tab: ActiveTab,
    pub focus: Focus,
    pub pending_g: bool,
    pub status_log: Vec<(String, Instant)>,
    pub search_query: String,
    pub status_message_timeout: u64,
    pub auto_sync: bool,
    pub filtered_agents: Vec<usize>,
    pub show_error_log: bool,
    pub preference_drift: bool,
    pub pref_editor_state: PreferenceEditorState,
    pub mcp_editor_state: McpEditorState,
    pub new_tool_input: String,
    pub should_quit: bool,
    pub credentials: CredentialManager,
}

impl App {
    pub fn new(event_rx: Option<Receiver<()>>) -> Result<Self> {
        let paths = ConfigPaths::new()?;
        paths.ensure_files_exist()?;
        let project_content = paths.read_project_content();
        let global_content = if paths.global_rules_primary.exists() {
            std::fs::read_to_string(&paths.global_rules_primary).unwrap_or_default()
        } else {
            String::new()
        };
        let agents = paths.get_agents();

        let filtered_agents: Vec<usize> = (0..agents.len()).collect();

        let mut credentials = CredentialManager::new(&paths.config_dir);
        let _ = credentials.load();

        let mut app = Self {
            paths,
            agents,
            project_content,
            global_content,
            status_message: None,
            event_rx,
            selected_agent: 0,
            project_scroll: 0,
            global_scroll: 0,
            detail_scroll: 0,
            mode: AppMode::Normal,
            active_tab: ActiveTab::Dashboard,
            focus: Focus::Agents,
            pending_g: false,
            status_log: Vec::new(),
            search_query: String::new(),
            status_message_timeout: 5,
            auto_sync: false,
            filtered_agents,
            show_error_log: false,
            preference_drift: false,
            pref_editor_state: PreferenceEditorState::default(),
            mcp_editor_state: McpEditorState::default(),
            new_tool_input: String::new(),
            should_quit: false,
            credentials,
        };

        app.update_mcp_list();
        Ok(app)
    }

    pub fn refresh(&mut self) {
        self.project_content = self.paths.read_project_content();
        self.global_content = if self.paths.global_rules_primary.exists() {
            std::fs::read_to_string(&self.paths.global_rules_primary).unwrap_or_default()
        } else {
            String::new()
        };
        self.agents = self.paths.get_agents();
        self.preference_drift = self.paths.check_preference_drift();

        self.update_filter();

        if self.selected_agent >= self.agents.len() && !self.agents.is_empty() {
            self.selected_agent = self.agents.len() - 1;
        }

        if self.auto_sync {
            let _ = self.sync();
        }

        self.update_mcp_list();
    }

    pub fn update_mcp_list(&mut self) {
        let mut servers: Vec<String> = self
            .paths
            .preferences
            .global_prefs
            .mcp_servers
            .keys()
            .cloned()
            .collect();
        servers.sort();
        self.mcp_editor_state.server_list = servers;

        if self.mcp_editor_state.selected_server_idx >= self.mcp_editor_state.server_list.len()
            && !self.mcp_editor_state.server_list.is_empty()
        {
            self.mcp_editor_state.selected_server_idx = self.mcp_editor_state.server_list.len() - 1;
        }
    }

    pub fn mcp_next_server(&mut self) {
        if self.mcp_editor_state.server_list.is_empty() {
            return;
        }
        if self.mcp_editor_state.selected_server_idx < self.mcp_editor_state.server_list.len() - 1 {
            self.mcp_editor_state.selected_server_idx += 1;
        }
    }

    pub fn mcp_prev_server(&mut self) {
        if self.mcp_editor_state.server_list.is_empty() {
            return;
        }
        if self.mcp_editor_state.selected_server_idx > 0 {
            self.mcp_editor_state.selected_server_idx -= 1;
        }
    }

    pub fn mcp_start_add(&mut self) {
        self.mcp_editor_state.is_new = true;
        self.mcp_editor_state.editing_name.clear();
        self.mcp_editor_state.editing_command.clear();
        self.mcp_editor_state.editing_args.clear();
        self.mcp_editor_state.editing_env.clear();
        self.mcp_editor_state.clear_auth_fields();
        self.mcp_editor_state.focus = McpFieldFocus::Name;
        self.mode = AppMode::EditMcp;
    }

    pub fn mcp_start_edit(&mut self) {
        if self.mcp_editor_state.server_list.is_empty() {
            return;
        }

        let server_name =
            self.mcp_editor_state.server_list[self.mcp_editor_state.selected_server_idx].clone();

        let Some(config) = self
            .paths
            .preferences
            .global_prefs
            .mcp_servers
            .get(&server_name)
            .cloned()
        else {
            return;
        };

        self.mcp_editor_state.is_new = false;
        self.mcp_editor_state.editing_name = server_name;
        self.mcp_editor_state.clear_auth_fields();

        match &config {
            McpServerConfig::Stdio { command, args, env } => {
                self.mcp_editor_state.editing_command = command.clone();
                self.mcp_editor_state.editing_args = args.join(" ");
                self.mcp_editor_state.editing_env = env
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(",");
            }
            McpServerConfig::Sse { url, auth } => {
                self.mcp_editor_state.editing_command = url.clone();
                self.mcp_editor_state.editing_args.clear();
                self.mcp_editor_state.editing_env.clear();
                self.populate_auth_fields(auth);
            }
            McpServerConfig::Http { http_url, auth } => {
                self.mcp_editor_state.editing_command = http_url.clone();
                self.mcp_editor_state.editing_args.clear();
                self.mcp_editor_state.editing_env.clear();
                self.populate_auth_fields(auth);
            }
        }
        self.mcp_editor_state.focus = McpFieldFocus::Command;
        self.mode = AppMode::EditMcp;
    }

    fn populate_auth_fields(&mut self, auth: &McpAuth) {
        match auth {
            McpAuth::None => {
                self.mcp_editor_state.editing_auth_type = McpAuthType::None;
            }
            McpAuth::Bearer { token } => {
                self.mcp_editor_state.editing_auth_type = McpAuthType::Bearer;
                self.mcp_editor_state.editing_bearer_token = token.clone();
            }
            McpAuth::OAuth {
                client_id,
                client_secret,
                auth_server_url,
                scopes,
            } => {
                self.mcp_editor_state.editing_auth_type = McpAuthType::OAuth;
                self.mcp_editor_state.editing_oauth_client_id = client_id.clone();
                self.mcp_editor_state.editing_oauth_client_secret =
                    client_secret.clone().unwrap_or_default();
                self.mcp_editor_state.editing_oauth_auth_server_url =
                    auth_server_url.clone().unwrap_or_default();
                self.mcp_editor_state.editing_oauth_scopes = scopes.join(" ");
            }
        }
    }

    pub fn mcp_delete(&mut self) {
        if self.mcp_editor_state.server_list.is_empty() {
            return;
        }
        let server_name =
            self.mcp_editor_state.server_list[self.mcp_editor_state.selected_server_idx].clone();

        self.paths
            .preferences
            .global_prefs
            .mcp_servers
            .remove(&server_name);
        let _ = self.paths.preferences.save_global();
        self.update_mcp_list();
        self.set_status(format!("Deleted MCP server: {}", server_name));
    }

    pub fn mcp_toggle_enabled(&mut self) {
        if self.mcp_editor_state.server_list.is_empty() {
            return;
        }
        let server_name =
            self.mcp_editor_state.server_list[self.mcp_editor_state.selected_server_idx].clone();

        if self.paths.preferences.project_prefs.is_none() {
            self.paths.preferences.project_prefs =
                Some(crate::preferences::AgentPreferences::default());
        }

        if let Some(project_prefs) = &mut self.paths.preferences.project_prefs {
            if let Some(pos) = project_prefs
                .disabled_mcp_servers
                .iter()
                .position(|x| x == &server_name)
            {
                project_prefs.disabled_mcp_servers.remove(pos);
                self.set_status(format!(
                    "Enabled MCP server: {} for this project",
                    server_name
                ));
            } else {
                project_prefs.disabled_mcp_servers.push(server_name.clone());
                self.set_status(format!(
                    "Disabled MCP server: {} for this project",
                    server_name
                ));
            }

            if let Err(e) = self.paths.preferences.save_project(&self.paths.config_file) {
                self.set_status(format!("Failed to save project config: {}", e));
            }
        }
    }

    pub fn mcp_submit(&mut self) {
        let name = self.mcp_editor_state.editing_name.trim().to_string();
        let command = self.mcp_editor_state.editing_command.trim().to_string();
        let args: Vec<String> = self
            .mcp_editor_state
            .editing_args
            .split_whitespace()
            .map(String::from)
            .collect();
        let mut env = std::collections::HashMap::new();

        for pair in self.mcp_editor_state.editing_env.split(',') {
            if let Some((k, v)) = pair.split_once('=') {
                env.insert(k.trim().to_string(), v.trim().to_string());
            }
        }

        if name.is_empty() || command.is_empty() {
            self.set_status("Name and Command/URL are required".to_string());
            return;
        }

        let config = if command.starts_with("http://") || command.starts_with("https://") {
            let auth = self.build_auth_config();
            let requires_oauth = auth.requires_oauth();

            let config = McpServerConfig::Sse { url: command, auth };

            self.paths
                .preferences
                .global_prefs
                .mcp_servers
                .insert(name.clone(), config);

            let _ = self.paths.preferences.save_global();
            self.update_mcp_list();
            self.mode = AppMode::Normal;

            if requires_oauth {
                self.set_status(format!(
                    "Saved MCP server: {} - Press 'o' to authenticate",
                    name
                ));
            } else {
                self.set_status(format!("Saved MCP server: {} (syncs to all agents)", name));
            }
            return;
        } else {
            McpServerConfig::Stdio { command, args, env }
        };

        self.paths
            .preferences
            .global_prefs
            .mcp_servers
            .insert(name.clone(), config);

        let _ = self.paths.preferences.save_global();
        self.update_mcp_list();
        self.mode = AppMode::Normal;
        self.set_status(format!("Saved MCP server: {} (syncs to all agents)", name));
    }

    fn build_auth_config(&self) -> McpAuth {
        match self.mcp_editor_state.editing_auth_type {
            McpAuthType::None => McpAuth::None,
            McpAuthType::Bearer => {
                let token = self.mcp_editor_state.editing_bearer_token.trim();
                if token.is_empty() {
                    McpAuth::None
                } else {
                    McpAuth::Bearer {
                        token: token.to_string(),
                    }
                }
            }
            McpAuthType::OAuth => {
                let client_id = self.mcp_editor_state.editing_oauth_client_id.trim();
                if client_id.is_empty() {
                    McpAuth::None
                } else {
                    let client_secret = {
                        let s = self.mcp_editor_state.editing_oauth_client_secret.trim();
                        if s.is_empty() {
                            None
                        } else {
                            Some(s.to_string())
                        }
                    };
                    let auth_server_url = {
                        let s = self.mcp_editor_state.editing_oauth_auth_server_url.trim();
                        if s.is_empty() {
                            None
                        } else {
                            Some(s.to_string())
                        }
                    };
                    let scopes: Vec<String> = self
                        .mcp_editor_state
                        .editing_oauth_scopes
                        .split_whitespace()
                        .map(String::from)
                        .collect();

                    McpAuth::OAuth {
                        client_id: client_id.to_string(),
                        client_secret,
                        auth_server_url,
                        scopes,
                    }
                }
            }
        }
    }

    pub fn magic_mcp_setup(&mut self) {
        let defaults = vec![
            (
                "filesystem",
                "npx",
                vec!["-y", "@modelcontextprotocol/server-filesystem", "."],
            ),
            (
                "memory",
                "npx",
                vec!["-y", "@modelcontextprotocol/server-memory"],
            ),
            ("filesystem-uvx", "uvx", vec!["mcp-server-filesystem", "."]),
            ("memory-uvx", "uvx", vec!["mcp-server-memory"]),
        ];

        let mut added_count = 0;
        let mcp_servers = &mut self.paths.preferences.global_prefs.mcp_servers;

        for (name, cmd, args) in &defaults {
            if !mcp_servers.contains_key(*name) {
                mcp_servers.insert(
                    name.to_string(),
                    McpServerConfig::Stdio {
                        command: cmd.to_string(),
                        args: args.iter().map(|s| s.to_string()).collect(),
                        env: std::collections::HashMap::new(),
                    },
                );
                added_count += 1;
            }
        }

        if !mcp_servers.contains_key("mooagent")
            && let Some(mooagent_path) = dirs::home_dir()
                .map(|h| h.join(".local/bin/mooagent"))
                .filter(|p| p.exists())
        {
            mcp_servers.insert(
                "mooagent".to_string(),
                McpServerConfig::Stdio {
                    command: mooagent_path.to_string_lossy().to_string(),
                    args: vec!["--mcp".to_string()],
                    env: std::collections::HashMap::new(),
                },
            );
            added_count += 1;
        }

        if added_count > 0 {
            let _ = self.paths.preferences.save_global();
            self.update_mcp_list();
            self.set_status(format!(
                "Added {} default MCP servers (sync to apply to all agents)",
                added_count
            ));
        } else {
            self.set_status("All default MCP servers already configured".to_string());
        }
    }

    pub fn mcp_cancel(&mut self) {
        self.mode = AppMode::Normal;
    }

    pub fn mcp_next_field(&mut self) {
        let is_remote = self.mcp_editor_state.is_remote_server();
        let auth_type = self.mcp_editor_state.editing_auth_type;

        self.mcp_editor_state.focus = match self.mcp_editor_state.focus {
            McpFieldFocus::Name => McpFieldFocus::Command,
            McpFieldFocus::Command => {
                if is_remote {
                    McpFieldFocus::AuthType
                } else {
                    McpFieldFocus::Args
                }
            }
            McpFieldFocus::Args => McpFieldFocus::Env,
            McpFieldFocus::Env => McpFieldFocus::Name,
            McpFieldFocus::AuthType => match auth_type {
                McpAuthType::None => McpFieldFocus::Name,
                McpAuthType::Bearer => McpFieldFocus::BearerToken,
                McpAuthType::OAuth => McpFieldFocus::OAuthClientId,
            },
            McpFieldFocus::BearerToken => McpFieldFocus::Name,
            McpFieldFocus::OAuthClientId => McpFieldFocus::OAuthClientSecret,
            McpFieldFocus::OAuthClientSecret => McpFieldFocus::OAuthScopes,
            McpFieldFocus::OAuthScopes => McpFieldFocus::OAuthAuthServerUrl,
            McpFieldFocus::OAuthAuthServerUrl => McpFieldFocus::Name,
        };
    }

    pub fn mcp_cycle_auth_type(&mut self, forward: bool) {
        if self.mcp_editor_state.focus == McpFieldFocus::AuthType {
            self.mcp_editor_state.editing_auth_type = if forward {
                self.mcp_editor_state.editing_auth_type.next()
            } else {
                self.mcp_editor_state.editing_auth_type.prev()
            };
        }
    }

    pub fn mcp_input_char(&mut self, c: char) {
        match self.mcp_editor_state.focus {
            McpFieldFocus::Name => {
                if self.mcp_editor_state.is_new {
                    self.mcp_editor_state.editing_name.push(c)
                }
            }
            McpFieldFocus::Command => self.mcp_editor_state.editing_command.push(c),
            McpFieldFocus::Args => self.mcp_editor_state.editing_args.push(c),
            McpFieldFocus::Env => self.mcp_editor_state.editing_env.push(c),
            McpFieldFocus::AuthType => {
                if c == ' ' || c == 'l' || c == 'j' {
                    self.mcp_cycle_auth_type(true);
                } else if c == 'h' || c == 'k' {
                    self.mcp_cycle_auth_type(false);
                }
            }
            McpFieldFocus::BearerToken => {
                self.mcp_editor_state.editing_bearer_token.push(c);
            }
            McpFieldFocus::OAuthClientId => {
                self.mcp_editor_state.editing_oauth_client_id.push(c);
            }
            McpFieldFocus::OAuthClientSecret => {
                self.mcp_editor_state.editing_oauth_client_secret.push(c);
            }
            McpFieldFocus::OAuthScopes => {
                self.mcp_editor_state.editing_oauth_scopes.push(c);
            }
            McpFieldFocus::OAuthAuthServerUrl => {
                self.mcp_editor_state.editing_oauth_auth_server_url.push(c);
            }
        }
    }

    pub fn mcp_backspace(&mut self) {
        match self.mcp_editor_state.focus {
            McpFieldFocus::Name => {
                if self.mcp_editor_state.is_new {
                    let _ = self.mcp_editor_state.editing_name.pop();
                }
            }
            McpFieldFocus::Command => {
                let _ = self.mcp_editor_state.editing_command.pop();
            }
            McpFieldFocus::Args => {
                let _ = self.mcp_editor_state.editing_args.pop();
            }
            McpFieldFocus::Env => {
                let _ = self.mcp_editor_state.editing_env.pop();
            }
            McpFieldFocus::AuthType => {}
            McpFieldFocus::BearerToken => {
                let _ = self.mcp_editor_state.editing_bearer_token.pop();
            }
            McpFieldFocus::OAuthClientId => {
                let _ = self.mcp_editor_state.editing_oauth_client_id.pop();
            }
            McpFieldFocus::OAuthClientSecret => {
                let _ = self.mcp_editor_state.editing_oauth_client_secret.pop();
            }
            McpFieldFocus::OAuthScopes => {
                let _ = self.mcp_editor_state.editing_oauth_scopes.pop();
            }
            McpFieldFocus::OAuthAuthServerUrl => {
                let _ = self.mcp_editor_state.editing_oauth_auth_server_url.pop();
            }
        }
    }

    pub fn sync(&mut self) -> Result<()> {
        let agent_sync_result = self.paths.sync();
        let pref_sync_result = self.paths.sync_preferences();

        match (agent_sync_result, pref_sync_result) {
            (Ok(agent_msg), Ok(pref_msg)) => {
                self.set_status(format!("{} | {}", agent_msg, pref_msg));
                self.refresh();
                Ok(())
            }
            (Err(e), _) => {
                self.set_status(format!("Agent Sync Error: {}", e));
                Err(e)
            }
            (_, Err(e)) => {
                self.set_status(format!("Pref Sync Error: {}", e));
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
            let current_pos = self
                .filtered_agents
                .iter()
                .position(|&i| i == self.selected_agent)
                .unwrap_or(0);
            let next_pos = (current_pos + 1) % self.filtered_agents.len();
            self.selected_agent = self.filtered_agents[next_pos];
        }
    }

    pub fn prev_agent(&mut self) {
        if !self.filtered_agents.is_empty() {
            let current_pos = self
                .filtered_agents
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
        let line_count = self.global_content.lines().count();
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
                self.global_scroll = self.global_content.lines().count().saturating_sub(1);
            }
            Focus::Project => {
                self.project_scroll = self.project_content.lines().count().saturating_sub(1);
            }
        }
    }

    pub fn toggle_auto_sync(&mut self) {
        self.auto_sync = !self.auto_sync;
        let status = if self.auto_sync {
            "enabled"
        } else {
            "disabled"
        };
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
            self.filtered_agents = self
                .agents
                .iter()
                .enumerate()
                .filter(|(_, agent)| {
                    agent
                        .name
                        .to_lowercase()
                        .contains(&self.search_query.to_lowercase())
                        || agent
                            .target_path
                            .to_string_lossy()
                            .to_lowercase()
                            .contains(&self.search_query.to_lowercase())
                })
                .map(|(i, _)| i)
                .collect();
        }

        if !self.filtered_agents.is_empty() && !self.filtered_agents.contains(&self.selected_agent)
        {
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

    pub fn sync_preferences(&mut self) -> Result<()> {
        match self.paths.sync_preferences() {
            Ok(msg) => {
                self.set_status(msg);
                self.refresh();
                Ok(())
            }
            Err(e) => {
                self.set_status(format!("Error syncing preferences: {}", e));
                Err(e)
            }
        }
    }

    pub fn pref_next_focus(&mut self) {
        self.pref_editor_state.focus = match self.pref_editor_state.focus {
            PrefEditorFocus::Presets => PrefEditorFocus::IndividualTools,
            PrefEditorFocus::IndividualTools => PrefEditorFocus::GeneralSettings,
            PrefEditorFocus::GeneralSettings => PrefEditorFocus::Presets,
        };
    }

    pub fn pref_scroll_down(&mut self) {
        match self.pref_editor_state.focus {
            PrefEditorFocus::Presets => {
                if self.pref_editor_state.selected_preset
                    < self.pref_editor_state.preset_list.len().saturating_sub(1)
                {
                    self.pref_editor_state.selected_preset += 1;
                }
            }
            PrefEditorFocus::IndividualTools => {
                if self.pref_editor_state.selected_tool
                    < self
                        .pref_editor_state
                        .individual_tool_list
                        .len()
                        .saturating_sub(1)
                {
                    self.pref_editor_state.selected_tool += 1;
                }
            }
            PrefEditorFocus::GeneralSettings => {
                if self.pref_editor_state.selected_general < 2 {
                    self.pref_editor_state.selected_general += 1;
                }
            }
        }
    }

    pub fn pref_scroll_up(&mut self) {
        match self.pref_editor_state.focus {
            PrefEditorFocus::Presets => {
                if self.pref_editor_state.selected_preset > 0 {
                    self.pref_editor_state.selected_preset -= 1;
                }
            }
            PrefEditorFocus::IndividualTools => {
                if self.pref_editor_state.selected_tool > 0 {
                    self.pref_editor_state.selected_tool -= 1;
                }
            }
            PrefEditorFocus::GeneralSettings => {
                if self.pref_editor_state.selected_general > 0 {
                    self.pref_editor_state.selected_general -= 1;
                }
            }
        }
    }

    pub fn pref_toggle_item(&mut self) {
        let mgr = &mut self.paths.preferences;

        match self.pref_editor_state.focus {
            PrefEditorFocus::Presets => {
                if let Some(preset_name) = self
                    .pref_editor_state
                    .preset_list
                    .get(self.pref_editor_state.selected_preset)
                {
                    if let Some(group) = mgr.global_prefs.tool_presets.get_mut(preset_name) {
                        group.enabled = !group.enabled;
                    } else {
                        mgr.global_prefs.tool_presets.insert(
                            preset_name.clone(),
                            crate::preferences::PresetGroup { enabled: true },
                        );
                    }
                }
            }
            PrefEditorFocus::IndividualTools => {
                if let Some(tool_name) = self
                    .pref_editor_state
                    .individual_tool_list
                    .get(self.pref_editor_state.selected_tool)
                {
                    let current = *mgr
                        .global_prefs
                        .individual_tools
                        .get(tool_name)
                        .unwrap_or(&false);
                    mgr.global_prefs
                        .individual_tools
                        .insert(tool_name.clone(), !current);
                }
            }
            PrefEditorFocus::GeneralSettings => match self.pref_editor_state.selected_general {
                0 => {
                    let current = mgr.global_prefs.general.auto_accept_tools.unwrap_or(true);
                    mgr.global_prefs.general.auto_accept_tools = Some(!current);
                }
                1 => {
                    let current = mgr.global_prefs.general.enable_logging.unwrap_or(true);
                    mgr.global_prefs.general.enable_logging = Some(!current);
                }
                2 => {
                    let current = mgr.global_prefs.general.sandboxed_mode.unwrap_or(true);
                    mgr.global_prefs.general.sandboxed_mode = Some(!current);
                }
                _ => {}
            },
        }

        let _ = mgr.save_global();
        self.refresh();
    }

    pub fn add_tool_char(&mut self, c: char) {
        self.new_tool_input.push(c);
    }

    pub fn backspace_add_tool(&mut self) {
        self.new_tool_input.pop();
    }

    pub fn submit_new_tool(&mut self) {
        if !self.new_tool_input.trim().is_empty() {
            let tool_name = self.new_tool_input.trim().to_string();

            if !self
                .pref_editor_state
                .individual_tool_list
                .contains(&tool_name)
            {
                self.pref_editor_state
                    .individual_tool_list
                    .push(tool_name.clone());
                self.pref_editor_state.individual_tool_list.sort();

                if let Some(pos) = self
                    .pref_editor_state
                    .individual_tool_list
                    .iter()
                    .position(|t| t == &tool_name)
                {
                    self.pref_editor_state.selected_tool = pos;
                }

                self.paths
                    .preferences
                    .global_prefs
                    .individual_tools
                    .insert(tool_name, true);
                let _ = self.paths.preferences.save_global();
            }
        }
        self.new_tool_input.clear();
        self.mode = AppMode::Normal;
    }

    pub fn cancel_add_tool(&mut self) {
        self.new_tool_input.clear();
        self.mode = AppMode::Normal;
    }

    pub fn get_selected_mcp_oauth_status(&self) -> Option<(TokenStatus, Option<String>)> {
        if self.mcp_editor_state.server_list.is_empty() {
            return None;
        }

        let server_name =
            &self.mcp_editor_state.server_list[self.mcp_editor_state.selected_server_idx];

        let config = self
            .paths
            .preferences
            .global_prefs
            .mcp_servers
            .get(server_name)?;

        let url = match config {
            McpServerConfig::Sse {
                url,
                auth: McpAuth::OAuth { .. },
            } => url,
            McpServerConfig::Http {
                http_url,
                auth: McpAuth::OAuth { .. },
            } => http_url,
            _ => return None,
        };

        let status = self.credentials.token_status(url);
        Some((status, Some(url.clone())))
    }

    pub fn mcp_requires_oauth(&self) -> bool {
        if self.mcp_editor_state.server_list.is_empty() {
            return false;
        }

        let server_name =
            &self.mcp_editor_state.server_list[self.mcp_editor_state.selected_server_idx];

        self.paths
            .preferences
            .global_prefs
            .mcp_servers
            .get(server_name)
            .map(|c| c.requires_oauth())
            .unwrap_or(false)
    }

    pub fn mcp_oauth_logout(&mut self) {
        let Some((_, Some(url))) = self.get_selected_mcp_oauth_status() else {
            self.set_status("Selected server does not use OAuth".to_string());
            return;
        };

        match self.credentials.remove_token(&url) {
            Ok(Some(_)) => {
                self.set_status(
                    "OAuth token removed. Run sync to update agent configs.".to_string(),
                );
            }
            Ok(None) => {
                self.set_status("No OAuth token stored for this server".to_string());
            }
            Err(e) => {
                self.set_status(format!("Failed to remove token: {}", e));
            }
        }
    }

    pub fn get_mcp_oauth_config(&self) -> Option<OAuthFlowConfig> {
        if self.mcp_editor_state.server_list.is_empty() {
            return None;
        }

        let server_name =
            &self.mcp_editor_state.server_list[self.mcp_editor_state.selected_server_idx];

        let config = self
            .paths
            .preferences
            .global_prefs
            .mcp_servers
            .get(server_name)?;

        match config {
            McpServerConfig::Sse {
                url,
                auth:
                    McpAuth::OAuth {
                        client_id,
                        client_secret,
                        scopes,
                        auth_server_url,
                    },
            } => Some(OAuthFlowConfig {
                server_url: url.clone(),
                client_id: client_id.clone(),
                client_secret: client_secret.clone(),
                scopes: scopes.clone(),
                auth_server_url: auth_server_url.clone(),
            }),
            McpServerConfig::Http {
                http_url,
                auth:
                    McpAuth::OAuth {
                        client_id,
                        client_secret,
                        scopes,
                        auth_server_url,
                    },
            } => Some(OAuthFlowConfig {
                server_url: http_url.clone(),
                client_id: client_id.clone(),
                client_secret: client_secret.clone(),
                scopes: scopes.clone(),
                auth_server_url: auth_server_url.clone(),
            }),
            _ => None,
        }
    }

    pub fn store_oauth_token(&mut self, url: &str, token: crate::credentials::StoredToken) {
        match self.credentials.store_token(url, token) {
            Ok(()) => {
                self.set_status(
                    "OAuth login successful! Run sync to update agent configs.".to_string(),
                );
            }
            Err(e) => {
                self.set_status(format!("Failed to store token: {}", e));
            }
        }
    }
}

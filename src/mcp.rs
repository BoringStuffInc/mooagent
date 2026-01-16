use crate::config::ConfigPaths;
use crate::credentials::{CredentialManager, TokenStatus};
use crate::oauth;
use crate::preferences::{McpAuth, McpServerConfig};
use anyhow::Result;
use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

pub fn run_mcp_server() -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());

    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let error_response = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: Value::Null,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                    }),
                };
                writeln!(stdout, "{}", serde_json::to_string(&error_response)?)?;
                stdout.flush()?;
                continue;
            }
        };

        if let Some(response) = handle_request(&request) {
            writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
            stdout.flush()?;
        }
    }

    Ok(())
}

fn handle_request(request: &JsonRpcRequest) -> Option<JsonRpcResponse> {
    let id = request.id.clone().unwrap_or(Value::Null);

    match request.method.as_str() {
        "initialize" => Some(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "mooagent",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            error: None,
        }),

        "notifications/initialized" => None,

        "tools/list" => Some(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(json!({
                "tools": get_tools_list()
            })),
            error: None,
        }),

        "tools/call" => {
            let tool_name = request.params.get("name").and_then(|v| v.as_str());
            let arguments = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or(json!({}));

            match tool_name {
                Some(name) => match call_tool(name, arguments) {
                    Ok(result) => Some(JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id,
                        result: Some(json!({
                            "content": [{
                                "type": "text",
                                "text": result
                            }]
                        })),
                        error: None,
                    }),
                    Err(e) => Some(JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id,
                        result: Some(json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Error: {}", e)
                            }],
                            "isError": true
                        })),
                        error: None,
                    }),
                },
                None => Some(JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32602,
                        message: "Missing tool name".to_string(),
                    }),
                }),
            }
        }

        method if method.starts_with("notifications/") => None,

        _ => Some(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", request.method),
            }),
        }),
    }
}

fn parse_auth_config(arguments: &Value) -> McpAuth {
    let auth_obj = match arguments.get("auth") {
        Some(v) if v.is_object() => v,
        _ => return McpAuth::None,
    };

    let auth_type = auth_obj.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match auth_type {
        "bearer" => {
            if let Some(token) = auth_obj.get("token").and_then(|v| v.as_str()) {
                McpAuth::Bearer {
                    token: token.to_string(),
                }
            } else {
                McpAuth::None
            }
        }
        "oauth" => {
            if let Some(client_id) = auth_obj.get("client_id").and_then(|v| v.as_str()) {
                let client_secret = auth_obj
                    .get("client_secret")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let auth_server_url = auth_obj
                    .get("auth_server_url")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let scopes = auth_obj
                    .get("scopes")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                McpAuth::OAuth {
                    client_id: client_id.to_string(),
                    client_secret,
                    auth_server_url,
                    scopes,
                }
            } else {
                McpAuth::None
            }
        }
        _ => McpAuth::None,
    }
}

fn format_auth_status(
    result: &mut String,
    url: &str,
    auth: &McpAuth,
    credentials: &CredentialManager,
) {
    match auth {
        McpAuth::None => {}
        McpAuth::Bearer { .. } => {
            result.push_str("  Auth: Bearer token (static)\n");
        }
        McpAuth::OAuth {
            client_id, scopes, ..
        } => {
            let status = credentials.token_status(url);
            let status_icon = match status {
                TokenStatus::Valid => "✅",
                TokenStatus::ExpiresSoon => "⚠️",
                TokenStatus::Expired => "❌",
                TokenStatus::None => "🔒",
            };
            result.push_str(&format!(
                "  Auth: OAuth {} ({})\n",
                status_icon,
                status.description()
            ));
            result.push_str(&format!("  Client ID: {}\n", client_id));
            if !scopes.is_empty() {
                result.push_str(&format!("  Scopes: {}\n", scopes.join(" ")));
            }
        }
    }
}

fn get_tools_list() -> Vec<Value> {
    vec![
        json!({
            "name": "mcp_list",
            "description": "List all configured MCP servers. These are global and sync to all agents (Claude, Gemini, OpenCode).",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "mcp_add",
            "description": "Add a new MCP server. It will be synced to all agents on next sync.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Unique identifier for the MCP server"
                    },
                    "command": {
                        "type": "string",
                        "description": "Command to run (for local servers) or URL (for remote SSE servers)"
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Arguments for the command (only for local servers)"
                    },
                    "env": {
                        "type": "object",
                        "description": "Environment variables (only for local servers)"
                    },
                    "auth": {
                        "type": "object",
                        "description": "Authentication configuration for remote servers",
                        "properties": {
                            "type": {
                                "type": "string",
                                "enum": ["bearer", "oauth"],
                                "description": "Authentication type"
                            },
                            "token": {
                                "type": "string",
                                "description": "Bearer token (for type=bearer)"
                            },
                            "client_id": {
                                "type": "string",
                                "description": "OAuth client ID (for type=oauth)"
                            },
                            "client_secret": {
                                "type": "string",
                                "description": "OAuth client secret (optional, for type=oauth)"
                            },
                            "scopes": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "OAuth scopes (optional, for type=oauth)"
                            },
                            "auth_server_url": {
                                "type": "string",
                                "description": "OAuth authorization server URL (optional, auto-discovered if not provided)"
                            }
                        }
                    }
                },
                "required": ["name", "command"]
            }
        }),
        json!({
            "name": "mcp_remove",
            "description": "Remove an MCP server by name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the MCP server to remove"
                    }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "edit_global_rules",
            "description": "Append content to GLOBAL_RULES.md. This file is synced to all agents' global config files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "Content to append to GLOBAL_RULES.md"
                    }
                },
                "required": ["content"]
            }
        }),
        json!({
            "name": "edit_project_rules",
            "description": "Append content to AGENTS.md in the current project. This file is synced to all agents' project-specific config files.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "Content to append to AGENTS.md"
                    }
                },
                "required": ["content"]
            }
        }),
        json!({
            "name": "read_global_rules",
            "description": "Read the current content of GLOBAL_RULES.md.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "read_project_rules",
            "description": "Read the current content of AGENTS.md in the current project.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "set_global_rules",
            "description": "Replace the entire content of GLOBAL_RULES.md. Creates a backup before overwriting.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The new content to write to GLOBAL_RULES.md"
                    }
                },
                "required": ["content"]
            }
        }),
        json!({
            "name": "set_project_rules",
            "description": "Replace the entire content of AGENTS.md in the current project. Creates a backup before overwriting.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "description": "The new content to write to AGENTS.md"
                    }
                },
                "required": ["content"]
            }
        }),
        json!({
            "name": "edit_section_global_rules",
            "description": "Edit a specific markdown section in GLOBAL_RULES.md by heading. Supports replace, append, prepend, or delete operations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "section_heading": {
                        "type": "string",
                        "description": "The markdown heading to find (e.g., '## Coding Style', '# Rules')"
                    },
                    "content": {
                        "type": "string",
                        "description": "The new content for the section (ignored for delete action)"
                    },
                    "action": {
                        "type": "string",
                        "enum": ["replace", "append", "prepend", "delete"],
                        "description": "The action to perform: replace (default), append, prepend, or delete"
                    }
                },
                "required": ["section_heading"]
            }
        }),
        json!({
            "name": "edit_section_project_rules",
            "description": "Edit a specific markdown section in AGENTS.md by heading. Supports replace, append, prepend, or delete operations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "section_heading": {
                        "type": "string",
                        "description": "The markdown heading to find (e.g., '## Coding Style', '# Rules')"
                    },
                    "content": {
                        "type": "string",
                        "description": "The new content for the section (ignored for delete action)"
                    },
                    "action": {
                        "type": "string",
                        "enum": ["replace", "append", "prepend", "delete"],
                        "description": "The action to perform: replace (default), append, prepend, or delete"
                    }
                },
                "required": ["section_heading"]
            }
        }),
        json!({
            "name": "list_sections_global_rules",
            "description": "List all markdown sections (headings) in GLOBAL_RULES.md. Useful for discovering sections before editing.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "list_sections_project_rules",
            "description": "List all markdown sections (headings) in AGENTS.md. Useful for discovering sections before editing.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "sync",
            "description": "Sync all configurations to all agents. This includes rules and MCP servers.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "sync_preview",
            "description": "Preview what sync would do without making changes (dry-run). Shows files that would be created or modified.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "get_status",
            "description": "Get sync status for all agents.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "bootstrap",
            "description": "Bootstrap mooagent MCP server to all agents. This adds mooagent itself as an MCP server and syncs, so all agents can use mooagent tools.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "test_mcp_server",
            "description": "Test connectivity to an MCP server. For remote servers (SSE/HTTP), pings the endpoint. For local servers (stdio), checks if the command exists.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the MCP server to test"
                    }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "oauth_status",
            "description": "Get OAuth authentication status for an MCP server.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the MCP server to check"
                    }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "oauth_login",
            "description": "Initiate OAuth login flow for an MCP server. Opens browser for authentication.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the MCP server to authenticate"
                    }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "oauth_logout",
            "description": "Remove stored OAuth token for an MCP server.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the MCP server to logout from"
                    }
                },
                "required": ["name"]
            }
        }),
        json!({
            "name": "export_config",
            "description": "Export all mooagent configuration (MCP servers, preferences, tool permissions) to JSON. Useful for backup or sharing.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": []
            }
        }),
        json!({
            "name": "import_config",
            "description": "Import mooagent configuration from JSON. Merges with existing config (use 'replace: true' to overwrite).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "config": {
                        "type": "string",
                        "description": "JSON string containing the configuration to import"
                    },
                    "replace": {
                        "type": "boolean",
                        "description": "If true, replace existing config instead of merging (default: false)"
                    }
                },
                "required": ["config"]
            }
        }),
    ]
}

fn edit_markdown_section(content: &str, heading: &str, new_content: &str, action: &str) -> Result<String> {
    let lines: Vec<&str> = content.lines().collect();
    let heading_level = heading.chars().take_while(|&c| c == '#').count();

    if heading_level == 0 {
        anyhow::bail!("Invalid heading format: must start with #");
    }

    let mut section_start: Option<usize> = None;
    let mut section_end: Option<usize> = None;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed == heading || trimmed.starts_with(&format!("{} ", heading)) {
            section_start = Some(idx);
            continue;
        }

        if section_start.is_some() && section_end.is_none() {
            let line_level = trimmed.chars().take_while(|&c| c == '#').count();
            if line_level > 0 && line_level <= heading_level {
                section_end = Some(idx);
                break;
            }
        }
    }

    let Some(start) = section_start else {
        anyhow::bail!("Section '{}' not found", heading);
    };

    let end = section_end.unwrap_or(lines.len());

    let before: Vec<&str> = lines[..start].to_vec();
    let section_content: Vec<&str> = lines[start..end].to_vec();
    let after: Vec<&str> = lines[end..].to_vec();

    let new_section = match action {
        "delete" => String::new(),
        "replace" => format!("{}\n{}", heading, new_content),
        "append" => {
            let existing = section_content.join("\n");
            format!("{}\n\n{}", existing, new_content)
        }
        "prepend" => {
            let existing_body = section_content[1..].join("\n");
            format!("{}\n{}\n\n{}", heading, new_content, existing_body)
        }
        _ => anyhow::bail!("Unknown action: {}. Use replace, append, prepend, or delete", action),
    };

    let mut result = before.join("\n");
    if !result.is_empty() && !new_section.is_empty() {
        result.push('\n');
    }
    result.push_str(&new_section);
    if !after.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&after.join("\n"));
    }

    Ok(result)
}

fn list_markdown_sections(content: &str) -> Vec<(String, usize)> {
    content
        .lines()
        .enumerate()
        .filter_map(|(line_num, line)| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                let level = trimmed.chars().take_while(|&c| c == '#').count();
                let heading = trimmed.trim_start_matches('#').trim();
                if !heading.is_empty() {
                    return Some((format!("{} {}", "#".repeat(level), heading), line_num + 1));
                }
            }
            None
        })
        .collect()
}

fn backup_file(path: &std::path::Path, backup_dir: &std::path::Path) -> Result<()> {
    if path.exists() {
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
        let backup_name = format!("{}_{}", filename, timestamp);
        let backup = backup_dir.join(backup_name);
        std::fs::create_dir_all(backup_dir)?;
        std::fs::copy(path, &backup)?;
    }
    Ok(())
}

fn call_tool(name: &str, arguments: Value) -> Result<String> {
    let mut paths = ConfigPaths::new()?;

    // Check permissions
    let merged = paths.preferences.get_merged();
    let enabled_tools = crate::preferences::expand_tools(&merged);

    // 1. Check individual_tools/presets (explicitly disabled)
    if enabled_tools.get(name) == Some(&false) {
        return Err(anyhow::anyhow!(
            "Tool '{}' is disabled by configuration.\n\nTo enable:\n  1. Run mooagent TUI and go to Preferences tab (press 2)\n  2. Or edit ~/.config/mooagent/preferences.toml",
            name
        ));
    }

    // 2. Check disabled_tools in 'mooagent' MCP config
    if let Some(config) = merged.mcp_servers.get("mooagent")
        && config.disabled_tools().contains(&name.to_string())
    {
        return Err(anyhow::anyhow!(
            "Tool '{}' is disabled in mooagent MCP server config.\n\nTo enable: remove '{}' from disabled_tools in the mooagent MCP configuration.",
            name, name
        ));
    }

    match name {
        "mcp_list" => {
            let merged = paths.preferences.get_merged();
            let servers = &merged.mcp_servers;
            if servers.is_empty() {
                return Ok("No MCP servers configured.".to_string());
            }

            let mut credentials = CredentialManager::new(&paths.config_dir);
            let _ = credentials.load();

            let mut result = String::from("Configured MCP servers (Effective):\n\n");
            for (name, config) in servers {
                result.push_str(&format!("- **{}**\n", name));
                match config {
                    McpServerConfig::Stdio { command, args, env, .. } => {
                        result.push_str("  Type: local (stdio)\n");
                        result.push_str(&format!("  Command: {}\n", command));
                        if !args.is_empty() {
                            result.push_str(&format!("  Args: {}\n", args.join(" ")));
                        }
                        if !env.is_empty() {
                            result.push_str("  Env:\n");
                            for (k, v) in env {
                                result.push_str(&format!("    {}={}\n", k, v));
                            }
                        }
                    }
                    McpServerConfig::Sse { url, auth, .. } => {
                        result.push_str("  Type: remote (SSE)\n");
                        result.push_str(&format!("  URL: {}\n", url));
                        format_auth_status(&mut result, url, auth, &credentials);
                    }
                    McpServerConfig::Http { http_url, auth, .. } => {
                        result.push_str("  Type: remote (HTTP)\n");
                        result.push_str(&format!("  URL: {}\n", http_url));
                        format_auth_status(&mut result, http_url, auth, &credentials);
                    }
                }
                result.push('\n');
            }
            Ok(result)
        }

        "mcp_add" => {
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?;
            let command = arguments
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'command' argument"))?;

            let mut paths = paths;

            let config = if command.starts_with("http://") || command.starts_with("https://") {
                let auth = parse_auth_config(&arguments);
                McpServerConfig::Sse {
                    url: command.to_string(),
                    auth,
                    disabled_tools: Vec::new(),
                    auto_allow: false,
                }
            } else {
                let args: Vec<String> = arguments
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                let env: HashMap<String, String> = arguments
                    .get("env")
                    .and_then(|v| v.as_object())
                    .map(|obj| {
                        obj.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect()
                    })
                    .unwrap_or_default();

                McpServerConfig::Stdio {
                    command: command.to_string(),
                    args,
                    env,
                    disabled_tools: Vec::new(),
                    auto_allow: false,
                }
            };

            let has_oauth = config.requires_oauth();

            paths
                .preferences
                .global_prefs
                .mcp_servers
                .insert(name.to_string(), config);
            paths.preferences.save_global()?;

            if has_oauth {
                Ok(format!(
                    "Added MCP server '{}' with OAuth. Run 'oauth_login' to authenticate, then 'sync' to apply.",
                    name
                ))
            } else {
                Ok(format!(
                    "Added MCP server '{}'. Run 'sync' to apply to all agents.",
                    name
                ))
            }
        }

        "mcp_remove" => {
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?;

            let mut paths = paths;

            if paths
                .preferences
                .global_prefs
                .mcp_servers
                .remove(name)
                .is_some()
            {
                paths.preferences.save_global()?;
                Ok(format!(
                    "Removed MCP server '{}'. Run 'sync' to apply to all agents.",
                    name
                ))
            } else {
                Ok(format!("MCP server '{}' not found.", name))
            }
        }

        "edit_global_rules" => {
            let content = arguments
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;

            let current = if paths.global_rules_primary.exists() {
                std::fs::read_to_string(&paths.global_rules_primary)?
            } else {
                String::new()
            };

            let new_content = if current.is_empty() {
                content.to_string()
            } else {
                format!("{}\n\n{}", current.trim_end(), content)
            };

            std::fs::write(&paths.global_rules_primary, new_content)?;
            Ok("Updated GLOBAL_RULES.md. Run 'sync' to propagate to all agents.".to_string())
        }

        "edit_project_rules" => {
            let content = arguments
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;

            let current = if paths.project_agents.exists() {
                std::fs::read_to_string(&paths.project_agents)?
            } else {
                String::new()
            };

            let new_content = if current.is_empty() {
                content.to_string()
            } else {
                format!("{}\n\n{}", current.trim_end(), content)
            };

            std::fs::write(&paths.project_agents, new_content)?;
            Ok("Updated AGENTS.md. Run 'sync' to propagate to all agents.".to_string())
        }

        "read_global_rules" => {
            if paths.global_rules_primary.exists() {
                let content = std::fs::read_to_string(&paths.global_rules_primary)?;
                Ok(format!(
                    "GLOBAL_RULES.md ({}):\n\n{}",
                    paths.global_rules_primary.display(),
                    content
                ))
            } else {
                Ok("GLOBAL_RULES.md does not exist yet.".to_string())
            }
        }

        "read_project_rules" => {
            if paths.project_agents.exists() {
                let content = std::fs::read_to_string(&paths.project_agents)?;
                Ok(format!(
                    "AGENTS.md ({}):\n\n{}",
                    paths.project_agents.display(),
                    content
                ))
            } else {
                Ok("AGENTS.md does not exist in current project.".to_string())
            }
        }

        "set_global_rules" => {
            let content = arguments
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;

            backup_file(&paths.global_rules_primary, &paths.backup_dir)?;
            std::fs::write(&paths.global_rules_primary, content)?;
            Ok("Replaced GLOBAL_RULES.md content. Run 'sync' to propagate to all agents.".to_string())
        }

        "set_project_rules" => {
            let content = arguments
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;

            backup_file(&paths.project_agents, &paths.backup_dir)?;
            std::fs::write(&paths.project_agents, content)?;
            Ok("Replaced AGENTS.md content. Run 'sync' to propagate to all agents.".to_string())
        }

        "edit_section_global_rules" => {
            let section_heading = arguments
                .get("section_heading")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'section_heading' argument"))?;

            let content = arguments
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let action = arguments
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("replace");

            if !paths.global_rules_primary.exists() {
                anyhow::bail!("GLOBAL_RULES.md does not exist");
            }

            let current = std::fs::read_to_string(&paths.global_rules_primary)?;
            let updated = edit_markdown_section(&current, section_heading, content, action)?;

            backup_file(&paths.global_rules_primary, &paths.backup_dir)?;
            std::fs::write(&paths.global_rules_primary, updated)?;

            Ok(format!(
                "Section '{}' {}. Run 'sync' to propagate to all agents.",
                section_heading,
                match action {
                    "delete" => "deleted",
                    "append" => "appended",
                    "prepend" => "prepended",
                    _ => "replaced",
                }
            ))
        }

        "edit_section_project_rules" => {
            let section_heading = arguments
                .get("section_heading")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'section_heading' argument"))?;

            let content = arguments
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let action = arguments
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("replace");

            if !paths.project_agents.exists() {
                anyhow::bail!("AGENTS.md does not exist in current project");
            }

            let current = std::fs::read_to_string(&paths.project_agents)?;
            let updated = edit_markdown_section(&current, section_heading, content, action)?;

            backup_file(&paths.project_agents, &paths.backup_dir)?;
            std::fs::write(&paths.project_agents, updated)?;

            Ok(format!(
                "Section '{}' {}. Run 'sync' to propagate to all agents.",
                section_heading,
                match action {
                    "delete" => "deleted",
                    "append" => "appended",
                    "prepend" => "prepended",
                    _ => "replaced",
                }
            ))
        }

        "list_sections_global_rules" => {
            if !paths.global_rules_primary.exists() {
                return Ok("GLOBAL_RULES.md does not exist yet.".to_string());
            }

            let content = std::fs::read_to_string(&paths.global_rules_primary)?;
            let sections = list_markdown_sections(&content);

            if sections.is_empty() {
                return Ok("No sections found in GLOBAL_RULES.md".to_string());
            }

            let mut result = format!("Sections in GLOBAL_RULES.md ({}):\n\n", paths.global_rules_primary.display());
            for (heading, line) in sections {
                result.push_str(&format!("  Line {}: {}\n", line, heading));
            }
            Ok(result)
        }

        "list_sections_project_rules" => {
            if !paths.project_agents.exists() {
                return Ok("AGENTS.md does not exist in current project.".to_string());
            }

            let content = std::fs::read_to_string(&paths.project_agents)?;
            let sections = list_markdown_sections(&content);

            if sections.is_empty() {
                return Ok("No sections found in AGENTS.md".to_string());
            }

            let mut result = format!("Sections in AGENTS.md ({}):\n\n", paths.project_agents.display());
            for (heading, line) in sections {
                result.push_str(&format!("  Line {}: {}\n", line, heading));
            }
            Ok(result)
        }

        "sync" => {
            let rules_result = paths.sync();
            let global_result = paths.sync_global_rules();
            let prefs_result = paths.sync_preferences();

            match (&rules_result, &global_result, &prefs_result) {
                (Ok(rules_msg), Ok(_), Ok(prefs_msg)) => {
                    Ok(format!("{}\nGlobal rules synced.\n{}", rules_msg, prefs_msg))
                }
                (Err(e), _, _) => Err(anyhow::anyhow!("Rules sync failed: {}", e)),
                (_, Err(e), _) => Err(anyhow::anyhow!("Global rules sync failed: {}", e)),
                (_, _, Err(e)) => Err(anyhow::anyhow!("Preferences sync failed: {}", e)),
            }
        }

        "sync_preview" => {
            let agents = paths.get_agents();
            let mut result = String::from("## Sync Preview (Dry Run)\n\n");

            let agents_needing_sync: Vec<_> = agents
                .iter()
                .filter(|a| a.status != crate::config::AgentStatus::Ok)
                .collect();

            if agents_needing_sync.is_empty() {
                result.push_str("### Rules\n\nAll agents already in sync.\n\n");
            } else {
                result.push_str(&format!(
                    "### Rules\n\n{} agent(s) would be synced:\n\n",
                    agents_needing_sync.len()
                ));
                for agent in &agents_needing_sync {
                    let action = match agent.status {
                        crate::config::AgentStatus::Missing => "CREATE",
                        crate::config::AgentStatus::Drift => "UPDATE",
                        crate::config::AgentStatus::Ok => "SKIP",
                    };
                    let strategy = match agent.strategy {
                        crate::config::SyncStrategy::Merge => "merge",
                        crate::config::SyncStrategy::Symlink => "symlink",
                    };
                    result.push_str(&format!(
                        "- **{}** [{}]: {} ({})\n",
                        agent.name,
                        action,
                        agent.target_path.display(),
                        strategy
                    ));
                }
                result.push('\n');
            }

            result.push_str("### Preferences\n\n");
            let mcp_count = paths.preferences.global_prefs.mcp_servers.len();
            if mcp_count > 0 {
                result.push_str(&format!(
                    "{} MCP server(s) would be synced to agent configs.\n",
                    mcp_count
                ));
            } else {
                result.push_str("No MCP servers configured.\n");
            }

            result.push_str("\n---\n\nRun `sync` to apply these changes.");
            Ok(result)
        }

        "get_status" => {
            let agents = paths.get_agents();
            let mut result = String::from("Agent Status:\n\n");

            for agent in &agents {
                let status_str = match &agent.status {
                    crate::config::AgentStatus::Ok => "✅ Synced",
                    crate::config::AgentStatus::Drift => "⚠️ Drift detected",
                    crate::config::AgentStatus::Missing => "❌ Missing",
                };
                result.push_str(&format!(
                    "- **{}**: {} ({})\n",
                    agent.name,
                    status_str,
                    agent.target_path.display()
                ));
            }

            result.push_str(&format!(
                "\nGlobal Rules: {}\n",
                paths.global_rules_primary.display()
            ));
            result.push_str(&format!(
                "Project Rules: {}\n",
                paths.project_agents.display()
            ));
            result.push_str(&format!(
                "MCP Servers: {} configured\n",
                paths.preferences.global_prefs.mcp_servers.len()
            ));

            Ok(result)
        }

        "bootstrap" => {
            let installed_path = dirs::home_dir()
                .map(|h| h.join(".local/bin/mooagent"))
                .filter(|p| p.exists());

            let mooagent_path =
                installed_path.unwrap_or_else(|| std::env::current_exe().unwrap_or_default());

            if !mooagent_path.exists() {
                return Err(anyhow::anyhow!(
                    "mooagent binary not found. Run 'just install' first to install to ~/.local/bin/"
                ));
            }

            let mut paths = paths;

            if paths
                .preferences
                .global_prefs
                .mcp_servers
                .contains_key("mooagent")
            {
                return Ok(
                    "mooagent MCP is already configured. Run 'sync' if you need to update agents."
                        .to_string(),
                );
            }

            paths.preferences.global_prefs.mcp_servers.insert(
                "mooagent".to_string(),
                McpServerConfig::Stdio {
                    command: mooagent_path.to_string_lossy().to_string(),
                    args: vec!["--mcp".to_string()],
                    env: HashMap::new(),
                    disabled_tools: Vec::new(),
                    auto_allow: false,
                },
            );
            paths.preferences.save_global()?;

            let rules_result = paths.sync();
            let prefs_result = paths.sync_preferences();

            match (rules_result, prefs_result) {
                (Ok(_), Ok(_)) => Ok(format!(
                    "Bootstrapped mooagent MCP server!\n\n\
                     Added: {}\n\
                     Synced to all agents.\n\n\
                     All agents now have access to mooagent tools:\n\
                     - mcp_list, mcp_add, mcp_remove\n\
                     - read_global_rules, read_project_rules\n\
                     - edit_global_rules, edit_project_rules (append)\n\
                     - set_global_rules, set_project_rules (replace)\n\
                     - edit_section_global_rules, edit_section_project_rules\n\
                     - list_sections_global_rules, list_sections_project_rules\n\
                     - sync, sync_preview, get_status, bootstrap\n\
                     - test_mcp_server, oauth_status, oauth_login, oauth_logout\n\
                     - export_config, import_config",
                    mooagent_path.display()
                )),
                (Err(e), _) => Err(anyhow::anyhow!(
                    "Bootstrap added config but rules sync failed: {}",
                    e
                )),
                (_, Err(e)) => Err(anyhow::anyhow!(
                    "Bootstrap added config but prefs sync failed: {}",
                    e
                )),
            }
        }

        "test_mcp_server" => {
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?;

            let server = paths
                .preferences
                .global_prefs
                .mcp_servers
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("MCP server '{}' not found.\n\nTo add: use 'mcp_add' tool or run mooagent TUI (press 3 for MCP tab, then 'a' to add).", name))?
                .clone();

            match &server {
                McpServerConfig::Stdio { command, .. } => {
                    let cmd_exists = std::process::Command::new("which")
                        .arg(command)
                        .output()
                        .map(|o| o.status.success())
                        .unwrap_or(false);

                    if cmd_exists {
                        Ok(format!(
                            "✅ Local server '{}': command '{}' found in PATH",
                            name, command
                        ))
                    } else {
                        Ok(format!(
                            "❌ Local server '{}': command '{}' NOT found in PATH",
                            name, command
                        ))
                    }
                }
                McpServerConfig::Sse { url, .. } | McpServerConfig::Http { http_url: url, .. } => {
                    let rt = tokio::runtime::Runtime::new()?;
                    let url_clone = url.clone();
                    let result: Result<(reqwest::StatusCode, Option<String>), String> = rt.block_on(async {
                        let client = reqwest::Client::builder()
                            .timeout(std::time::Duration::from_secs(10))
                            .build()
                            .map_err(|e| e.to_string())?;

                        match client.head(&url_clone).send().await {
                            Ok(resp) => Ok((resp.status(), None)),
                            Err(e) => {
                                if e.is_timeout() {
                                    Err("Connection timed out (10s)".to_string())
                                } else if e.is_connect() {
                                    Err("Connection refused".to_string())
                                } else {
                                    Ok((reqwest::StatusCode::default(), Some(e.to_string())))
                                }
                            }
                        }
                    });

                    match result {
                        Ok((status, None)) => {
                            if status.is_success() || status.is_redirection() {
                                Ok(format!(
                                    "✅ Remote server '{}' ({}) is reachable (HTTP {})",
                                    name, url, status.as_u16()
                                ))
                            } else {
                                Ok(format!(
                                    "⚠️ Remote server '{}' ({}) responded with HTTP {}",
                                    name, url, status.as_u16()
                                ))
                            }
                        }
                        Ok((_, Some(err))) => Ok(format!(
                            "❌ Remote server '{}' ({}): {}",
                            name, url, err
                        )),
                        Err(msg) => Ok(format!(
                            "❌ Remote server '{}' ({}): {}",
                            name, url, msg
                        )),
                    }
                }
            }
        }

        "oauth_status" => {
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?;

            let server = paths
                .preferences
                .global_prefs
                .mcp_servers
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("MCP server '{}' not found.\n\nTo add: use 'mcp_add' tool or run mooagent TUI (press 3 for MCP tab, then 'a' to add).", name))?;

            let (url, auth) = match server {
                McpServerConfig::Sse { url, auth, .. } => (url.as_str(), auth),
                McpServerConfig::Http { http_url, auth, .. } => (http_url.as_str(), auth),
                McpServerConfig::Stdio { .. } => {
                    return Ok(format!(
                        "MCP server '{}' is a local (stdio) server - OAuth not applicable.",
                        name
                    ));
                }
            };

            match auth {
                McpAuth::None => Ok(format!(
                    "MCP server '{}' has no authentication configured.",
                    name
                )),
                McpAuth::Bearer { .. } => {
                    Ok(format!("MCP server '{}' uses a static bearer token.", name))
                }
                McpAuth::OAuth {
                    client_id, scopes, ..
                } => {
                    let mut credentials = CredentialManager::new(&paths.config_dir);
                    let _ = credentials.load();

                    let status = credentials.token_status(url);
                    let mut result = format!("MCP server '{}' OAuth status:\n\n", name);
                    result.push_str(&format!("  Client ID: {}\n", client_id));
                    if !scopes.is_empty() {
                        result.push_str(&format!("  Scopes: {}\n", scopes.join(" ")));
                    }
                    result.push_str(&format!(
                        "  Status: {} {}\n",
                        status.symbol(),
                        status.description()
                    ));

                    if let Some(token) = credentials.get_token(url)
                        && let Some(expires) = token.expires_at
                    {
                        result.push_str(&format!("  Expires: {}\n", expires));
                    }

                    Ok(result)
                }
            }
        }

        "oauth_login" => {
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?;

            let server = paths
                .preferences
                .global_prefs
                .mcp_servers
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("MCP server '{}' not found.\n\nTo add: use 'mcp_add' tool or run mooagent TUI (press 3 for MCP tab, then 'a' to add).", name))?
                .clone();

            let (url, auth) = match &server {
                McpServerConfig::Sse { url, auth, .. } => (url.clone(), auth.clone()),
                McpServerConfig::Http { http_url, auth, .. } => (http_url.clone(), auth.clone()),
                McpServerConfig::Stdio { .. } => {
                    return Err(anyhow::anyhow!(
                        "MCP server '{}' is a local (stdio) server - OAuth not applicable.",
                        name
                    ));
                }
            };

            match auth {
                McpAuth::OAuth {
                    client_id,
                    client_secret,
                    auth_server_url,
                    scopes,
                } => {
                    let rt = tokio::runtime::Runtime::new()?;
                    let token = rt.block_on(oauth::run_oauth_flow(
                        &url,
                        &client_id,
                        client_secret.as_deref(),
                        scopes,
                        auth_server_url.as_deref(),
                    ))?;

                    let mut credentials = CredentialManager::new(&paths.config_dir);
                    let _ = credentials.load();
                    credentials.store_token(&url, token)?;

                    match paths.sync_preferences() {
                        Ok(msg) => Ok(format!(
                            "Successfully authenticated for '{}' and synced to agents.\n\n{}",
                            name, msg
                        )),
                        Err(e) => Ok(format!(
                            "Successfully authenticated for '{}', but sync failed: {}",
                            name, e
                        )),
                    }
                }
                McpAuth::None => Err(anyhow::anyhow!(
                    "MCP server '{}' has no authentication configured. Add OAuth config first.",
                    name
                )),
                McpAuth::Bearer { .. } => Err(anyhow::anyhow!(
                    "MCP server '{}' uses a static bearer token - no login needed.",
                    name
                )),
            }
        }

        "oauth_logout" => {
            let name = arguments
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?;

            let server = paths
                .preferences
                .global_prefs
                .mcp_servers
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("MCP server '{}' not found.\n\nTo add: use 'mcp_add' tool or run mooagent TUI (press 3 for MCP tab, then 'a' to add).", name))?;

            let url = match server {
                McpServerConfig::Sse { url, .. } => url.clone(),
                McpServerConfig::Http { http_url, .. } => http_url.clone(),
                McpServerConfig::Stdio { .. } => {
                    return Err(anyhow::anyhow!(
                        "MCP server '{}' is a local (stdio) server - OAuth not applicable.",
                        name
                    ));
                }
            };

            let mut credentials = CredentialManager::new(&paths.config_dir);
            let _ = credentials.load();

            if credentials.remove_token(&url)?.is_some() {
                Ok(format!(
                    "Removed OAuth token for '{}'. Run 'sync' to update agent configs.",
                    name
                ))
            } else {
                Ok(format!("No OAuth token found for '{}'.", name))
            }
        }

        "export_config" => {
            let prefs = &paths.preferences.global_prefs;
            let export = json!({
                "mcp_servers": prefs.mcp_servers,
                "tool_permissions": prefs.tool_permissions,
                "tool_presets": prefs.tool_presets,
                "individual_tools": prefs.individual_tools,
                "disabled_mcp_servers": prefs.disabled_mcp_servers,
            });

            let json_str = serde_json::to_string_pretty(&export)?;
            Ok(format!(
                "## Exported Configuration\n\n```json\n{}\n```\n\nCopy the JSON above to import into another mooagent instance.",
                json_str
            ))
        }

        "import_config" => {
            let config_str = arguments
                .get("config")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing 'config' argument"))?;

            let replace = arguments
                .get("replace")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let import: serde_json::Value = serde_json::from_str(config_str)
                .map_err(|e| anyhow::anyhow!("Invalid JSON: {}", e))?;

            let mut count = 0;

            if let Some(servers) = import.get("mcp_servers").and_then(|v| v.as_object()) {
                if replace {
                    paths.preferences.global_prefs.mcp_servers.clear();
                }
                for (name, config) in servers {
                    if let Ok(server_config) = serde_json::from_value::<McpServerConfig>(config.clone()) {
                        paths.preferences.global_prefs.mcp_servers.insert(name.clone(), server_config);
                        count += 1;
                    }
                }
            }

            if let Some(perms) = import.get("tool_permissions")
                && let Ok(tool_perms) = serde_json::from_value::<crate::preferences::ToolPermissions>(perms.clone())
            {
                if replace {
                    paths.preferences.global_prefs.tool_permissions = tool_perms;
                } else {
                    paths.preferences.global_prefs.tool_permissions.allow.extend(tool_perms.allow);
                    paths.preferences.global_prefs.tool_permissions.ask.extend(tool_perms.ask);
                    paths.preferences.global_prefs.tool_permissions.deny.extend(tool_perms.deny);
                }
            }

            if let Some(presets) = import.get("tool_presets").and_then(|v| v.as_object()) {
                for (name, config) in presets {
                    if let Ok(preset) = serde_json::from_value::<crate::preferences::PresetGroup>(config.clone()) {
                        paths.preferences.global_prefs.tool_presets.insert(name.clone(), preset);
                    }
                }
            }

            if let Some(tools) = import.get("individual_tools").and_then(|v| v.as_object()) {
                for (name, enabled) in tools {
                    if let Some(enabled) = enabled.as_bool() {
                        paths.preferences.global_prefs.individual_tools.insert(name.clone(), enabled);
                    }
                }
            }

            paths.preferences.save_global()?;

            Ok(format!(
                "Imported configuration ({} MCP servers). Run 'sync' to apply to agents.",
                count
            ))
        }

        _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
    }
}

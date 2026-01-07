use crate::config::ConfigPaths;
use crate::preferences::McpServerConfig;
use anyhow::Result;
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
            "name": "sync",
            "description": "Sync all configurations to all agents. This includes rules and MCP servers.",
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
    ]
}

fn call_tool(name: &str, arguments: Value) -> Result<String> {
    let paths = ConfigPaths::new()?;

    match name {
        "mcp_list" => {
            let servers = &paths.preferences.global_prefs.mcp_servers;
            if servers.is_empty() {
                return Ok("No MCP servers configured.".to_string());
            }

            let mut result = String::from("Configured MCP servers:\n\n");
            for (name, config) in servers {
                result.push_str(&format!("- **{}**\n", name));
                match config {
                    McpServerConfig::Stdio { command, args, env } => {
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
                    McpServerConfig::Sse { url } => {
                        result.push_str("  Type: remote (SSE)\n");
                        result.push_str(&format!("  URL: {}\n", url));
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
                McpServerConfig::Sse {
                    url: command.to_string(),
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
                }
            };

            paths
                .preferences
                .global_prefs
                .mcp_servers
                .insert(name.to_string(), config);
            paths.preferences.save_global()?;

            Ok(format!(
                "Added MCP server '{}'. Run 'sync' to apply to all agents.",
                name
            ))
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

        "sync" => {
            let rules_result = paths.sync();
            let prefs_result = paths.sync_preferences();

            match (rules_result, prefs_result) {
                (Ok(rules_msg), Ok(prefs_msg)) => Ok(format!("{}\n{}", rules_msg, prefs_msg)),
                (Err(e), _) => Err(anyhow::anyhow!("Rules sync failed: {}", e)),
                (_, Err(e)) => Err(anyhow::anyhow!("Preferences sync failed: {}", e)),
            }
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
                     - edit_global_rules, edit_project_rules\n\
                     - read_global_rules, read_project_rules\n\
                     - sync, get_status, bootstrap",
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

        _ => Err(anyhow::anyhow!("Unknown tool: {}", name)),
    }
}

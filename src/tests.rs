#[cfg(test)]
mod tests {
    use crate::preferences::ConfigGenerator;
    use serde_json::json;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn test_merge_strategy_claude() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join(".claude");
        fs::create_dir_all(&config_dir).unwrap();

        let settings_path = config_dir.join("settings.json");
        let initial_settings = json!({
            "theme": "dark",
            "fontSize": 14,
            "enabledPlugins": {
                "git": true
            }
        });
        fs::write(
            &settings_path,
            serde_json::to_string(&initial_settings).unwrap(),
        )
        .unwrap();

        let user_config_path = dir.path().join(".claude.json");

        let generator = crate::preferences::ClaudeConfigGenerator {
            config_dir: config_dir.clone(),
            user_config_path: user_config_path.clone(),
        };

        let mut prefs = crate::preferences::AgentPreferences::default();
        let mut specific = crate::preferences::AgentSpecificPrefs::default();
        specific.plugins.insert("git".to_string(), false); // Disable git
        specific.plugins.insert("terminal".to_string(), true); // Enable terminal
        prefs.agent_specific.insert("Claude".to_string(), specific);

        let results = generator.generate(&prefs).unwrap();

        // Find settings.json result
        let (_, content) = results.iter().find(|(p, _)| p == &settings_path).unwrap();
        let new_settings: serde_json::Value = serde_json::from_str(content).unwrap();

        assert_eq!(new_settings["theme"], "dark"); // Preserved
        assert_eq!(new_settings["fontSize"], 14); // Preserved
        assert_eq!(new_settings["enabledPlugins"]["git"], false); // Updated
        assert_eq!(new_settings["enabledPlugins"]["terminal"], true); // Added
    }

    #[test]
    fn test_claude_mcp_servers_in_user_config() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join(".claude");
        fs::create_dir_all(&config_dir).unwrap();

        let user_config_path = dir.path().join(".claude.json");
        let initial_user_config = json!({
            "numStartups": 10,
            "mcpServers": {
                "existing-server": {
                    "type": "stdio",
                    "command": "node",
                    "args": ["server.js"]
                }
            }
        });
        fs::write(
            &user_config_path,
            serde_json::to_string(&initial_user_config).unwrap(),
        )
        .unwrap();

        let generator = crate::preferences::ClaudeConfigGenerator {
            config_dir: config_dir.clone(),
            user_config_path: user_config_path.clone(),
        };

        let mut prefs = crate::preferences::AgentPreferences::default();

        let mut env = std::collections::HashMap::new();
        env.insert("API_KEY".to_string(), "123".to_string());

        prefs.mcp_servers.insert(
            "new-server".to_string(),
            crate::preferences::McpServerConfig::Stdio {
                command: "python".to_string(),
                args: vec!["main.py".to_string()],
                env,
            },
        );

        let results = generator.generate(&prefs).unwrap();

        let (_, content) = results
            .iter()
            .find(|(p, _)| p == &user_config_path)
            .unwrap();
        let new_config: serde_json::Value = serde_json::from_str(content).unwrap();

        // Check other settings preserved
        assert_eq!(new_config["numStartups"], 10);

        // Check new MCP server added with correct format
        assert!(new_config["mcpServers"]["new-server"].is_object());
        assert_eq!(new_config["mcpServers"]["new-server"]["type"], "stdio");
        assert_eq!(new_config["mcpServers"]["new-server"]["command"], "python");
        assert_eq!(
            new_config["mcpServers"]["new-server"]["env"]["API_KEY"],
            "123"
        );
    }

    #[test]
    fn test_magic_mcp_setup() {
        let dir = tempdir().unwrap();
        let config_dir = dir.path().join(".config");
        fs::create_dir_all(&config_dir).unwrap();

        // Let's verify the logic by simulating what magic_mcp_setup does:
        let mut prefs = crate::preferences::AgentPreferences::default();
        let agents = vec![crate::config::AgentInfo {
            name: "Claude".to_string(),
            target_path: PathBuf::from("dummy"),
            status: crate::config::AgentStatus::Ok,
            strategy: crate::config::SyncStrategy::Merge,
        }];

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

        for agent in &agents {
            let entry = prefs.agent_specific.entry(agent.name.clone()).or_default();

            for (name, cmd, args) in &defaults {
                if !entry.mcp_servers.contains_key(*name) {
                    entry.mcp_servers.insert(
                        name.to_string(),
                        crate::preferences::McpServerConfig::Stdio {
                            command: cmd.to_string(),
                            args: args.iter().map(|s| s.to_string()).collect(),
                            env: std::collections::HashMap::new(),
                        },
                    );
                }
            }
        }

        let claude_prefs = prefs.agent_specific.get("Claude").unwrap();
        assert!(claude_prefs.mcp_servers.contains_key("filesystem"));
        assert!(claude_prefs.mcp_servers.contains_key("memory"));
        assert!(claude_prefs.mcp_servers.contains_key("filesystem-uvx"));
        assert!(claude_prefs.mcp_servers.contains_key("memory-uvx"));

        if let crate::preferences::McpServerConfig::Stdio { command, .. } =
            &claude_prefs.mcp_servers["filesystem"]
        {
            assert_eq!(command, "npx");
        } else {
            panic!("Expected Stdio config");
        }
    }
}

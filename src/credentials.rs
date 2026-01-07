use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredToken {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl StoredToken {
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| exp <= Utc::now())
            .unwrap_or(false)
    }

    pub fn expires_soon(&self, buffer_seconds: i64) -> bool {
        self.expires_at
            .map(|exp| exp <= Utc::now() + chrono::Duration::seconds(buffer_seconds))
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenStore {
    pub tokens: HashMap<String, StoredToken>,
}

impl TokenStore {
    pub fn get(&self, server_url: &str) -> Option<&StoredToken> {
        self.tokens.get(&normalize_url(server_url))
    }

    pub fn get_valid(&self, server_url: &str) -> Option<&StoredToken> {
        self.get(server_url).filter(|t| !t.is_expired())
    }

    pub fn insert(&mut self, server_url: &str, token: StoredToken) {
        self.tokens.insert(normalize_url(server_url), token);
    }

    pub fn remove(&mut self, server_url: &str) -> Option<StoredToken> {
        self.tokens.remove(&normalize_url(server_url))
    }

    #[allow(dead_code)]
    pub fn needs_refresh(&self, server_url: &str, buffer_seconds: i64) -> bool {
        self.get(server_url)
            .map(|t| t.expires_soon(buffer_seconds))
            .unwrap_or(false)
    }
}

fn normalize_url(url: &str) -> String {
    url.trim_end_matches('/').to_lowercase()
}

pub struct CredentialManager {
    store_path: PathBuf,
    store: TokenStore,
}

impl CredentialManager {
    pub fn new(config_dir: &Path) -> Self {
        let store_path = config_dir.join("tokens.json");
        Self {
            store_path,
            store: TokenStore::default(),
        }
    }

    pub fn load(&mut self) -> Result<()> {
        if self.store_path.exists() {
            let content =
                fs::read_to_string(&self.store_path).context("Failed to read token store")?;
            self.store = serde_json::from_str(&content).context("Failed to parse token store")?;
        }
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.store_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.store)?;
        fs::write(&self.store_path, content)?;
        Ok(())
    }

    pub fn get_token(&self, server_url: &str) -> Option<&StoredToken> {
        self.store.get(server_url)
    }

    pub fn get_valid_token(&self, server_url: &str) -> Option<&StoredToken> {
        self.store.get_valid(server_url)
    }

    pub fn store_token(&mut self, server_url: &str, token: StoredToken) -> Result<()> {
        self.store.insert(server_url, token);
        self.save()
    }

    pub fn remove_token(&mut self, server_url: &str) -> Result<Option<StoredToken>> {
        let removed = self.store.remove(server_url);
        if removed.is_some() {
            self.save()?;
        }
        Ok(removed)
    }

    #[allow(dead_code)]
    pub fn needs_refresh(&self, server_url: &str) -> bool {
        self.store.needs_refresh(server_url, 300)
    }

    #[allow(dead_code)]
    pub fn list_servers_with_tokens(&self) -> Vec<&str> {
        self.store.tokens.keys().map(|s| s.as_str()).collect()
    }

    pub fn token_status(&self, server_url: &str) -> TokenStatus {
        match self.store.get(server_url) {
            None => TokenStatus::None,
            Some(token) if token.is_expired() => TokenStatus::Expired,
            Some(token) if token.expires_soon(300) => TokenStatus::ExpiresSoon,
            Some(_) => TokenStatus::Valid,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenStatus {
    None,
    Valid,
    ExpiresSoon,
    Expired,
}

impl TokenStatus {
    pub fn symbol(&self) -> &'static str {
        match self {
            TokenStatus::None => "?",
            TokenStatus::Valid => "V",
            TokenStatus::ExpiresSoon => "!",
            TokenStatus::Expired => "X",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            TokenStatus::None => "Not authenticated",
            TokenStatus::Valid => "Authenticated",
            TokenStatus::ExpiresSoon => "Token expires soon",
            TokenStatus::Expired => "Token expired",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_token_expiry() {
        let expired = StoredToken {
            access_token: "test".into(),
            refresh_token: None,
            expires_at: Some(Utc::now() - Duration::hours(1)),
            token_type: "Bearer".into(),
            scopes: vec![],
        };
        assert!(expired.is_expired());

        let valid = StoredToken {
            access_token: "test".into(),
            refresh_token: None,
            expires_at: Some(Utc::now() + Duration::hours(1)),
            token_type: "Bearer".into(),
            scopes: vec![],
        };
        assert!(!valid.is_expired());
    }

    #[test]
    fn test_token_expires_soon() {
        let soon = StoredToken {
            access_token: "test".into(),
            refresh_token: None,
            expires_at: Some(Utc::now() + Duration::seconds(100)),
            token_type: "Bearer".into(),
            scopes: vec![],
        };
        assert!(soon.expires_soon(300));
        assert!(!soon.expires_soon(60));
    }

    #[test]
    fn test_url_normalization() {
        let mut store = TokenStore::default();
        store.insert(
            "https://api.example.com/mcp/",
            StoredToken {
                access_token: "test".into(),
                refresh_token: None,
                expires_at: None,
                token_type: "Bearer".into(),
                scopes: vec![],
            },
        );
        assert!(store.get("https://api.example.com/mcp").is_some());
        assert!(store.get("HTTPS://API.EXAMPLE.COM/MCP/").is_some());
    }
}

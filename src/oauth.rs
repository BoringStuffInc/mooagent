use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use url::Url;

use crate::credentials::StoredToken;
use crate::http;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectedResourceMetadata {
    pub resource: Option<String>,
    #[serde(default)]
    pub authorization_servers: Vec<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthServerMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub registration_endpoint: Option<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
    #[serde(default)]
    pub response_types_supported: Vec<String>,
    #[serde(default)]
    pub grant_types_supported: Vec<String>,
    #[serde(default)]
    pub code_challenge_methods_supported: Vec<String>,
    #[serde(default)]
    pub client_id_metadata_document_supported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

impl TokenResponse {
    pub fn into_stored_token(self) -> StoredToken {
        let expires_at = self
            .expires_in
            .map(|secs| Utc::now() + Duration::seconds(secs as i64));

        let scopes = self
            .scope
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default();

        StoredToken {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            expires_at,
            token_type: if self.token_type.is_empty() {
                "Bearer".to_string()
            } else {
                self.token_type
            },
            scopes,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub scopes: Vec<String>,
    pub auth_server_url: Option<String>,
}

pub struct OAuthFlow {
    server_url: String,
    config: OAuthConfig,
    auth_metadata: Option<AuthServerMetadata>,
}

impl OAuthFlow {
    pub fn new(server_url: String, config: OAuthConfig) -> Self {
        Self {
            server_url,
            config,
            auth_metadata: None,
        }
    }

    pub async fn discover_metadata(&mut self) -> Result<&AuthServerMetadata> {
        if self.auth_metadata.is_some() {
            return Ok(self.auth_metadata.as_ref().unwrap());
        }

        let auth_server_url = if let Some(url) = &self.config.auth_server_url {
            url.clone()
        } else {
            self.discover_auth_server().await?
        };

        let metadata = self.fetch_auth_server_metadata(&auth_server_url).await?;

        if !metadata
            .code_challenge_methods_supported
            .contains(&"S256".to_string())
            && !metadata.code_challenge_methods_supported.is_empty()
        {
            bail!("Authorization server does not support PKCE S256");
        }

        self.auth_metadata = Some(metadata);
        Ok(self.auth_metadata.as_ref().unwrap())
    }

    async fn discover_auth_server(&self) -> Result<String> {
        let base_url = http::extract_base_url(&self.server_url)?;

        let parsed = Url::parse(&self.server_url)?;
        let path = parsed.path();

        let well_known_urls = if path.is_empty() || path == "/" {
            vec![format!("{}/.well-known/oauth-protected-resource", base_url)]
        } else {
            vec![
                format!(
                    "{}/.well-known/oauth-protected-resource{}",
                    base_url,
                    path.trim_end_matches('/')
                ),
                format!("{}/.well-known/oauth-protected-resource", base_url),
            ]
        };

        for url in well_known_urls {
            if let Ok(metadata) = http::get_json::<ProtectedResourceMetadata>(&url).await
                && let Some(auth_server) = metadata.authorization_servers.first()
            {
                return Ok(auth_server.clone());
            }
        }

        Ok(base_url)
    }

    async fn fetch_auth_server_metadata(
        &self,
        auth_server_url: &str,
    ) -> Result<AuthServerMetadata> {
        let parsed = Url::parse(auth_server_url)?;
        let path = parsed.path();
        let base = http::extract_base_url(auth_server_url)?;

        let urls_to_try = if path.is_empty() || path == "/" {
            vec![
                format!("{}/.well-known/oauth-authorization-server", base),
                format!("{}/.well-known/openid-configuration", base),
            ]
        } else {
            vec![
                format!(
                    "{}/.well-known/oauth-authorization-server{}",
                    base,
                    path.trim_end_matches('/')
                ),
                format!(
                    "{}/.well-known/openid-configuration{}",
                    base,
                    path.trim_end_matches('/')
                ),
                format!(
                    "{}{}/.well-known/openid-configuration",
                    base,
                    path.trim_end_matches('/')
                ),
            ]
        };

        for url in &urls_to_try {
            if let Ok(metadata) = http::get_json::<AuthServerMetadata>(url).await {
                return Ok(metadata);
            }
        }

        Ok(AuthServerMetadata {
            issuer: auth_server_url.to_string(),
            authorization_endpoint: format!("{}/authorize", base),
            token_endpoint: format!("{}/token", base),
            registration_endpoint: Some(format!("{}/register", base)),
            scopes_supported: vec![],
            response_types_supported: vec!["code".to_string()],
            grant_types_supported: vec!["authorization_code".to_string()],
            code_challenge_methods_supported: vec!["S256".to_string()],
            client_id_metadata_document_supported: false,
        })
    }

    pub async fn authorize(&mut self) -> Result<StoredToken> {
        let metadata = self.discover_metadata().await?.clone();
        let token_endpoint = metadata.token_endpoint.clone();

        let (code_verifier, code_challenge) = generate_pkce();
        let state = generate_state();

        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let redirect_uri = format!("http://127.0.0.1:{}/callback", port);

        let scopes = if self.config.scopes.is_empty() {
            metadata.scopes_supported.join(" ")
        } else {
            self.config.scopes.join(" ")
        };

        let mut auth_url = Url::parse(&metadata.authorization_endpoint)?;
        {
            let mut params = auth_url.query_pairs_mut();
            params.append_pair("response_type", "code");
            params.append_pair("client_id", &self.config.client_id);
            params.append_pair("redirect_uri", &redirect_uri);
            params.append_pair("state", &state);
            params.append_pair("code_challenge", &code_challenge);
            params.append_pair("code_challenge_method", "S256");
            params.append_pair("resource", &self.server_url);
            if !scopes.is_empty() {
                params.append_pair("scope", &scopes);
            }
        }

        log::info!("Opening browser for OAuth authorization");
        open::that(auth_url.as_str()).context("Failed to open browser")?;

        let code = wait_for_callback(listener, &state)?;

        self.exchange_code(&token_endpoint, &code, &redirect_uri, &code_verifier)
            .await
    }

    async fn exchange_code(
        &self,
        token_endpoint: &str,
        code: &str,
        redirect_uri: &str,
        code_verifier: &str,
    ) -> Result<StoredToken> {
        let client_id = self.config.client_id.clone();
        let client_secret = self.config.client_secret.clone();
        let resource = self.server_url.clone();

        let mut params = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", client_id.as_str()),
            ("code_verifier", code_verifier),
            ("resource", resource.as_str()),
        ];

        let secret_ref;
        if let Some(ref secret) = client_secret {
            secret_ref = secret.as_str();
            params.push(("client_secret", secret_ref));
        }

        let response: TokenResponse = http::post_form(token_endpoint, &params).await?;

        Ok(response.into_stored_token())
    }

    #[allow(dead_code)]
    pub async fn refresh_token(&mut self, refresh_token: &str) -> Result<StoredToken> {
        let metadata = self.discover_metadata().await?;
        let token_endpoint = metadata.token_endpoint.clone();

        let client_id = self.config.client_id.clone();
        let client_secret = self.config.client_secret.clone();

        let mut params = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id.as_str()),
        ];

        let secret_ref;
        if let Some(ref secret) = client_secret {
            secret_ref = secret.as_str();
            params.push(("client_secret", secret_ref));
        }

        let response: TokenResponse = http::post_form(&token_endpoint, &params).await?;

        let mut stored = response.into_stored_token();
        if stored.refresh_token.is_none() {
            stored.refresh_token = Some(refresh_token.to_string());
        }

        Ok(stored)
    }
}

fn generate_pkce() -> (String, String) {
    let mut rng = rand::thread_rng();
    let verifier: String = (0..64)
        .map(|_| {
            let idx = rng.gen_range(0..66);
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~"[idx] as char
        })
        .collect();

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let challenge = URL_SAFE_NO_PAD.encode(hash);

    (verifier, challenge)
}

fn generate_state() -> String {
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| {
            let idx = rng.gen_range(0..62);
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"[idx] as char
        })
        .collect()
}

fn wait_for_callback(listener: TcpListener, expected_state: &str) -> Result<String> {
    listener
        .set_nonblocking(false)
        .context("Failed to set listener to blocking")?;

    log::info!("Waiting for OAuth callback on {}", listener.local_addr()?);

    let (mut stream, _) = listener.accept().context("Failed to accept connection")?;

    let mut buffer = [0u8; 4096];
    let n = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..n]);

    let first_line = request.lines().next().unwrap_or("");
    let path = first_line.split_whitespace().nth(1).unwrap_or("/");

    let params: HashMap<String, String> = Url::parse(&format!("http://localhost{}", path))
        .ok()
        .map(|url| url.query_pairs().into_owned().collect())
        .unwrap_or_default();

    let response_body;
    let status;

    if let Some(error) = params.get("error") {
        let desc = params
            .get("error_description")
            .map(|s| s.as_str())
            .unwrap_or("Unknown error");
        response_body = format!(
            "<html><body><h1>Authorization Failed</h1><p>{}: {}</p></body></html>",
            error, desc
        );
        status = "400 Bad Request";

        let response = format!(
            "HTTP/1.1 {}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            status,
            response_body.len(),
            response_body
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();

        bail!("OAuth error: {} - {}", error, desc);
    }

    let state = params.get("state").map(|s| s.as_str()).unwrap_or("");
    if state != expected_state {
        response_body =
            "<html><body><h1>Invalid State</h1><p>CSRF validation failed.</p></body></html>"
                .to_string();
        status = "400 Bad Request";

        let response = format!(
            "HTTP/1.1 {}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            status,
            response_body.len(),
            response_body
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();

        bail!("OAuth state mismatch - possible CSRF attack");
    }

    let code = params
        .get("code")
        .context("No authorization code in callback")?
        .clone();

    response_body = "<html><body><h1>Authorization Successful</h1><p>You can close this window and return to mooagent.</p></body></html>".to_string();
    status = "200 OK";

    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        response_body.len(),
        response_body
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;

    Ok(code)
}

pub async fn run_oauth_flow(
    server_url: &str,
    client_id: &str,
    client_secret: Option<&str>,
    scopes: Vec<String>,
    auth_server_url: Option<&str>,
) -> Result<StoredToken> {
    let config = OAuthConfig {
        client_id: client_id.to_string(),
        client_secret: client_secret.map(String::from),
        scopes,
        auth_server_url: auth_server_url.map(String::from),
    };

    let mut flow = OAuthFlow::new(server_url.to_string(), config);
    flow.authorize().await
}

#[allow(dead_code)]
pub async fn refresh_oauth_token(
    server_url: &str,
    client_id: &str,
    client_secret: Option<&str>,
    refresh_token: &str,
    auth_server_url: Option<&str>,
) -> Result<StoredToken> {
    let config = OAuthConfig {
        client_id: client_id.to_string(),
        client_secret: client_secret.map(String::from),
        scopes: vec![],
        auth_server_url: auth_server_url.map(String::from),
    };

    let mut flow = OAuthFlow::new(server_url.to_string(), config);
    flow.refresh_token(refresh_token).await
}

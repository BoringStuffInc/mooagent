use anyhow::{Context, Result};
use reqwest::Client;
use std::sync::OnceLock;
use std::time::Duration;

static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

pub fn client() -> &'static Client {
    HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(concat!("mooagent/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("Failed to build HTTP client")
    })
}

pub async fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T> {
    let response = client()
        .get(url)
        .send()
        .await
        .context("HTTP request failed")?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("HTTP {} - {}", status, body);
    }

    response
        .json()
        .await
        .context("Failed to parse JSON response")
}

pub async fn post_form<T: serde::de::DeserializeOwned>(
    url: &str,
    form: &[(&str, &str)],
) -> Result<T> {
    let response = client()
        .post(url)
        .form(form)
        .send()
        .await
        .context("HTTP request failed")?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("HTTP {} - {}", status, body);
    }

    response
        .json()
        .await
        .context("Failed to parse JSON response")
}

pub fn extract_base_url(server_url: &str) -> Result<String> {
    let parsed = url::Url::parse(server_url).context("Invalid server URL")?;
    let base = format!(
        "{}://{}{}",
        parsed.scheme(),
        parsed.host_str().context("URL has no host")?,
        parsed.port().map(|p| format!(":{}", p)).unwrap_or_default()
    );
    Ok(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_base_url() {
        assert_eq!(
            extract_base_url("https://api.example.com/v1/mcp").unwrap(),
            "https://api.example.com"
        );
        assert_eq!(
            extract_base_url("https://api.example.com:8443/mcp").unwrap(),
            "https://api.example.com:8443"
        );
        assert_eq!(
            extract_base_url("http://localhost:3000/api/mcp").unwrap(),
            "http://localhost:3000"
        );
    }
}

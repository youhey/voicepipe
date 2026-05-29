use anyhow::{Context, Result, anyhow};
use reqwest::StatusCode;

pub struct UpstreamClient {
    client: reqwest::Client,
    access_token: Option<String>,
}

impl UpstreamClient {
    pub fn new(access_token: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            access_token,
        }
    }

    pub async fn fetch_episode_json(&self, url: &str) -> Result<String> {
        let mut request = self.client.get(url);
        if let Some(token) = self.access_token.as_deref() {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|error| anyhow!("upstream API に接続できません: {}", error))?;

        let status = response.status();
        if !status.is_success() {
            return Err(upstream_status_error(status, response).await);
        }

        response
            .text()
            .await
            .context("upstream API のレスポンス本文を読み込めません")
    }
}

async fn upstream_status_error(status: StatusCode, response: reqwest::Response) -> anyhow::Error {
    let body = response.text().await.unwrap_or_default();
    let summary = body
        .lines()
        .take(8)
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(1000)
        .collect::<String>();

    if summary.is_empty() {
        return anyhow!("upstream API が失敗しました: HTTP {}", status);
    }

    anyhow!("upstream API が失敗しました: HTTP {}: {}", status, summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[tokio::test]
    async fn sends_bearer_token_when_configured() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
        let address = listener.local_addr().expect("test server address");

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request should arrive");
            let mut buffer = [0_u8; 4096];
            let size = stream.read(&mut buffer).expect("request should read");
            let request = String::from_utf8_lossy(&buffer[..size]);

            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer test-token")
            );

            let body = r#"{"episode":{"episode_key":"episode-001","title":"テスト","language":"ja","scenario_json":{"sections":[{"type":"opening","title":"オープニング","text":"こんにちは。"}]}}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("response should write");
        });

        let client = UpstreamClient::new(Some("test-token".to_string()));
        let body = client
            .fetch_episode_json(&format!("http://{address}/episode"))
            .await
            .expect("fetch should succeed");

        assert!(body.contains("episode-001"));
        handle.join().expect("server thread should finish");
    }
}

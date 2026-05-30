use std::{fs, path::Path};

use anyhow::{Context, Result, anyhow};
use reqwest::{StatusCode, multipart};
use serde_json::json;

pub struct DownstreamClient {
    client: reqwest::Client,
    access_token: Option<String>,
}

impl DownstreamClient {
    pub fn new(access_token: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            access_token,
        }
    }

    pub async fn upload_episode(
        &self,
        url: &str,
        episode_key: &str,
        json_path: &Path,
        audio_path: &Path,
    ) -> Result<()> {
        let json_bytes = fs::read(json_path).with_context(|| {
            format!(
                "upload 用 Episode JSON を読み込めません: {}",
                json_path.display()
            )
        })?;
        let audio_bytes = fs::read(audio_path)
            .with_context(|| format!("upload 用 MP3 を読み込めません: {}", audio_path.display()))?;
        let metadata = json!({
            "episode_key": episode_key,
            "json_path": json_path.display().to_string(),
            "audio_path": audio_path.display().to_string(),
            "voicepipe_version": env!("CARGO_PKG_VERSION"),
        });

        let json_filename = json_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("episode.json")
            .to_string();
        let audio_filename = audio_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("episode.mp3")
            .to_string();

        let form = multipart::Form::new()
            .part(
                "audio",
                multipart::Part::bytes(audio_bytes)
                    .file_name(audio_filename)
                    .mime_str("audio/mpeg")
                    .context("audio multipart part を作成できません")?,
            )
            .part(
                "episode_json",
                multipart::Part::bytes(json_bytes)
                    .file_name(json_filename)
                    .mime_str("application/json")
                    .context("episode_json multipart part を作成できません")?,
            )
            .part(
                "render_metadata",
                multipart::Part::text(metadata.to_string())
                    .mime_str("application/json")
                    .context("render_metadata multipart part を作成できません")?,
            );

        let mut request = self.client.post(url).multipart(form);
        if let Some(token) = self.access_token.as_deref() {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|error| anyhow!("downstream API に接続できません: {}", error))?;

        let status = response.status();
        if !status.is_success() {
            return Err(downstream_status_error(status, response).await);
        }

        Ok(())
    }
}

async fn downstream_status_error(status: StatusCode, response: reqwest::Response) -> anyhow::Error {
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
        return anyhow!("downstream API が失敗しました: HTTP {}", status);
    }

    anyhow!(
        "downstream API が失敗しました: HTTP {}: {}",
        status,
        summary
    )
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
    async fn uploads_audio_json_and_metadata_parts() {
        let temp_dir =
            std::env::temp_dir().join(format!("voicepipe-downstream-test-{}", std::process::id()));
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let json_path = temp_dir.join("episode.json");
        let audio_path = temp_dir.join("episode.mp3");
        fs::write(&json_path, br#"{"episode":{"episode_key":"episode-001"}}"#)
            .expect("json should be written");
        fs::write(&audio_path, b"fake mp3").expect("audio should be written");

        let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
        let address = listener.local_addr().expect("test server address");

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request should arrive");
            let request = read_http_request(&mut stream);

            assert!(request.contains("name=\"audio\""));
            assert!(request.contains("name=\"episode_json\""));
            assert!(request.contains("name=\"render_metadata\""));
            assert!(request.contains("episode-001"));

            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .expect("response should write");
        });

        DownstreamClient::new(None)
            .upload_episode(
                &format!("http://{address}/api/episodes"),
                "episode-001",
                &json_path,
                &audio_path,
            )
            .await
            .expect("upload should succeed");

        handle.join().expect("server thread should finish");
        fs::remove_dir_all(temp_dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn sends_bearer_token_when_configured() {
        let temp_dir = std::env::temp_dir().join(format!(
            "voicepipe-downstream-token-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let json_path = temp_dir.join("episode.json");
        let audio_path = temp_dir.join("episode.mp3");
        fs::write(&json_path, br#"{"episode":{"episode_key":"episode-001"}}"#)
            .expect("json should be written");
        fs::write(&audio_path, b"fake mp3").expect("audio should be written");

        let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
        let address = listener.local_addr().expect("test server address");

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request should arrive");
            let request = read_http_request(&mut stream);

            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer test-token")
            );

            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .expect("response should write");
        });

        DownstreamClient::new(Some("test-token".to_string()))
            .upload_episode(
                &format!("http://{address}/api/episodes"),
                "episode-001",
                &json_path,
                &audio_path,
            )
            .await
            .expect("upload should succeed");

        handle.join().expect("server thread should finish");
        fs::remove_dir_all(temp_dir).expect("temp dir should be removed");
    }

    fn read_http_request(stream: &mut std::net::TcpStream) -> String {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 4096];
        let mut content_length = None;

        loop {
            let size = stream.read(&mut chunk).expect("request should read");
            if size == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..size]);

            if content_length.is_none()
                && let Some(header_end) = find_header_end(&buffer)
            {
                let headers = String::from_utf8_lossy(&buffer[..header_end]);
                content_length = headers.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                });
            }

            if let (Some(header_end), Some(length)) = (find_header_end(&buffer), content_length)
                && buffer.len() >= header_end + 4 + length
            {
                break;
            }
        }

        String::from_utf8_lossy(&buffer).to_string()
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }
}

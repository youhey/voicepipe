use std::{fs, path::Path};

use anyhow::{Context, Result, anyhow};
use reqwest::{StatusCode, Url, multipart};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadMethod {
    Post,
    Put,
}

impl UploadMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Post => "post",
            Self::Put => "put",
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DownstreamManifestEpisode {
    pub episode_key: String,
    pub audio_sha256: Option<String>,
    pub episode_json_sha256: Option<String>,
    pub audio_size_bytes: Option<u64>,
    pub episode_json_size_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ManifestResponse {
    Episodes {
        episodes: Vec<DownstreamManifestEpisode>,
    },
    DataEpisodes {
        data: ManifestData,
    },
    Data {
        data: Vec<DownstreamManifestEpisode>,
    },
    Bare(Vec<DownstreamManifestEpisode>),
}

#[derive(Debug, Deserialize)]
struct ManifestData {
    episodes: Vec<DownstreamManifestEpisode>,
}

impl ManifestResponse {
    fn into_episodes(self) -> Vec<DownstreamManifestEpisode> {
        match self {
            Self::Episodes { episodes } => episodes,
            Self::DataEpisodes { data } => data.episodes,
            Self::Data { data } => data,
            Self::Bare(episodes) => episodes,
        }
    }
}

pub struct DownstreamClient {
    client: reqwest::Client,
    access_token: Option<String>,
}

pub struct SyncEpisodeRequest<'a> {
    pub method: UploadMethod,
    pub upload_url: &'a str,
    pub episode_key: &'a str,
    pub json_path: &'a Path,
    pub audio_path: &'a Path,
    pub render_metadata_path: &'a Path,
    pub recorded_at: &'a str,
    pub audio_duration_seconds: u64,
}

impl DownstreamClient {
    pub fn new(access_token: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            access_token,
        }
    }

    pub async fn sync_episode(&self, request: SyncEpisodeRequest<'_>) -> Result<()> {
        let json_bytes = fs::read(request.json_path).with_context(|| {
            format!(
                "upload 用 Episode JSON を読み込めません: {}",
                request.json_path.display()
            )
        })?;
        let audio_bytes = fs::read(request.audio_path).with_context(|| {
            format!(
                "upload 用 MP3 を読み込めません: {}",
                request.audio_path.display()
            )
        })?;
        let metadata_bytes = fs::read(request.render_metadata_path).with_context(|| {
            format!(
                "upload 用 render metadata を読み込めません: {}",
                request.render_metadata_path.display()
            )
        })?;

        let json_filename = request
            .json_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("episode.json")
            .to_string();
        let audio_filename = request
            .audio_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("episode.mp3")
            .to_string();
        let metadata_filename = request
            .render_metadata_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("render_metadata.json")
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
                "render_metadata_json",
                multipart::Part::bytes(metadata_bytes)
                    .file_name(metadata_filename)
                    .mime_str("application/json")
                    .context("render_metadata_json multipart part を作成できません")?,
            );
        let form = form
            .text("recorded_at", request.recorded_at.to_string())
            .text(
                "audio_duration_seconds",
                request.audio_duration_seconds.to_string(),
            );

        let url = match request.method {
            UploadMethod::Post => request.upload_url.to_string(),
            UploadMethod::Put => episode_url(request.upload_url, request.episode_key)?,
        };
        let mut http_request = match request.method {
            UploadMethod::Post => self.client.post(&url),
            UploadMethod::Put => self.client.put(&url),
        }
        .multipart(form);
        if let Some(token) = self.access_token.as_deref() {
            http_request = http_request.bearer_auth(token);
        }

        let response = http_request
            .send()
            .await
            .map_err(|error| anyhow!("downstream API に接続できません: {}", error))?;

        let status = response.status();
        if !status.is_success() {
            return Err(downstream_status_error(status, response).await);
        }

        Ok(())
    }

    pub async fn fetch_manifest(&self, upload_url: &str) -> Result<Vec<DownstreamManifestEpisode>> {
        let url = manifest_url(upload_url)?;
        let mut request = self.client.get(&url);
        if let Some(token) = self.access_token.as_deref() {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|error| anyhow!("downstream manifest API に接続できません: {}", error))?;

        let status = response.status();
        if !status.is_success() {
            return Err(downstream_status_error(status, response).await);
        }

        let body = response
            .text()
            .await
            .context("downstream manifest API のレスポンス本文を読み込めません")?;
        parse_manifest_json(&body)
    }
}

pub fn manifest_url(upload_url: &str) -> Result<String> {
    endpoint_url(upload_url, "manifest")
}

fn episode_url(upload_url: &str, episode_key: &str) -> Result<String> {
    endpoint_url(upload_url, episode_key)
}

fn endpoint_url(upload_url: &str, segment: &str) -> Result<String> {
    let mut url = Url::parse(upload_url)
        .with_context(|| format!("downstream upload_url が URL として不正です: {upload_url}"))?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| anyhow!("downstream upload_url に path segment を追加できません"))?;
        segments.pop_if_empty();
        segments.push(segment);
    }

    Ok(url.to_string())
}

fn parse_manifest_json(body: &str) -> Result<Vec<DownstreamManifestEpisode>> {
    let response = serde_json::from_str::<ManifestResponse>(body)
        .context("downstream manifest JSON を解析できません")?;
    Ok(response.into_episodes())
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
        let metadata_path = temp_dir.join("render_metadata.json");
        fs::write(&json_path, br#"{"episode":{"episode_key":"episode-001"}}"#)
            .expect("json should be written");
        fs::write(&audio_path, b"fake mp3").expect("audio should be written");
        fs::write(
            &metadata_path,
            br#"{"episode_key":"episode-001","recorded_at":"2026-05-31T04:12:30Z","audio_duration_seconds":842}"#,
        )
        .expect("metadata should be written");

        let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
        let address = listener.local_addr().expect("test server address");

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request should arrive");
            let request = read_http_request(&mut stream);

            assert!(request.contains("name=\"audio\""));
            assert!(request.contains("name=\"episode_json\""));
            assert!(request.contains("name=\"render_metadata_json\""));
            assert!(request.contains("name=\"recorded_at\""));
            assert!(request.contains("2026-05-31T04:12:30Z"));
            assert!(request.contains("name=\"audio_duration_seconds\""));
            assert!(request.contains("842"));
            assert!(request.contains("episode-001"));

            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .expect("response should write");
        });

        DownstreamClient::new(None)
            .sync_episode(SyncEpisodeRequest {
                method: UploadMethod::Post,
                upload_url: &format!("http://{address}/api/episodes"),
                episode_key: "episode-001",
                json_path: &json_path,
                audio_path: &audio_path,
                render_metadata_path: &metadata_path,
                recorded_at: "2026-05-31T04:12:30Z",
                audio_duration_seconds: 842,
            })
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
        let metadata_path = temp_dir.join("render_metadata.json");
        fs::write(&json_path, br#"{"episode":{"episode_key":"episode-001"}}"#)
            .expect("json should be written");
        fs::write(&audio_path, b"fake mp3").expect("audio should be written");
        fs::write(
            &metadata_path,
            br#"{"episode_key":"episode-001","recorded_at":"2026-05-31T04:12:30Z","audio_duration_seconds":842}"#,
        )
        .expect("metadata should be written");

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
            .sync_episode(SyncEpisodeRequest {
                method: UploadMethod::Post,
                upload_url: &format!("http://{address}/api/episodes"),
                episode_key: "episode-001",
                json_path: &json_path,
                audio_path: &audio_path,
                render_metadata_path: &metadata_path,
                recorded_at: "2026-05-31T04:12:30Z",
                audio_duration_seconds: 842,
            })
            .await
            .expect("upload should succeed");

        handle.join().expect("server thread should finish");
        fs::remove_dir_all(temp_dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn puts_episode_to_episode_endpoint() {
        let temp_dir = std::env::temp_dir().join(format!(
            "voicepipe-downstream-put-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let json_path = temp_dir.join("episode.json");
        let audio_path = temp_dir.join("episode.mp3");
        let metadata_path = temp_dir.join("render_metadata.json");
        fs::write(&json_path, br#"{"episode":{"episode_key":"episode-001"}}"#)
            .expect("json should be written");
        fs::write(&audio_path, b"fake mp3").expect("audio should be written");
        fs::write(&metadata_path, br#"{"episode_key":"episode-001"}"#)
            .expect("metadata should be written");

        let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
        let address = listener.local_addr().expect("test server address");

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request should arrive");
            let request = read_http_request(&mut stream);

            assert!(request.starts_with("PUT /api/episodes/episode-001 HTTP/1.1"));
            assert!(request.contains("name=\"episode_json\""));

            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .expect("response should write");
        });

        DownstreamClient::new(None)
            .sync_episode(SyncEpisodeRequest {
                method: UploadMethod::Put,
                upload_url: &format!("http://{address}/api/episodes"),
                episode_key: "episode-001",
                json_path: &json_path,
                audio_path: &audio_path,
                render_metadata_path: &metadata_path,
                recorded_at: "2026-05-31T04:12:30Z",
                audio_duration_seconds: 842,
            })
            .await
            .expect("upload should succeed");

        handle.join().expect("server thread should finish");
        fs::remove_dir_all(temp_dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn fetches_manifest_with_bearer_token() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
        let address = listener.local_addr().expect("test server address");

        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("request should arrive");
            let request = read_http_request(&mut stream);

            assert!(request.starts_with("GET /api/episodes/manifest HTTP/1.1"));
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer manifest-token")
            );

            let body = br#"{"episodes":[{"episode_key":"episode-001","audio_sha256":"aaa","episode_json_sha256":"bbb","audio_size_bytes":3,"episode_json_size_bytes":4}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                String::from_utf8_lossy(body)
            );
            stream
                .write_all(response.as_bytes())
                .expect("response should write");
        });

        let manifest = DownstreamClient::new(Some("manifest-token".to_string()))
            .fetch_manifest(&format!("http://{address}/api/episodes"))
            .await
            .expect("manifest should load");

        assert_eq!(manifest.len(), 1);
        assert_eq!(manifest[0].episode_key, "episode-001");
        assert_eq!(manifest[0].audio_sha256.as_deref(), Some("aaa"));
        assert_eq!(manifest[0].episode_json_sha256.as_deref(), Some("bbb"));

        handle.join().expect("server thread should finish");
    }

    #[test]
    fn manifest_url_appends_manifest_segment() {
        assert_eq!(
            manifest_url("https://example.com/api/episodes").expect("manifest url"),
            "https://example.com/api/episodes/manifest"
        );
    }

    #[test]
    fn parses_manifest_wrapped_or_bare() {
        let wrapped = parse_manifest_json(r#"{"episodes":[{"episode_key":"episode-001"}]}"#)
            .expect("wrapped manifest should parse");
        let bare = parse_manifest_json(r#"[{"episode_key":"episode-002"}]"#)
            .expect("bare manifest should parse");
        let data_wrapped =
            parse_manifest_json(r#"{"data":{"episodes":[{"episode_key":"episode-003"}]}}"#)
                .expect("data-wrapped manifest should parse");

        assert_eq!(wrapped[0].episode_key, "episode-001");
        assert_eq!(bare[0].episode_key, "episode-002");
        assert_eq!(data_wrapped[0].episode_key, "episode-003");
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

            if let Some(header_end) = find_header_end(&buffer)
                && content_length.is_none()
            {
                let headers = String::from_utf8_lossy(&buffer[..header_end]);
                if headers.starts_with("GET ") || headers.starts_with("HEAD ") {
                    break;
                }
            }
        }

        String::from_utf8_lossy(&buffer).to_string()
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }
}

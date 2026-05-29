use anyhow::{Context, Error, Result, anyhow, bail};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{cli::VoicevoxArgs, config};

pub const DEFAULT_VOICEVOX_ENDPOINT: &str = "http://127.0.0.1:50021";
pub const DEFAULT_SPEAKER: u32 = 3;
pub const DEFAULT_SPEED_SCALE: f64 = 1.2;
pub const DEFAULT_PITCH_SCALE: f64 = 0.0;
pub const DEFAULT_INTONATION_SCALE: f64 = 0.9;
pub const DEFAULT_PAUSE_LENGTH_SCALE: f64 = 1.3;
pub const DEFAULT_VOLUME_SCALE: f64 = 1.0;

#[derive(Debug, Clone)]
pub struct VoiceOptions {
    pub speed_scale: f64,
    pub pitch_scale: f64,
    pub intonation_scale: f64,
    pub pause_length_scale: f64,
    pub volume_scale: f64,
}

impl Default for VoiceOptions {
    fn default() -> Self {
        Self {
            speed_scale: DEFAULT_SPEED_SCALE,
            pitch_scale: DEFAULT_PITCH_SCALE,
            intonation_scale: DEFAULT_INTONATION_SCALE,
            pause_length_scale: DEFAULT_PAUSE_LENGTH_SCALE,
            volume_scale: DEFAULT_VOLUME_SCALE,
        }
    }
}

#[derive(Debug, Clone)]
pub struct VoicevoxClient {
    client: reqwest::Client,
    endpoint: String,
    speaker: u32,
    voice_options: VoiceOptions,
}

#[derive(Debug, Serialize)]
struct VoicevoxQuery<'a> {
    text: &'a str,
    speaker: u32,
}

#[derive(Debug, Deserialize)]
pub struct Speaker {
    pub name: String,
    pub styles: Vec<SpeakerStyle>,
}

#[derive(Debug, Deserialize)]
pub struct SpeakerStyle {
    pub name: String,
    pub id: u32,
}

impl VoicevoxClient {
    pub fn new(endpoint: String, speaker: u32, voice_options: VoiceOptions) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoint: endpoint.trim_end_matches('/').to_string(),
            speaker,
            voice_options,
        }
    }

    pub async fn ensure_ready(&self) -> Result<()> {
        let response = self
            .client
            .get(self.url("/version"))
            .send()
            .await
            .map_err(|error| {
                anyhow!(
                    "VOICEVOX Engine に接続できません: {} ({})",
                    self.endpoint,
                    error
                )
            })?;

        if !response.status().is_success() {
            bail!(
                "VOICEVOX Engine の疎通確認に失敗しました: HTTP {} ({})",
                response.status(),
                self.endpoint
            );
        }

        Ok(())
    }

    pub async fn synthesize(&self, text: &str) -> Result<Vec<u8>> {
        let audio_query = self.audio_query(text).await?;
        self.synthesis(&audio_query).await
    }

    pub async fn speakers(&self) -> Result<Vec<Speaker>> {
        let response = self
            .client
            .get(self.url("/speakers"))
            .send()
            .await
            .map_err(|error| anyhow!("VOICEVOX Engine の speakers に接続できません: {}", error))?;

        let status = response.status();
        if !status.is_success() {
            return Err(voicevox_status_error("speakers", status, response).await);
        }

        response
            .json::<Vec<Speaker>>()
            .await
            .context("VOICEVOX speakers の JSON を解析できません")
    }

    async fn audio_query(&self, text: &str) -> Result<Value> {
        let response = self
            .client
            .post(self.url("/audio_query"))
            .query(&VoicevoxQuery {
                text,
                speaker: self.speaker,
            })
            .send()
            .await
            .map_err(|error| {
                anyhow!("VOICEVOX Engine の audio_query に接続できません: {}", error)
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(voicevox_status_error("audio_query", status, response).await);
        }

        let mut query = response
            .json::<Value>()
            .await
            .context("VOICEVOX audio_query の JSON を解析できません")?;
        self.apply_voice_options(&mut query);

        Ok(query)
    }

    async fn synthesis(&self, audio_query: &Value) -> Result<Vec<u8>> {
        let response = self
            .client
            .post(self.url("/synthesis"))
            .query(&[("speaker", self.speaker)])
            .json(audio_query)
            .send()
            .await
            .map_err(|error| anyhow!("VOICEVOX Engine の synthesis に接続できません: {}", error))?;

        let status = response.status();
        if !status.is_success() {
            return Err(voicevox_status_error("synthesis", status, response).await);
        }

        Ok(response
            .bytes()
            .await
            .context("VOICEVOX synthesis の音声データを読み込めません")?
            .to_vec())
    }

    fn apply_voice_options(&self, query: &mut Value) {
        set_number(query, "speedScale", self.voice_options.speed_scale);
        set_number(query, "pitchScale", self.voice_options.pitch_scale);
        set_number(
            query,
            "intonationScale",
            self.voice_options.intonation_scale,
        );
        set_number(
            query,
            "pauseLengthScale",
            self.voice_options.pause_length_scale,
        );
        set_number(query, "volumeScale", self.voice_options.volume_scale);
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.endpoint, path)
    }
}

fn set_number(query: &mut Value, key: &str, value: f64) {
    if let Some(object) = query.as_object_mut()
        && object.contains_key(key)
    {
        object.insert(key.to_string(), json!(value));
    }
}

async fn voicevox_status_error(
    operation: &str,
    status: StatusCode,
    response: reqwest::Response,
) -> Error {
    let body = response.text().await.unwrap_or_default();
    let summary = summarize_body(&body);

    if summary.is_empty() {
        return anyhow!("VOICEVOX {} が失敗しました: HTTP {}", operation, status);
    }

    anyhow!(
        "VOICEVOX {} が失敗しました: HTTP {}: {}",
        operation,
        status,
        summary
    )
}

fn summarize_body(body: &str) -> String {
    body.lines()
        .take(8)
        .collect::<Vec<_>>()
        .join("\n")
        .chars()
        .take(1000)
        .collect()
}

pub async fn print_speakers(args: VoicevoxArgs) -> Result<()> {
    let mut loaded = config::load(args.config.as_deref())?;
    loaded.values.apply_overrides(args.config_overrides());
    loaded.values.validate()?;

    let client = VoicevoxClient::new(
        loaded.values.voicevox_endpoint,
        loaded.values.speaker,
        loaded.values.voice,
    );
    let speakers = client.speakers().await?;

    for speaker in speakers {
        println!("{}", speaker.name);
        for style in speaker.styles {
            println!("  - {}: {}", style.id, style.name);
        }
    }

    Ok(())
}

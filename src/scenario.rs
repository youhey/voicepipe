use std::{fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ScenarioExport {
    pub episode: Episode,
}

#[derive(Debug, Deserialize)]
pub struct Episode {
    pub episode_key: String,
    pub title: String,
    pub language: String,
    pub scenario_json: ScenarioJson,
}

#[derive(Debug, Deserialize)]
pub struct ScenarioJson {
    pub sections: Vec<Section>,
}

#[derive(Debug, Deserialize)]
pub struct Section {
    #[serde(rename = "type")]
    pub section_type: String,
    pub title: String,
    pub text: String,
    pub estimated_duration_seconds: Option<u64>,
}

pub fn load(path: &Path) -> Result<ScenarioExport> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("入力 JSON を読み込めません: {}", path.display()))?;

    serde_json::from_str(&source)
        .with_context(|| format!("入力 JSON の形式が正しくありません: {}", path.display()))
}

pub fn validate(scenario: &ScenarioExport) -> Result<()> {
    if scenario.episode.episode_key.trim().is_empty() {
        bail!("episode.episode_key は必須です");
    }

    if scenario.episode.language != "ja" {
        bail!(
            "episode.language は ja のみ対応しています: {}",
            scenario.episode.language
        );
    }

    if scenario.episode.scenario_json.sections.is_empty() {
        bail!("episode.scenario_json.sections は 1 件以上必要です");
    }

    for (index, section) in scenario.episode.scenario_json.sections.iter().enumerate() {
        if section.text.trim().is_empty() {
            bail!(
                "episode.scenario_json.sections[{}].text は空にできません",
                index
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_scenario() -> ScenarioExport {
        serde_json::from_str(
            r#"{
                "episode": {
                    "episode_key": "episode-001",
                    "title": "テスト回",
                    "language": "ja",
                    "scenario_json": {
                        "sections": [
                            {
                                "type": "opening",
                                "title": "オープニング",
                                "text": "こんにちは。",
                                "estimated_duration_seconds": 3
                            }
                        ]
                    }
                }
            }"#,
        )
        .expect("valid fixture should parse")
    }

    #[test]
    fn accepts_valid_scenario() {
        let scenario = valid_scenario();

        assert!(validate(&scenario).is_ok());
        assert_eq!(
            scenario.episode.scenario_json.sections[0].estimated_duration_seconds,
            Some(3)
        );
    }

    #[test]
    fn rejects_non_japanese_episode() {
        let mut scenario = valid_scenario();
        scenario.episode.language = "en".to_string();

        assert!(validate(&scenario).is_err());
    }

    #[test]
    fn rejects_empty_sections() {
        let mut scenario = valid_scenario();
        scenario.episode.scenario_json.sections.clear();

        assert!(validate(&scenario).is_err());
    }

    #[test]
    fn rejects_empty_section_text() {
        let mut scenario = valid_scenario();
        scenario.episode.scenario_json.sections[0].text = "  ".to_string();

        assert!(validate(&scenario).is_err());
    }
}

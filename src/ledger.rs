use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

pub struct Ledger {
    connection: Connection,
}

impl Ledger {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "SQLite データベースディレクトリを作成できません: {}",
                    parent.display()
                )
            })?;
        }

        let connection = Connection::open(path)
            .with_context(|| format!("SQLite データベースを開けません: {}", path.display()))?;
        let ledger = Self { connection };
        ledger.migrate()?;

        Ok(ledger)
    }

    pub fn uploaded_episode_keys(&self) -> Result<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT episode_key FROM episodes WHERE status = 'uploaded'")
            .context("uploaded episode の照会を準備できません")?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .context("uploaded episode を照会できません")?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("uploaded episode を読み込めません")
    }

    pub fn upsert_pending(&self, episode_key: &str) -> Result<()> {
        self.connection
            .execute(
                r#"
                INSERT INTO episodes (episode_key, status, created_at, updated_at)
                VALUES (?1, 'pending', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))
                ON CONFLICT(episode_key) DO UPDATE SET
                    status = 'pending',
                    error_message = NULL,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                "#,
                params![episode_key],
            )
            .with_context(|| format!("ledger pending 更新に失敗しました: {episode_key}"))?;

        Ok(())
    }

    pub fn mark_fetched(&self, episode_key: &str, json_path: &Path) -> Result<()> {
        self.update_status(
            episode_key,
            "fetched",
            Some(("json_path", json_path.to_path_buf())),
            "upstream_fetched_at",
        )
    }

    pub fn mark_recorded(
        &self,
        episode_key: &str,
        audio_path: &Path,
        recorded_at: &str,
        audio_duration_seconds: u64,
    ) -> Result<()> {
        let existing = self
            .connection
            .query_row(
                "SELECT episode_key FROM episodes WHERE episode_key = ?1",
                params![episode_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .context("ledger episode 存在確認に失敗しました")?;

        if existing.is_none() {
            self.upsert_pending(episode_key)?;
        }

        self.connection
            .execute(
                r#"
                UPDATE episodes
                SET status = 'recorded',
                    audio_path = ?2,
                    recorded_at = ?3,
                    audio_duration_seconds = ?4,
                    error_message = NULL,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                WHERE episode_key = ?1
                "#,
                params![
                    episode_key,
                    audio_path.display().to_string(),
                    recorded_at,
                    audio_duration_seconds
                ],
            )
            .with_context(|| format!("ledger recorded 更新に失敗しました: {episode_key}"))?;

        Ok(())
    }

    pub fn mark_uploaded(&self, episode_key: &str) -> Result<()> {
        self.connection
            .execute(
                r#"
                UPDATE episodes
                SET status = 'uploaded',
                    uploaded_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                    error_message = NULL,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                WHERE episode_key = ?1
                "#,
                params![episode_key],
            )
            .with_context(|| format!("ledger uploaded 更新に失敗しました: {episode_key}"))?;

        Ok(())
    }

    pub fn mark_failed(&self, episode_key: &str, error: &str) -> Result<()> {
        self.connection
            .execute(
                r#"
                INSERT INTO episodes (episode_key, status, error_message, created_at, updated_at)
                VALUES (?1, 'failed', ?2, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))
                ON CONFLICT(episode_key) DO UPDATE SET
                    status = 'failed',
                    error_message = excluded.error_message,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                "#,
                params![episode_key, summarize_error(error)],
            )
            .with_context(|| format!("ledger failed 更新に失敗しました: {episode_key}"))?;

        Ok(())
    }

    fn migrate(&self) -> Result<()> {
        self.connection
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS episodes (
                    episode_key TEXT PRIMARY KEY,
                    status TEXT NOT NULL CHECK (status IN ('pending', 'fetched', 'recorded', 'uploaded', 'failed')),
                    json_path TEXT,
                    audio_path TEXT,
                    upstream_fetched_at TEXT,
                    recorded_at TEXT,
                    audio_duration_seconds INTEGER,
                    uploaded_at TEXT,
                    error_message TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_episodes_status ON episodes(status);
                "#,
            )
            .context("ledger schema migration に失敗しました")?;
        self.ensure_column("audio_duration_seconds", "INTEGER")?;

        Ok(())
    }

    fn ensure_column(&self, name: &str, definition: &str) -> Result<()> {
        let mut statement = self
            .connection
            .prepare("PRAGMA table_info(episodes)")
            .context("ledger schema 確認を準備できません")?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .context("ledger schema を確認できません")?
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("ledger schema の列を読み込めません")?;

        if columns.iter().any(|column| column == name) {
            return Ok(());
        }

        self.connection
            .execute(
                &format!("ALTER TABLE episodes ADD COLUMN {name} {definition}"),
                [],
            )
            .with_context(|| format!("ledger schema に {name} を追加できません"))?;

        Ok(())
    }

    fn update_status(
        &self,
        episode_key: &str,
        status: &str,
        path_value: Option<(&str, PathBuf)>,
        timestamp_column: &str,
    ) -> Result<()> {
        let existing = self
            .connection
            .query_row(
                "SELECT episode_key FROM episodes WHERE episode_key = ?1",
                params![episode_key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .context("ledger episode 存在確認に失敗しました")?;

        if existing.is_none() {
            self.upsert_pending(episode_key)?;
        }

        let path_string = path_value
            .as_ref()
            .map(|(_, path)| path.display().to_string())
            .unwrap_or_default();

        let sql = match (
            path_value.as_ref().map(|(column, _)| *column),
            timestamp_column,
        ) {
            (Some("json_path"), "upstream_fetched_at") => {
                r#"
                UPDATE episodes
                SET status = ?2,
                    json_path = ?3,
                    upstream_fetched_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                    error_message = NULL,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                WHERE episode_key = ?1
                "#
            }
            _ => unreachable!("unsupported ledger status update"),
        };

        self.connection
            .execute(sql, params![episode_key, status, path_string])
            .with_context(|| format!("ledger {status} 更新に失敗しました: {episode_key}"))?;

        Ok(())
    }
}

fn summarize_error(error: &str) -> String {
    error.chars().take(2000).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_uploaded_episode_keys() {
        let temp_dir =
            std::env::temp_dir().join(format!("voicepipe-ledger-test-{}", std::process::id()));
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let db_path = temp_dir.join("voicepipe.sqlite");
        let ledger = Ledger::open(&db_path).expect("ledger should open");

        ledger
            .upsert_pending("episode-001")
            .expect("pending should update");
        ledger
            .mark_fetched("episode-001", Path::new("dist/json/episode-001.json"))
            .expect("fetched should update");
        ledger
            .mark_recorded(
                "episode-001",
                Path::new("dist/record/episode-001.mp3"),
                "2026-05-31T04:12:30Z",
                842,
            )
            .expect("recorded should update");
        ledger
            .mark_uploaded("episode-001")
            .expect("uploaded should update");

        assert_eq!(
            ledger
                .uploaded_episode_keys()
                .expect("uploaded keys should load"),
            vec!["episode-001".to_string()]
        );

        fs::remove_dir_all(temp_dir).expect("temp dir should be removed");
    }

    #[test]
    fn stores_recording_metadata_separately_from_upload_time() {
        let temp_dir = std::env::temp_dir().join(format!(
            "voicepipe-ledger-recording-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let db_path = temp_dir.join("voicepipe.sqlite");
        let ledger = Ledger::open(&db_path).expect("ledger should open");

        ledger
            .mark_recorded(
                "episode-001",
                Path::new("dist/record/episode-001.mp3"),
                "2026-05-31T04:12:30Z",
                842,
            )
            .expect("recorded should update");
        ledger
            .mark_uploaded("episode-001")
            .expect("uploaded should update");

        let (recorded_at, duration, uploaded_at): (String, u64, String) = ledger
            .connection
            .query_row(
                "SELECT recorded_at, audio_duration_seconds, uploaded_at FROM episodes WHERE episode_key = ?1",
                params!["episode-001"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("recording metadata should load");

        assert_eq!(recorded_at, "2026-05-31T04:12:30Z");
        assert_eq!(duration, 842);
        assert!(!uploaded_at.is_empty());

        fs::remove_dir_all(temp_dir).expect("temp dir should be removed");
    }

    #[test]
    fn truncates_failed_error_message() {
        let message = "a".repeat(2100);

        assert_eq!(summarize_error(&message).chars().count(), 2000);
    }
}

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

pub struct Ledger {
    connection: Connection,
}

pub struct DownstreamSyncMetadata {
    pub audio_sha256: String,
    pub episode_json_sha256: String,
    pub audio_size_bytes: u64,
    pub episode_json_size_bytes: u64,
    pub downstream_status: String,
    pub last_upload_method: String,
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

    pub fn mark_downstream_synced(
        &self,
        episode_key: &str,
        metadata: &DownstreamSyncMetadata,
        upload_performed: bool,
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
                SET status = 'uploaded',
                    audio_sha256 = ?2,
                    episode_json_sha256 = ?3,
                    audio_size_bytes = ?4,
                    episode_json_size_bytes = ?5,
                    downstream_synced_at = strftime('%Y-%m-%dT%H:%M:%SZ','now'),
                    downstream_status = ?6,
                    last_upload_method = ?7,
                    uploaded_at = CASE
                        WHEN ?8 THEN strftime('%Y-%m-%dT%H:%M:%SZ','now')
                        ELSE uploaded_at
                    END,
                    error_message = NULL,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                WHERE episode_key = ?1
                "#,
                params![
                    episode_key,
                    metadata.audio_sha256,
                    metadata.episode_json_sha256,
                    metadata.audio_size_bytes,
                    metadata.episode_json_size_bytes,
                    metadata.downstream_status,
                    metadata.last_upload_method,
                    upload_performed,
                ],
            )
            .with_context(|| format!("ledger downstream 同期更新に失敗しました: {episode_key}"))?;

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
                    audio_sha256 TEXT,
                    episode_json_sha256 TEXT,
                    audio_size_bytes INTEGER,
                    episode_json_size_bytes INTEGER,
                    downstream_synced_at TEXT,
                    downstream_status TEXT,
                    last_upload_method TEXT,
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
        self.ensure_column("audio_sha256", "TEXT")?;
        self.ensure_column("episode_json_sha256", "TEXT")?;
        self.ensure_column("audio_size_bytes", "INTEGER")?;
        self.ensure_column("episode_json_size_bytes", "INTEGER")?;
        self.ensure_column("downstream_synced_at", "TEXT")?;
        self.ensure_column("downstream_status", "TEXT")?;
        self.ensure_column("last_upload_method", "TEXT")?;

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
            .mark_downstream_synced(
                "episode-001",
                &DownstreamSyncMetadata {
                    audio_sha256: "audio-hash".to_string(),
                    episode_json_sha256: "json-hash".to_string(),
                    audio_size_bytes: 10,
                    episode_json_size_bytes: 20,
                    downstream_status: "created".to_string(),
                    last_upload_method: "post".to_string(),
                },
                true,
            )
            .expect("sync metadata should update");

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

    #[test]
    fn stores_downstream_sync_metadata() {
        let temp_dir =
            std::env::temp_dir().join(format!("voicepipe-ledger-sync-test-{}", std::process::id()));
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let db_path = temp_dir.join("voicepipe.sqlite");
        let ledger = Ledger::open(&db_path).expect("ledger should open");

        ledger
            .upsert_pending("episode-001")
            .expect("pending should update");
        ledger
            .mark_downstream_synced(
                "episode-001",
                &DownstreamSyncMetadata {
                    audio_sha256: "audio-hash".to_string(),
                    episode_json_sha256: "json-hash".to_string(),
                    audio_size_bytes: 10,
                    episode_json_size_bytes: 20,
                    downstream_status: "matched".to_string(),
                    last_upload_method: "skip".to_string(),
                },
                false,
            )
            .expect("sync metadata should update");

        let (
            status,
            audio_sha256,
            episode_json_sha256,
            audio_size_bytes,
            episode_json_size_bytes,
            downstream_synced_at,
            downstream_status,
            last_upload_method,
            uploaded_at,
        ): (
            String,
            String,
            String,
            u64,
            u64,
            String,
            String,
            String,
            Option<String>,
        ) = ledger
            .connection
            .query_row(
                "SELECT status, audio_sha256, episode_json_sha256, audio_size_bytes, episode_json_size_bytes, downstream_synced_at, downstream_status, last_upload_method, uploaded_at FROM episodes WHERE episode_key = ?1",
                params!["episode-001"],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                    ))
                },
            )
            .expect("sync metadata should load");

        assert_eq!(status, "uploaded");
        assert_eq!(audio_sha256, "audio-hash");
        assert_eq!(episode_json_sha256, "json-hash");
        assert_eq!(audio_size_bytes, 10);
        assert_eq!(episode_json_size_bytes, 20);
        assert!(!downstream_synced_at.is_empty());
        assert_eq!(downstream_status, "matched");
        assert_eq!(last_upload_method, "skip");
        assert!(uploaded_at.is_none());

        fs::remove_dir_all(temp_dir).expect("temp dir should be removed");
    }
}

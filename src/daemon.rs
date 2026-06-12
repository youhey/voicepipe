use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Days, LocalResult, NaiveDate, NaiveTime, SecondsFormat, TimeZone, Utc};
use chrono_tz::Tz;
use rusqlite::{Connection, OptionalExtension, params};
use tokio::{sync::Notify, time::sleep};

use crate::{
    audio,
    cli::DaemonArgs,
    config::{self, DaemonMode, DaemonScheduleConfig, ResolvedConfig},
    ffmpeg, onair,
    upstream::UpstreamClient,
    voicevox::VoicevoxClient,
};

pub async fn run(args: DaemonArgs) -> Result<()> {
    let mut loaded_config = config::load(args.config.as_deref())?;
    apply_daemon_overrides(&args, &mut loaded_config.values);
    loaded_config.values.validate()?;

    validate_environment(&loaded_config.values).await?;

    println!("voicepipe daemon started");
    println!("mode={}", loaded_config.values.daemon.mode);

    let shutdown = Shutdown::new();
    shutdown.listen();

    match loaded_config.values.daemon.mode {
        DaemonMode::Interval => {
            run_interval_loop(&args, &loaded_config.values, &shutdown).await?;
        }
        DaemonMode::Schedule => {
            run_schedule_loop(&args, &loaded_config.values, &shutdown).await?;
        }
    }

    println!("voicepipe daemon stopped");

    Ok(())
}

fn apply_daemon_overrides(args: &DaemonArgs, config: &mut ResolvedConfig) {
    if let Some(mode) = args.mode {
        config.daemon.mode = mode;
    }
    if let Some(interval) = args.interval {
        config.daemon.interval = interval;
    }
    if let Some(timezone) = &args.timezone {
        config.daemon.schedule.timezone = timezone.clone();
    }
    if !args.schedule_times.is_empty() {
        config.daemon.schedule.times = args.schedule_times.clone();
    }
}

async fn run_interval_loop(
    args: &DaemonArgs,
    config: &ResolvedConfig,
    shutdown: &Shutdown,
) -> Result<()> {
    let interval = config.daemon.interval;
    if interval == 0 {
        bail!("daemon.interval は 1 以上を指定してください");
    }
    println!("interval={interval}");

    loop {
        println!("running onair cycle...");
        if let Err(error) = onair::run_onair_once(args.to_onair_args()).await {
            println!("onair cycle failed: {error:#}");
        }

        if args.once || shutdown.is_requested() {
            break;
        }

        println!("sleeping {interval} seconds");
        tokio::select! {
            () = sleep(Duration::from_secs(interval)) => {}
            () = shutdown.notified() => {
                break;
            }
        }
    }

    Ok(())
}

async fn run_schedule_loop(
    args: &DaemonArgs,
    config: &ResolvedConfig,
    shutdown: &Shutdown,
) -> Result<()> {
    let schedule = ScheduleSpec::new(&config.daemon.schedule)?;
    let ledger = ScheduleLedger::open(&config.onair_database)?;

    println!("timezone={}", schedule.timezone);
    println!(
        "times={}",
        schedule
            .times
            .iter()
            .map(|time| time.format("%H:%M").to_string())
            .collect::<Vec<_>>()
            .join(",")
    );

    loop {
        let slot = schedule.next_after(Utc::now())?;
        println!("next onair run: {}", slot.scheduled_at);
        sleep_until(slot.scheduled_at_utc, shutdown).await;
        if shutdown.is_requested() {
            break;
        }

        if ledger.is_completed(&slot)? {
            println!(
                "scheduled onair already completed: {} {} {}",
                slot.scheduled_date, slot.scheduled_time, slot.timezone
            );
            if args.once {
                break;
            }
            continue;
        }

        let started_at = Utc::now();
        ledger.mark_running(&slot, started_at)?;
        println!(
            "starting scheduled onair: {} {} {}",
            slot.scheduled_date, slot.scheduled_time, slot.timezone
        );

        let result = onair::run_onair_once(args.to_onair_args()).await;
        let finished_at = Utc::now();

        match result {
            Ok(()) => {
                ledger.mark_finished(&slot, finished_at, "completed", None)?;
                println!("scheduled onair completed");
            }
            Err(error) => {
                let message = format!("{error:#}");
                ledger.mark_finished(&slot, finished_at, "failed", Some(&message))?;
                println!("scheduled onair failed: {message}");
            }
        }
        mark_overlapped_schedule_slots(&ledger, &schedule, &slot, started_at, finished_at)?;

        if args.once || shutdown.is_requested() {
            break;
        }
    }

    Ok(())
}

async fn sleep_until(scheduled_at_utc: DateTime<Utc>, shutdown: &Shutdown) {
    let duration = scheduled_at_utc
        .signed_duration_since(Utc::now())
        .to_std()
        .unwrap_or_else(|_| Duration::from_secs(0));

    tokio::select! {
        () = sleep(duration) => {}
        () = shutdown.notified() => {}
    }
}

fn mark_overlapped_schedule_slots(
    ledger: &ScheduleLedger,
    schedule: &ScheduleSpec,
    current_slot: &ScheduleSlot,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
) -> Result<()> {
    let mut cursor = started_at;
    loop {
        let slot = schedule.next_after(cursor)?;
        if slot.scheduled_at_utc > finished_at {
            break;
        }
        if slot.key() != current_slot.key() && !ledger.has_record(&slot)? {
            ledger.mark_skipped(&slot, finished_at, "previous run still running")?;
            println!(
                "scheduled onair skipped: {} {} {}: previous run still running",
                slot.scheduled_date, slot.scheduled_time, slot.timezone
            );
        }
        cursor = slot.scheduled_at_utc;
    }

    Ok(())
}

#[derive(Debug)]
struct ScheduleSpec {
    timezone: String,
    timezone_value: Tz,
    times: Vec<NaiveTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScheduleSlot {
    scheduled_date: String,
    scheduled_time: String,
    timezone: String,
    scheduled_at: String,
    scheduled_at_utc: DateTime<Utc>,
}

impl ScheduleSlot {
    fn key(&self) -> (&str, &str, &str) {
        (&self.scheduled_date, &self.scheduled_time, &self.timezone)
    }
}

impl ScheduleSpec {
    fn new(config: &DaemonScheduleConfig) -> Result<Self> {
        let timezone = config.timezone.trim().to_string();
        let timezone_value = timezone
            .parse::<Tz>()
            .with_context(|| format!("daemon.schedule.timezone が不正です: {timezone}"))?;
        let mut times = config
            .times
            .iter()
            .map(|value| parse_schedule_time(value))
            .collect::<Result<Vec<_>>>()?;
        times.sort_unstable();
        times.dedup();

        if times.is_empty() {
            bail!("daemon.schedule.times は 1 件以上指定してください");
        }

        Ok(Self {
            timezone,
            timezone_value,
            times,
        })
    }

    fn next_after(&self, now_utc: DateTime<Utc>) -> Result<ScheduleSlot> {
        let local_now = now_utc.with_timezone(&self.timezone_value);
        let local_date = local_now.date_naive();

        for day_offset in 0..=370 {
            let date = local_date
                .checked_add_days(Days::new(day_offset))
                .context("schedule date の計算に失敗しました")?;
            for time in &self.times {
                if let Some(slot) = self.slot_for(date, *time)?
                    && slot.scheduled_at_utc > now_utc
                {
                    return Ok(slot);
                }
            }
        }

        bail!("次回の schedule slot を計算できません");
    }

    fn slot_for(&self, date: NaiveDate, time: NaiveTime) -> Result<Option<ScheduleSlot>> {
        let local_naive = date.and_time(time);
        let local = match self.timezone_value.from_local_datetime(&local_naive) {
            LocalResult::Single(value) => value,
            LocalResult::Ambiguous(earliest, _) => earliest,
            LocalResult::None => return Ok(None),
        };
        let scheduled_at_utc = local.with_timezone(&Utc);

        Ok(Some(ScheduleSlot {
            scheduled_date: date.to_string(),
            scheduled_time: time.format("%H:%M").to_string(),
            timezone: self.timezone.clone(),
            scheduled_at: local.to_rfc3339_opts(SecondsFormat::Secs, true),
            scheduled_at_utc,
        }))
    }
}

fn parse_schedule_time(value: &str) -> Result<NaiveTime> {
    let Some((hour, minute)) = value.split_once(':') else {
        bail!("schedule time は HH:MM 形式で指定してください: {value}");
    };
    if hour.len() != 2 || minute.len() != 2 {
        bail!("schedule time は HH:MM 形式で指定してください: {value}");
    }
    let hour = hour
        .parse::<u32>()
        .with_context(|| format!("schedule time の hour が不正です: {value}"))?;
    let minute = minute
        .parse::<u32>()
        .with_context(|| format!("schedule time の minute が不正です: {value}"))?;

    NaiveTime::from_hms_opt(hour, minute, 0)
        .with_context(|| format!("schedule time が範囲外です: {value}"))
}

struct ScheduleLedger {
    connection: Connection,
}

impl ScheduleLedger {
    fn open(path: &Path) -> Result<Self> {
        let absolute = audio::absolute_path(path)?;
        if let Some(parent) = absolute.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "SQLite データベースディレクトリを作成できません: {}",
                    parent.display()
                )
            })?;
        }

        let connection = Connection::open(&absolute)
            .with_context(|| format!("SQLite データベースを開けません: {}", absolute.display()))?;
        let ledger = Self { connection };
        ledger.migrate()?;
        Ok(ledger)
    }

    fn migrate(&self) -> Result<()> {
        self.connection
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS schedule_runs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    scheduled_date TEXT NOT NULL,
                    scheduled_time TEXT NOT NULL,
                    timezone TEXT NOT NULL,
                    scheduled_at TEXT NOT NULL,
                    started_at TEXT,
                    finished_at TEXT,
                    status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'failed', 'skipped')),
                    error_message TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    UNIQUE (scheduled_date, scheduled_time, timezone)
                );

                CREATE INDEX IF NOT EXISTS idx_schedule_runs_slot
                    ON schedule_runs(scheduled_date, scheduled_time, timezone);
                "#,
            )
            .context("schedule_runs schema migration に失敗しました")?;

        Ok(())
    }

    fn has_record(&self, slot: &ScheduleSlot) -> Result<bool> {
        self.connection
            .query_row(
                r#"
                SELECT 1 FROM schedule_runs
                WHERE scheduled_date = ?1 AND scheduled_time = ?2 AND timezone = ?3
                "#,
                params![slot.scheduled_date, slot.scheduled_time, slot.timezone],
                |_| Ok(()),
            )
            .optional()
            .context("schedule_runs の存在確認に失敗しました")
            .map(|value| value.is_some())
    }

    fn is_completed(&self, slot: &ScheduleSlot) -> Result<bool> {
        self.connection
            .query_row(
                r#"
                SELECT status FROM schedule_runs
                WHERE scheduled_date = ?1 AND scheduled_time = ?2 AND timezone = ?3
                "#,
                params![slot.scheduled_date, slot.scheduled_time, slot.timezone],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .context("schedule_runs の status 確認に失敗しました")
            .map(|value| value.as_deref() == Some("completed"))
    }

    fn mark_running(&self, slot: &ScheduleSlot, started_at: DateTime<Utc>) -> Result<()> {
        self.connection
            .execute(
                r#"
                INSERT INTO schedule_runs (
                    scheduled_date,
                    scheduled_time,
                    timezone,
                    scheduled_at,
                    started_at,
                    status,
                    created_at,
                    updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, 'running', strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))
                ON CONFLICT(scheduled_date, scheduled_time, timezone) DO UPDATE SET
                    started_at = excluded.started_at,
                    finished_at = NULL,
                    status = 'running',
                    error_message = NULL,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                WHERE schedule_runs.status != 'completed'
                "#,
                params![
                    slot.scheduled_date,
                    slot.scheduled_time,
                    slot.timezone,
                    slot.scheduled_at,
                    utc_rfc3339(started_at),
                ],
            )
            .context("schedule_runs running 更新に失敗しました")?;

        Ok(())
    }

    fn mark_finished(
        &self,
        slot: &ScheduleSlot,
        finished_at: DateTime<Utc>,
        status: &str,
        error_message: Option<&str>,
    ) -> Result<()> {
        self.connection
            .execute(
                r#"
                UPDATE schedule_runs
                SET finished_at = ?4,
                    status = ?5,
                    error_message = ?6,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                WHERE scheduled_date = ?1 AND scheduled_time = ?2 AND timezone = ?3
                "#,
                params![
                    slot.scheduled_date,
                    slot.scheduled_time,
                    slot.timezone,
                    utc_rfc3339(finished_at),
                    status,
                    error_message.map(summarize_error),
                ],
            )
            .context("schedule_runs finished 更新に失敗しました")?;

        Ok(())
    }

    fn mark_skipped(
        &self,
        slot: &ScheduleSlot,
        finished_at: DateTime<Utc>,
        reason: &str,
    ) -> Result<()> {
        self.connection
            .execute(
                r#"
                INSERT INTO schedule_runs (
                    scheduled_date,
                    scheduled_time,
                    timezone,
                    scheduled_at,
                    finished_at,
                    status,
                    error_message,
                    created_at,
                    updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, 'skipped', ?6, strftime('%Y-%m-%dT%H:%M:%SZ','now'), strftime('%Y-%m-%dT%H:%M:%SZ','now'))
                ON CONFLICT(scheduled_date, scheduled_time, timezone) DO NOTHING
                "#,
                params![
                    slot.scheduled_date,
                    slot.scheduled_time,
                    slot.timezone,
                    slot.scheduled_at,
                    utc_rfc3339(finished_at),
                    summarize_error(reason),
                ],
            )
            .context("schedule_runs skipped 更新に失敗しました")?;

        Ok(())
    }
}

fn utc_rfc3339(datetime: DateTime<Utc>) -> String {
    datetime.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn summarize_error(error: &str) -> String {
    error.chars().take(2000).collect()
}

async fn validate_environment(config: &ResolvedConfig) -> Result<()> {
    let upstream_url = config
        .upstream_episode_url
        .as_deref()
        .context("daemon には [upstream].episode_url が必要です")?;

    ffmpeg::ensure_available()?;
    ffmpeg::ensure_probe_available()?;

    let voicevox = VoicevoxClient::new(
        config.voicevox_endpoint.clone(),
        config.speaker,
        config.voice.clone(),
    );
    if let Err(error) = voicevox.ensure_ready().await {
        println!("warning: daemon 起動前の VOICEVOX 到達確認に失敗しました: {error:#}");
    }

    let upstream = UpstreamClient::new(resolve_upstream_access_token(config));
    if let Err(error) = upstream.list_episodes(upstream_url).await {
        println!("warning: daemon 起動前の upstream 到達確認に失敗しました: {error:#}");
    }

    ensure_sqlite_writable(&config.onair_database)?;
    ensure_directory_writable(Path::new("dist"))?;
    ensure_directory_writable(Path::new("work"))?;

    Ok(())
}

fn resolve_upstream_access_token(config: &ResolvedConfig) -> Option<String> {
    std::env::var("VOICEPIPE_UPSTREAM_ACCESS_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty())
        .or_else(|| config.upstream_access_token.clone())
}

fn ensure_sqlite_writable(path: &Path) -> Result<()> {
    let absolute = audio::absolute_path(path)?;
    if let Some(parent) = absolute.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "SQLite データベースディレクトリを作成できません: {}",
                parent.display()
            )
        })?;
    }

    let connection = rusqlite::Connection::open(&absolute).with_context(|| {
        format!(
            "SQLite データベースを書き込み用に開けません: {}",
            absolute.display()
        )
    })?;
    connection
        .execute_batch("CREATE TABLE IF NOT EXISTS daemon_writable_check (checked_at TEXT); DROP TABLE daemon_writable_check;")
        .with_context(|| format!("SQLite データベースに書き込めません: {}", absolute.display()))?;

    Ok(())
}

fn ensure_directory_writable(path: &Path) -> Result<()> {
    let absolute = audio::absolute_path(path)?;
    fs::create_dir_all(&absolute)
        .with_context(|| format!("ディレクトリを作成できません: {}", absolute.display()))?;

    let test_file = absolute.join(".voicepipe-daemon-write-test");
    fs::write(&test_file, b"voicepipe daemon\n")
        .with_context(|| format!("テストファイルを書き込めません: {}", test_file.display()))?;
    fs::remove_file(&test_file)
        .with_context(|| format!("テストファイルを削除できません: {}", test_file.display()))?;

    Ok(())
}

#[derive(Clone)]
struct Shutdown {
    requested: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Shutdown {
    fn new() -> Self {
        Self {
            requested: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    fn listen(&self) {
        let shutdown = self.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                println!("shutdown requested");
                shutdown.requested.store(true, Ordering::SeqCst);
                shutdown.notify.notify_waiters();
            }
        });
    }

    fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }

    async fn notified(&self) {
        self.notify.notified().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::path::PathBuf;

    #[test]
    fn calculates_next_tokyo_schedule_slot_before_morning_run() {
        let schedule = ScheduleSpec::new(&DaemonScheduleConfig {
            enabled: true,
            timezone: "Asia/Tokyo".to_string(),
            times: vec!["09:00".to_string(), "14:00".to_string()],
        })
        .expect("schedule should parse");
        let now = Utc
            .with_ymd_and_hms(2026, 6, 11, 23, 59, 0)
            .single()
            .expect("valid utc");

        let slot = schedule.next_after(now).expect("slot should calculate");

        assert_eq!(slot.scheduled_date, "2026-06-12");
        assert_eq!(slot.scheduled_time, "09:00");
        assert_eq!(slot.timezone, "Asia/Tokyo");
        assert_eq!(slot.scheduled_at, "2026-06-12T09:00:00+09:00");
        assert_eq!(
            slot.scheduled_at_utc,
            Utc.with_ymd_and_hms(2026, 6, 12, 0, 0, 0)
                .single()
                .expect("valid utc")
        );
    }

    #[test]
    fn calculates_next_tokyo_schedule_slot_after_morning_run() {
        let schedule = ScheduleSpec::new(&DaemonScheduleConfig {
            enabled: true,
            timezone: "Asia/Tokyo".to_string(),
            times: vec!["09:00".to_string(), "14:00".to_string()],
        })
        .expect("schedule should parse");
        let now = Utc
            .with_ymd_and_hms(2026, 6, 12, 0, 1, 0)
            .single()
            .expect("valid utc");

        let slot = schedule.next_after(now).expect("slot should calculate");

        assert_eq!(slot.scheduled_date, "2026-06-12");
        assert_eq!(slot.scheduled_time, "14:00");
        assert_eq!(slot.scheduled_at, "2026-06-12T14:00:00+09:00");
    }

    #[test]
    fn schedule_ledger_stores_completed_slot_and_prevents_duplicate() {
        let temp_dir = unique_temp_dir("voicepipe-schedule-ledger-test");
        fs::create_dir_all(&temp_dir).expect("temp dir should be created");
        let db_path = temp_dir.join("voicepipe.sqlite");
        let ledger = ScheduleLedger::open(&db_path).expect("ledger should open");
        let slot = ScheduleSlot {
            scheduled_date: "2026-06-12".to_string(),
            scheduled_time: "09:00".to_string(),
            timezone: "Asia/Tokyo".to_string(),
            scheduled_at: "2026-06-12T09:00:00+09:00".to_string(),
            scheduled_at_utc: Utc
                .with_ymd_and_hms(2026, 6, 12, 0, 0, 0)
                .single()
                .expect("valid utc"),
        };
        let started_at = Utc
            .with_ymd_and_hms(2026, 6, 12, 0, 0, 1)
            .single()
            .expect("valid utc");
        let finished_at = Utc
            .with_ymd_and_hms(2026, 6, 12, 0, 1, 0)
            .single()
            .expect("valid utc");

        ledger
            .mark_running(&slot, started_at)
            .expect("running should update");
        ledger
            .mark_finished(&slot, finished_at, "completed", None)
            .expect("finished should update");
        ledger
            .mark_running(&slot, finished_at)
            .expect("duplicate running should be ignored for completed slot");

        assert!(ledger.is_completed(&slot).expect("status should load"));

        let status: String = ledger
            .connection
            .query_row(
                "SELECT status FROM schedule_runs WHERE scheduled_date = ?1 AND scheduled_time = ?2 AND timezone = ?3",
                params![slot.scheduled_date, slot.scheduled_time, slot.timezone],
                |row| row.get(0),
            )
            .expect("status should load");
        assert_eq!(status, "completed");

        fs::remove_dir_all(temp_dir).expect("temp dir should be removed");
    }

    #[test]
    fn invalid_schedule_time_is_rejected() {
        assert!(parse_schedule_time("9:00").is_err());
        assert!(parse_schedule_time("24:00").is_err());
        assert!(parse_schedule_time("09:00").is_ok());
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{}-{}", prefix, std::process::id()))
    }
}

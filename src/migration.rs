use std::{
    fs::{self, File},
    io::{self, Cursor},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use chrono::{SecondsFormat, Utc};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde::{Deserialize, Serialize};
use tar::{Archive, Builder, EntryType, Header};

use crate::cli::{DumpArgs, RestoreArgs};

const MANIFEST_SCHEMA_VERSION: &str = "1.0";
const LAYOUT: &str = "dist-onair-v1";

#[derive(Debug, Serialize, Deserialize)]
struct DumpManifest {
    schema_version: String,
    created_at: String,
    voicepipe_version: String,
    layout: String,
}

#[derive(Debug)]
struct DumpSummary {
    output: PathBuf,
    config_included: bool,
    database_included: bool,
    episode_count: usize,
}

#[derive(Debug)]
struct RestoreSummary {
    config_restored: bool,
    database_restored: bool,
    episode_count: usize,
    manifest: DumpManifest,
}

#[derive(Debug, Clone, Copy)]
enum DumpLayout {
    Canonical,
    Legacy,
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        fs::create_dir_all(&path)
            .with_context(|| format!("一時ディレクトリを作成できません: {}", path.display()))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub fn dump(args: DumpArgs) -> Result<()> {
    println!("Creating dump...");
    let root = std::env::current_dir().context("現在のディレクトリを取得できません")?;
    let summary = create_dump(&root, &args.output, args.legacy_storage)?;

    println!("Including:");
    if summary.config_included {
        println!("- voicepipe.toml");
    }
    if summary.database_included {
        println!("- onair.sqlite");
    }
    println!("- {} episodes", summary.episode_count);
    println!();
    println!("Dump created:");
    println!("{}", summary.output.display());

    Ok(())
}

pub fn restore(args: RestoreArgs) -> Result<()> {
    println!("Restoring dump...");
    let root = std::env::current_dir().context("現在のディレクトリを取得できません")?;
    let summary = restore_dump(&root, &args.input, args.force)?;

    println!("Manifest:");
    println!("schema_version={}", summary.manifest.schema_version);
    println!("layout={}", summary.manifest.layout);
    println!();
    println!("Restored:");
    if summary.config_restored {
        println!("- voicepipe.toml");
    }
    if summary.database_restored {
        println!("- onair.sqlite");
    }
    println!("- {} episodes", summary.episode_count);

    Ok(())
}

fn create_dump(root: &Path, output: &Path, legacy_storage: bool) -> Result<DumpSummary> {
    let config_path = root.join("voicepipe.toml");
    if !config_path.exists() {
        bail!(
            "dump 対象の設定ファイルが見つかりません: {}",
            config_path.display()
        );
    }

    let layout = detect_dump_layout(root, legacy_storage)?;
    let database_path = match layout {
        DumpLayout::Canonical => root.join("dist/onair/onair.sqlite"),
        DumpLayout::Legacy => root.join("storage/onair/onair.sqlite"),
    };
    if !database_path.exists() {
        bail!(
            "dump 対象の SQLite ledger が見つかりません: {}",
            database_path.display()
        );
    }

    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "dump 出力ディレクトリを作成できません: {}",
                parent.display()
            )
        })?;
    }

    let file = File::create(output)
        .with_context(|| format!("dump を作成できません: {}", output.display()))?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = Builder::new(encoder);

    append_manifest(&mut builder)?;
    builder
        .append_path_with_name(&config_path, "config/voicepipe.toml")
        .with_context(|| {
            format!(
                "設定ファイルを archive に追加できません: {}",
                config_path.display()
            )
        })?;
    builder
        .append_path_with_name(&database_path, "database/onair.sqlite")
        .with_context(|| {
            format!(
                "SQLite ledger を archive に追加できません: {}",
                database_path.display()
            )
        })?;

    let episode_count = match layout {
        DumpLayout::Canonical => {
            let episodes_dir = root.join("dist/onair/episodes");
            append_episodes_dir(&mut builder, &episodes_dir)?
        }
        DumpLayout::Legacy => {
            let temp = TempDir::new("voicepipe-legacy-dump")?;
            let episodes_dir = temp.path().join("episodes");
            build_legacy_episode_export(root, &episodes_dir)?;
            append_episodes_dir(&mut builder, &episodes_dir)?
        }
    };

    builder
        .finish()
        .context("dump archive の書き込みを完了できません")?;

    Ok(DumpSummary {
        output: output.to_path_buf(),
        config_included: true,
        database_included: true,
        episode_count,
    })
}

fn restore_dump(root: &Path, input: &Path, force: bool) -> Result<RestoreSummary> {
    let staging = TempDir::new("voicepipe-restore")?;
    unpack_archive(input, staging.path())?;

    let manifest_path = staging.path().join("manifest.json");
    let manifest = read_manifest(&manifest_path)?;
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        bail!(
            "未対応の dump schema_version です: {}",
            manifest.schema_version
        );
    }
    if manifest.layout != LAYOUT {
        bail!("未対応の dump layout です: {}", manifest.layout);
    }

    let config_src = staging.path().join("config/voicepipe.toml");
    let database_src = staging.path().join("database/onair.sqlite");
    let episodes_src = staging.path().join("episodes");

    if !config_src.exists() {
        bail!("dump archive に config/voicepipe.toml が含まれていません");
    }
    if !database_src.exists() {
        bail!("dump archive に database/onair.sqlite が含まれていません");
    }

    let config_dst = root.join("voicepipe.toml");
    let database_dst = root.join("dist/onair/onair.sqlite");
    let episodes_dst = root.join("dist/onair/episodes");

    ensure_can_restore_file(&config_dst, force)?;
    ensure_can_restore_file(&database_dst, force)?;
    ensure_can_restore_dir(&episodes_dst, force)?;

    if force {
        remove_file_if_exists(&config_dst)?;
        remove_file_if_exists(&database_dst)?;
        remove_dir_if_exists(&episodes_dst)?;
    }

    copy_file(&config_src, &config_dst)?;
    copy_file(&database_src, &database_dst)?;

    let episode_count = if episodes_src.exists() {
        copy_dir_recursive(&episodes_src, &episodes_dst)?;
        count_episode_dirs(&episodes_src)?
    } else {
        fs::create_dir_all(&episodes_dst).with_context(|| {
            format!(
                "episode artifacts ディレクトリを作成できません: {}",
                episodes_dst.display()
            )
        })?;
        0
    };

    Ok(RestoreSummary {
        config_restored: true,
        database_restored: true,
        episode_count,
        manifest,
    })
}

fn detect_dump_layout(root: &Path, legacy_storage: bool) -> Result<DumpLayout> {
    if legacy_storage {
        return Ok(DumpLayout::Legacy);
    }

    if root.join("dist/onair/onair.sqlite").exists() || root.join("dist/onair/episodes").exists() {
        return Ok(DumpLayout::Canonical);
    }

    if root.join("storage/onair/onair.sqlite").exists()
        || root.join("storage/onair/episodes").exists()
        || root.join("storage/json").exists()
        || root.join("storage/audio").exists()
    {
        return Ok(DumpLayout::Legacy);
    }

    Ok(DumpLayout::Canonical)
}

fn append_manifest<W: io::Write>(builder: &mut Builder<W>) -> Result<()> {
    let manifest = DumpManifest {
        schema_version: MANIFEST_SCHEMA_VERSION.to_string(),
        created_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        voicepipe_version: env!("CARGO_PKG_VERSION").to_string(),
        layout: LAYOUT.to_string(),
    };
    let payload = serde_json::to_vec_pretty(&manifest).context("manifest JSON を生成できません")?;
    let mut header = Header::new_gnu();
    header.set_size(payload.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, "manifest.json", Cursor::new(payload))
        .context("manifest.json を archive に追加できません")?;
    Ok(())
}

fn append_episodes_dir<W: io::Write>(
    builder: &mut Builder<W>,
    episodes_dir: &Path,
) -> Result<usize> {
    if episodes_dir.exists() {
        builder
            .append_dir_all("episodes", episodes_dir)
            .with_context(|| {
                format!(
                    "episode artifacts を archive に追加できません: {}",
                    episodes_dir.display()
                )
            })?;
        count_episode_dirs(episodes_dir)
    } else {
        let temp = TempDir::new("voicepipe-empty-episodes")?;
        let empty = temp.path().join("episodes");
        fs::create_dir_all(&empty).with_context(|| {
            format!(
                "空の episode artifacts ディレクトリを作成できません: {}",
                empty.display()
            )
        })?;
        builder
            .append_dir_all("episodes", &empty)
            .context("空の episode artifacts を archive に追加できません")?;
        Ok(0)
    }
}

fn build_legacy_episode_export(root: &Path, episodes_dir: &Path) -> Result<()> {
    let legacy_episodes = root.join("storage/onair/episodes");
    if legacy_episodes.exists() {
        copy_dir_recursive(&legacy_episodes, episodes_dir)?;
    } else {
        fs::create_dir_all(episodes_dir).with_context(|| {
            format!(
                "legacy episode export ディレクトリを作成できません: {}",
                episodes_dir.display()
            )
        })?;
    }

    copy_legacy_named_files(
        &root.join("storage/json"),
        episodes_dir,
        "json",
        "episode.json",
    )?;
    copy_legacy_named_files(
        &root.join("storage/audio"),
        episodes_dir,
        "mp3",
        "audio.mp3",
    )?;

    Ok(())
}

fn copy_legacy_named_files(
    source_dir: &Path,
    episodes_dir: &Path,
    extension: &str,
    target_name: &str,
) -> Result<()> {
    if !source_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(source_dir).with_context(|| {
        format!(
            "legacy ディレクトリを読み込めません: {}",
            source_dir.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!(
                "legacy ディレクトリエントリを読み込めません: {}",
                source_dir.display()
            )
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some(extension) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let target_dir = episodes_dir.join(stem);
        fs::create_dir_all(&target_dir).with_context(|| {
            format!(
                "legacy episode ディレクトリを作成できません: {}",
                target_dir.display()
            )
        })?;
        copy_file(&path, &target_dir.join(target_name))?;
    }

    Ok(())
}

fn unpack_archive(input: &Path, staging: &Path) -> Result<()> {
    let file = File::open(input)
        .with_context(|| format!("dump archive を開けません: {}", input.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);

    for entry in archive
        .entries()
        .context("dump archive の entries を読み込めません")?
    {
        let mut entry = entry.context("dump archive entry を読み込めません")?;
        let path = entry
            .path()
            .context("dump archive entry path を読み込めません")?;
        let safe_path = safe_archive_path(&path)?;
        let target = staging.join(safe_path);
        let entry_type = entry.header().entry_type();

        if entry_type == EntryType::Directory {
            fs::create_dir_all(&target).with_context(|| {
                format!("archive ディレクトリを展開できません: {}", target.display())
            })?;
        } else if entry_type == EntryType::Regular || entry_type == EntryType::GNUSparse {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "archive 展開先ディレクトリを作成できません: {}",
                        parent.display()
                    )
                })?;
            }
            entry
                .unpack(&target)
                .with_context(|| format!("archive entry を展開できません: {}", target.display()))?;
        } else {
            bail!("dump archive に未対応の entry type が含まれています");
        }
    }

    Ok(())
}

fn safe_archive_path(path: &Path) -> Result<PathBuf> {
    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => safe.push(value),
            Component::CurDir => {}
            _ => bail!(
                "dump archive に不正な path が含まれています: {}",
                path.display()
            ),
        }
    }
    if safe.as_os_str().is_empty() {
        bail!("dump archive に空の path が含まれています");
    }
    Ok(safe)
}

fn read_manifest(path: &Path) -> Result<DumpManifest> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("manifest を読み込めません: {}", path.display()))?;
    serde_json::from_str(&source)
        .with_context(|| format!("manifest JSON を解析できません: {}", path.display()))
}

fn ensure_can_restore_file(path: &Path, force: bool) -> Result<()> {
    if !force && path.exists() {
        bail!(
            "restore 先のファイルが既に存在します。上書きする場合は --force を指定してください: {}",
            path.display()
        );
    }
    Ok(())
}

fn ensure_can_restore_dir(path: &Path, force: bool) -> Result<()> {
    if force || !path.exists() {
        return Ok(());
    }

    let has_data = fs::read_dir(path)
        .with_context(|| format!("restore 先ディレクトリを確認できません: {}", path.display()))?
        .next()
        .transpose()
        .with_context(|| format!("restore 先ディレクトリを確認できません: {}", path.display()))?
        .is_some();
    if has_data {
        bail!(
            "restore 先の episode artifacts が既に存在します。上書きする場合は --force を指定してください: {}",
            path.display()
        );
    }

    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("既存ファイルを削除できません: {}", path.display()))?;
    }
    Ok(())
}

fn remove_dir_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)
            .with_context(|| format!("既存ディレクトリを削除できません: {}", path.display()))?;
    }
    Ok(())
}

fn copy_file(source: &Path, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).with_context(|| {
            format!("コピー先ディレクトリを作成できません: {}", parent.display())
        })?;
    }
    fs::copy(source, target).with_context(|| {
        format!(
            "ファイルをコピーできません: {} -> {}",
            source.display(),
            target.display()
        )
    })?;
    Ok(())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)
        .with_context(|| format!("ディレクトリを作成できません: {}", target.display()))?;

    for entry in fs::read_dir(source)
        .with_context(|| format!("ディレクトリを読み込めません: {}", source.display()))?
    {
        let entry = entry.with_context(|| {
            format!("ディレクトリエントリを読み込めません: {}", source.display())
        })?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let metadata = entry.metadata().with_context(|| {
            format!(
                "ファイル metadata を取得できません: {}",
                source_path.display()
            )
        })?;

        if metadata.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else if metadata.is_file() {
            copy_file(&source_path, &target_path)?;
        }
    }

    Ok(())
}

fn count_episode_dirs(path: &Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }

    let mut count = 0;
    for entry in fs::read_dir(path)
        .with_context(|| format!("episodes を読み込めません: {}", path.display()))?
    {
        let entry = entry
            .with_context(|| format!("episodes entry を読み込めません: {}", path.display()))?;
        if entry
            .metadata()
            .with_context(|| {
                format!(
                    "episodes entry metadata を取得できません: {}",
                    path.display()
                )
            })?
            .is_dir()
        {
            count += 1;
        }
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dump_and_restore_canonical_layout() {
        let source = unique_temp_dir("voicepipe-dump-source");
        let target = unique_temp_dir("voicepipe-dump-target");
        fs::create_dir_all(source.join("dist/onair/episodes/episode-001"))
            .expect("source episode dir should be created");
        fs::write(
            source.join("voicepipe.toml"),
            "[daemon]\nmode = \"interval\"\n",
        )
        .expect("config should be written");
        fs::write(source.join("dist/onair/onair.sqlite"), b"sqlite")
            .expect("sqlite should be written");
        fs::write(
            source.join("dist/onair/episodes/episode-001/episode.json"),
            b"{}",
        )
        .expect("episode json should be written");
        fs::write(
            source.join("dist/onair/episodes/episode-001/audio.mp3"),
            b"mp3",
        )
        .expect("audio should be written");

        let archive = source.join("voicepipe-dump.tar.gz");
        let dump = create_dump(&source, &archive, false).expect("dump should be created");
        assert_eq!(dump.episode_count, 1);

        let restore = restore_dump(&target, &archive, false).expect("restore should succeed");

        assert_eq!(restore.manifest.schema_version, "1.0");
        assert_eq!(restore.episode_count, 1);
        assert!(target.join("voicepipe.toml").exists());
        assert!(target.join("dist/onair/onair.sqlite").exists());
        assert!(
            target
                .join("dist/onair/episodes/episode-001/episode.json")
                .exists()
        );
        assert!(
            target
                .join("dist/onair/episodes/episode-001/audio.mp3")
                .exists()
        );

        fs::remove_dir_all(source).expect("source should be removed");
        fs::remove_dir_all(target).expect("target should be removed");
    }

    #[test]
    fn restore_refuses_existing_targets_without_force() {
        let source = unique_temp_dir("voicepipe-restore-source");
        let target = unique_temp_dir("voicepipe-restore-target");
        fs::create_dir_all(source.join("dist/onair/episodes")).expect("source dirs");
        fs::write(
            source.join("voicepipe.toml"),
            "[daemon]\nmode = \"interval\"\n",
        )
        .expect("config");
        fs::write(source.join("dist/onair/onair.sqlite"), b"sqlite").expect("sqlite");
        fs::create_dir_all(&target).expect("target dir");
        fs::write(target.join("voicepipe.toml"), "local").expect("existing config");

        let archive = source.join("voicepipe-dump.tar.gz");
        create_dump(&source, &archive, false).expect("dump should be created");

        let error = restore_dump(&target, &archive, false).expect_err("restore should fail");
        assert!(error.to_string().contains("--force"));

        restore_dump(&target, &archive, true).expect("force restore should succeed");
        assert_eq!(
            fs::read_to_string(target.join("voicepipe.toml")).expect("config should read"),
            "[daemon]\nmode = \"interval\"\n"
        );

        fs::remove_dir_all(source).expect("source should be removed");
        fs::remove_dir_all(target).expect("target should be removed");
    }

    #[test]
    fn legacy_storage_is_exported_into_episode_layout() {
        let source = unique_temp_dir("voicepipe-legacy-source");
        fs::create_dir_all(source.join("storage/json")).expect("json dir");
        fs::create_dir_all(source.join("storage/audio")).expect("audio dir");
        fs::create_dir_all(source.join("storage/onair")).expect("onair dir");
        fs::write(
            source.join("voicepipe.toml"),
            "[daemon]\nmode = \"interval\"\n",
        )
        .expect("config");
        fs::write(source.join("storage/onair/onair.sqlite"), b"sqlite").expect("sqlite");
        fs::write(source.join("storage/json/episode-001.json"), b"{}").expect("json");
        fs::write(source.join("storage/audio/episode-001.mp3"), b"mp3").expect("audio");

        let archive = source.join("voicepipe-dump.tar.gz");
        let dump = create_dump(&source, &archive, true).expect("legacy dump should be created");
        assert_eq!(dump.episode_count, 1);

        let target = unique_temp_dir("voicepipe-legacy-target");
        restore_dump(&target, &archive, false).expect("restore should succeed");
        assert!(
            target
                .join("dist/onair/episodes/episode-001/episode.json")
                .exists()
        );
        assert!(
            target
                .join("dist/onair/episodes/episode-001/audio.mp3")
                .exists()
        );

        fs::remove_dir_all(source).expect("source should be removed");
        fs::remove_dir_all(target).expect("target should be removed");
    }

    #[test]
    fn unsafe_archive_path_is_rejected() {
        assert!(safe_archive_path(Path::new("../voicepipe.toml")).is_err());
        assert!(safe_archive_path(Path::new("/tmp/voicepipe.toml")).is_err());
        assert_eq!(
            safe_archive_path(Path::new("config/voicepipe.toml")).expect("path should be safe"),
            PathBuf::from("config/voicepipe.toml")
        );
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }
}

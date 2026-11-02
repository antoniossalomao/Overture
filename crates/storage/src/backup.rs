use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub format_version: u32,
    pub app_name: String,
    pub app_version: String,
    pub created_at: String,
    pub media_file_count: usize,
}

/// Creates a complete `.zip` backup containing an atomic SQLite snapshot (`VACUUM INTO`)
/// and all files inside `media_dir`.
pub async fn create_full_backup(
    pool: &SqlitePool,
    media_dir: &Path,
    dest_zip: &Path,
) -> Result<BackupManifest> {
    if let Some(parent) = dest_zip.parent() {
        fs::create_dir_all(parent).context("Failed to create backup destination directory")?;
    }

    let temp_db = std::env::temp_dir().join(format!(
        "overture_snapshot_{}.db",
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    if temp_db.exists() {
        let _ = fs::remove_file(&temp_db);
    }

    let escaped_temp = temp_db.to_string_lossy().replace('\'', "''");
    let vacuum_sql = format!("VACUUM INTO '{}'", escaped_temp);
    sqlx::query(&vacuum_sql)
        .execute(pool)
        .await
        .context("SQLite VACUUM INTO failed during backup")?;

    let db_bytes = fs::read(&temp_db).context("Failed to read temporary SQLite snapshot")?;
    let _ = fs::remove_file(&temp_db);

    let mut media_files: Vec<(String, PathBuf)> = Vec::new();
    if media_dir.exists() {
        collect_files_recursive(media_dir, media_dir, &mut media_files)?;
    }

    let manifest = BackupManifest {
        format_version: 1,
        app_name: "Overture".to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        media_file_count: media_files.len(),
    };
    let manifest_json = serde_json::to_vec_pretty(&manifest)?;

    let out_file = File::create(dest_zip).context("Failed to create backup archive file")?;
    let mut zip = ZipWriter::new(out_file);
    let opts = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    zip.start_file("manifest.json", opts)?;
    zip.write_all(&manifest_json)?;

    zip.start_file("database/overture.db", opts)?;
    zip.write_all(&db_bytes)?;

    for (rel_slash, full_path) in &media_files {
        let archive_entry = format!("media/{}", rel_slash);
        zip.start_file(archive_entry, opts)?;
        let data = fs::read(full_path)
            .with_context(|| format!("Failed to read media file {}", full_path.display()))?;
        zip.write_all(&data)?;
    }

    zip.finish().context("Failed to finalize backup archive")?;
    Ok(manifest)
}

/// Restores a `.zip` backup created by [`create_full_backup`] into `target_db_path` and `target_media_dir`.
/// Ensure any open `SqlitePool` pointing to `target_db_path` is closed before calling this on Windows.
pub fn restore_full_backup(
    src_zip: &Path,
    target_db_path: &Path,
    target_media_dir: &Path,
) -> Result<BackupManifest> {
    let file = File::open(src_zip).context("Failed to open backup archive")?;
    let mut archive = ZipArchive::new(file).context("Invalid ZIP backup archive")?;

    let manifest: BackupManifest = {
        let mut mf = archive
            .by_name("manifest.json")
            .context("Backup archive is missing manifest.json")?;
        let mut buf = Vec::new();
        mf.read_to_end(&mut buf)?;
        serde_json::from_slice(&buf).context("Invalid manifest.json in backup")?
    };

    if manifest.format_version != 1 {
        return Err(anyhow!(
            "Unsupported backup format version: {}",
            manifest.format_version
        ));
    }

    if let Some(parent) = target_db_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir_all(target_media_dir)?;

    // Remove WAL/SHM sidecars if present before replacing database file
    let wal_path = PathBuf::from(format!("{}-wal", target_db_path.display()));
    let shm_path = PathBuf::from(format!("{}-shm", target_db_path.display()));
    let _ = fs::remove_file(&wal_path);
    let _ = fs::remove_file(&shm_path);

    let mut found_db = false;
    for idx in 0..archive.len() {
        let mut entry = archive.by_index(idx)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        if name == "database/overture.db" {
            let mut db_out = File::create(target_db_path)
                .with_context(|| format!("Cannot write database {}", target_db_path.display()))?;
            std::io::copy(&mut entry, &mut db_out)?;
            found_db = true;
        } else if let Some(rel) = name.strip_prefix("media/") {
            // Prevent path traversal
            let safe_rel = Path::new(rel)
                .components()
                .filter(|c| matches!(c, std::path::Component::Normal(_)))
                .collect::<PathBuf>();
            if safe_rel.as_os_str().is_empty() {
                continue;
            }
            let out_path = target_media_dir.join(safe_rel);
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut m_out = File::create(&out_path)?;
            std::io::copy(&mut entry, &mut m_out)?;
        }
    }

    if !found_db {
        return Err(anyhow!("Backup archive did not contain database/overture.db"));
    }

    Ok(manifest)
}

fn collect_files_recursive(
    root: &Path,
    current: &Path,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(root, &path, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, path));
        }
    }
    Ok(())
}

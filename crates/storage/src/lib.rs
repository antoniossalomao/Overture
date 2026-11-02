//! SQLite persistence (`sqlx`), migrations, accent-insensitive search, reports, and backup/restore.

pub mod accent;
pub mod backup;
pub mod bible_seed;
pub mod service_resolver;

pub use accent::{canonical_lyrics_key, fold_search_text};
pub use backup::{BackupManifest, create_full_backup, restore_full_backup};
pub use bible_seed::{
    CANONICAL_BIBLE_BOOKS, CanonicalBookInfo, ParsedBibleRef, builtin_public_domain_bibles,
    canonical_book_by_order, default_studio_themes, load_full_tb1917_verses, parse_bible_reference,
};

use anyhow::{Context, Result};
use overture_core::{
    AppSettings, BibleFavorite, BibleVerse, BibleVersion, Group, MediaItem, MediaKind,
    MemberScheduleCount, ScheduleEntry, Service, ServiceItem, ServiceItemKind, ServiceItemParams,
    Song, SongPlayStat, SongSlide, TeamMember, Theme, ThemeConfig,
};
use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous};
use std::path::Path;
use std::str::FromStr;

const INITIAL_SCHEMA_SQL: &str = include_str!("../migrations/0001_initial.sql");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SongInsertOutcome {
    Inserted { id: i64, duplicate_title_artist: bool },
    SkippedExactDuplicate { existing_id: i64 },
}

#[derive(Clone)]
pub struct Storage {
    pool: SqlitePool,
}

impl Storage {
    pub async fn connect_file(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create DB folder {}", parent.display()))?;
        }
        let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display()))?
            .filename(db_path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await
            .with_context(|| format!("Failed to connect SQLite at {}", db_path.display()))?;

        let storage = Self { pool };
        storage.init_schema_and_seeds().await?;
        storage.ensure_studio_presets_and_full_bible().await?;
        Ok(storage)
    }

    pub async fn connect_memory() -> Result<Self> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .context("Failed to open in-memory SQLite")?;

        let storage = Self { pool };
        storage.init_schema_and_seeds().await?;
        Ok(storage)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    async fn init_schema_and_seeds(&self) -> Result<()> {
        sqlx::raw_sql(INITIAL_SCHEMA_SQL)
            .execute(&self.pool)
            .await
            .context("Failed to execute SQLite schema migration")?;

        // Seed default themes if empty
        let theme_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM themes")
            .fetch_one(&self.pool)
            .await?;
        if theme_count == 0 {
            for preset in [
                Theme::default_main(),
                Theme::default_scripture(),
                Theme::default_return_monitor(),
            ] {
                let mut t = preset;
                t.id = 0;
                self.save_theme(&t).await?;
            }
        }

        // Seed public-domain Bibles if empty
        let bible_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bible_versions")
            .fetch_one(&self.pool)
            .await?;
        if bible_count == 0 {
            for (ver, verses) in builtin_public_domain_bibles() {
                self.import_bible_version(&ver, &verses).await?;
            }
        }

        Ok(())
    }

    /// Populates rich studio themes and the complete 66-book Public Domain TB1917 Bible
    /// for persistent desktop databases without slowing down in-memory unit tests.
    async fn ensure_studio_presets_and_full_bible(&self) -> Result<()> {
        let existing_themes = self.list_themes().await?;
        if existing_themes.len() <= 3 {
            for preset in default_studio_themes() {
                if !existing_themes
                    .iter()
                    .any(|t| t.name.eq_ignore_ascii_case(&preset.name))
                {
                    let mut t = preset;
                    t.id = 0;
                    self.save_theme(&t).await?;
                }
            }
        }

        if let Some(tb_id) =
            sqlx::query_scalar::<_, i64>("SELECT id FROM bible_versions WHERE abbr = 'TB1917' LIMIT 1")
                .fetch_optional(&self.pool)
                .await?
        {
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM bible_verses WHERE version_id = ?",
            )
            .bind(tb_id)
            .fetch_one(&self.pool)
            .await?;

            if count < 10_000 {
                tracing::info!("Seeding complete 66-book Public Domain TB1917 Bible into SQLite...");
                let all_verses = load_full_tb1917_verses(tb_id)?;
                let mut tx = self.pool.begin().await?;
                for chunk in all_verses.chunks(400) {
                    let mut qb = sqlx::QueryBuilder::new(
                        "INSERT OR REPLACE INTO bible_verses (version_id, book, book_order, chapter, verse, text, search_blob) ",
                    );
                    qb.push_values(chunk, |mut b, v| {
                        let blob = fold_search_text(&format!(
                            "{} {} {} {}",
                            v.book, v.chapter, v.verse, v.text
                        ));
                        b.push_bind(tb_id)
                            .push_bind(&v.book)
                            .push_bind(v.book_order)
                            .push_bind(v.chapter)
                            .push_bind(v.verse)
                            .push_bind(&v.text)
                            .push_bind(blob);
                    });
                    qb.build().execute(&mut *tx).await?;
                }
                tx.commit().await?;
                tracing::info!("Seeded {} verses of TB1917", all_verses.len());
            }
        }

        Ok(())
    }

    // ==================== GROUPS ====================

    pub async fn upsert_group(&self, name: &str) -> Result<Group> {
        let trimmed = name.trim();
        sqlx::query("INSERT OR IGNORE INTO groups (name) VALUES (?)")
            .bind(trimmed)
            .execute(&self.pool)
            .await?;
        let row = sqlx::query("SELECT id, name FROM groups WHERE name = ?")
            .bind(trimmed)
            .fetch_one(&self.pool)
            .await?;
        Ok(Group {
            id: row.get("id"),
            name: row.get("name"),
        })
    }

    pub async fn list_groups(&self) -> Result<Vec<Group>> {
        let rows = sqlx::query("SELECT id, name FROM groups ORDER BY name ASC")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| Group {
                id: r.get("id"),
                name: r.get("name"),
            })
            .collect())
    }

    // ==================== SONGS ====================

    pub async fn insert_song(
        &self,
        song: &Song,
        skip_exact_duplicate: bool,
    ) -> Result<SongInsertOutcome> {
        let slide_refs: Vec<&str> = song.slides.iter().map(|s| s.text.as_str()).collect();
        let norm_hash = canonical_lyrics_key(&slide_refs);

        if skip_exact_duplicate && !norm_hash.is_empty() {
            let existing: Option<i64> =
                sqlx::query_scalar("SELECT id FROM songs WHERE normalized_hash = ? LIMIT 1")
                    .bind(&norm_hash)
                    .fetch_optional(&self.pool)
                    .await?;
            if let Some(existing_id) = existing {
                return Ok(SongInsertOutcome::SkippedExactDuplicate { existing_id });
            }
        }

        let dup_title_artist: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM songs WHERE lower(title) = lower(?) AND lower(artist) = lower(?) LIMIT 1",
        )
        .bind(&song.title)
        .bind(&song.artist)
        .fetch_optional(&self.pool)
        .await?;

        let is_dup_marked = song.duplicate_marked || dup_title_artist.is_some();
        let search_blob = build_song_search_blob(song);
        let created_at = if song.created_at.is_empty() {
            chrono::Utc::now().to_rfc3339()
        } else {
            song.created_at.clone()
        };

        let mut tx = self.pool.begin().await?;
        let res = sqlx::query(
            r#"INSERT INTO songs
               (title, artist, author, group_id, notes, source, archived, duplicate_marked, normalized_hash, search_blob, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(&song.title)
        .bind(&song.artist)
        .bind(&song.author)
        .bind(song.group_id)
        .bind(&song.notes)
        .bind(&song.source)
        .bind(song.archived as i32)
        .bind(is_dup_marked as i32)
        .bind(&norm_hash)
        .bind(&search_blob)
        .bind(&created_at)
        .execute(&mut *tx)
        .await?;

        let song_id = res.last_insert_rowid();
        for (idx, slide) in song.slides.iter().enumerate() {
            sqlx::query(
                r#"INSERT INTO song_slides (song_id, "order", text, forced_break, duration_secs)
                   VALUES (?, ?, ?, ?, ?)"#,
            )
            .bind(song_id)
            .bind(idx as i32)
            .bind(&slide.text)
            .bind(slide.forced_break as i32)
            .bind(slide.duration_secs.map(|d| d as i64))
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(SongInsertOutcome::Inserted {
            id: song_id,
            duplicate_title_artist: is_dup_marked,
        })
    }

    pub async fn update_song(&self, song: &Song) -> Result<()> {
        let slide_refs: Vec<&str> = song.slides.iter().map(|s| s.text.as_str()).collect();
        let norm_hash = canonical_lyrics_key(&slide_refs);
        let search_blob = build_song_search_blob(song);

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"UPDATE songs
               SET title = ?, artist = ?, author = ?, group_id = ?, notes = ?, source = ?,
                   archived = ?, duplicate_marked = ?, normalized_hash = ?, search_blob = ?
               WHERE id = ?"#,
        )
        .bind(&song.title)
        .bind(&song.artist)
        .bind(&song.author)
        .bind(song.group_id)
        .bind(&song.notes)
        .bind(&song.source)
        .bind(song.archived as i32)
        .bind(song.duplicate_marked as i32)
        .bind(&norm_hash)
        .bind(&search_blob)
        .bind(song.id)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM song_slides WHERE song_id = ?")
            .bind(song.id)
            .execute(&mut *tx)
            .await?;

        for (idx, slide) in song.slides.iter().enumerate() {
            sqlx::query(
                r#"INSERT INTO song_slides (song_id, "order", text, forced_break, duration_secs)
                   VALUES (?, ?, ?, ?, ?)"#,
            )
            .bind(song.id)
            .bind(idx as i32)
            .bind(&slide.text)
            .bind(slide.forced_break as i32)
            .bind(slide.duration_secs.map(|d| d as i64))
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn get_song(&self, id: i64) -> Result<Option<Song>> {
        let row = sqlx::query(
            r#"SELECT id, title, artist, author, group_id, notes, source, archived, duplicate_marked, created_at
               FROM songs WHERE id = ?"#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(r) = row else {
            return Ok(None);
        };
        let slides = self.load_slides_for_song(id).await?;
        Ok(Some(Song {
            id: r.get("id"),
            title: r.get("title"),
            artist: r.get("artist"),
            author: r.get("author"),
            group_id: r.get("group_id"),
            notes: r.get("notes"),
            source: r.get("source"),
            archived: r.get::<i32, _>("archived") != 0,
            duplicate_marked: r.get::<i32, _>("duplicate_marked") != 0,
            created_at: r.get("created_at"),
            slides,
        }))
    }

    pub async fn search_songs(
        &self,
        query: &str,
        include_archived: bool,
        group_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Song>> {
        let folded = fold_search_text(query);
        let tokens: Vec<&str> = folded.split_whitespace().collect();

        let rows = if tokens.is_empty() {
            sqlx::query(
                r#"SELECT id, title, artist, author, group_id, notes, source, archived, duplicate_marked, created_at
                   FROM songs
                   WHERE (? = 1 OR archived = 0)
                     AND (? IS NULL OR group_id = ?)
                   ORDER BY title COLLATE NOCASE ASC
                   LIMIT ?"#,
            )
            .bind(include_archived as i32)
            .bind(group_id)
            .bind(group_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            let mut sql = String::from(
                r#"SELECT id, title, artist, author, group_id, notes, source, archived, duplicate_marked, created_at
                   FROM songs
                   WHERE (? = 1 OR archived = 0)
                     AND (? IS NULL OR group_id = ?)"#,
            );
            for _ in &tokens {
                sql.push_str(" AND search_blob LIKE ?");
            }
            sql.push_str(" ORDER BY title COLLATE NOCASE ASC LIMIT ?");

            let mut q = sqlx::query(&sql)
                .bind(include_archived as i32)
                .bind(group_id)
                .bind(group_id);
            for tok in &tokens {
                q = q.bind(format!("%{tok}%"));
            }
            q = q.bind(limit);
            q.fetch_all(&self.pool).await?
        };

        let mut songs = Vec::with_capacity(rows.len());
        let song_ids: Vec<i64> = rows.iter().map(|r| r.get("id")).collect();
        let mut slides_map = self.load_slides_batch(&song_ids).await?;

        for r in rows {
            let id: i64 = r.get("id");
            let slides = slides_map.remove(&id).unwrap_or_default();
            songs.push(Song {
                id,
                title: r.get("title"),
                artist: r.get("artist"),
                author: r.get("author"),
                group_id: r.get("group_id"),
                notes: r.get("notes"),
                source: r.get("source"),
                archived: r.get::<i32, _>("archived") != 0,
                duplicate_marked: r.get::<i32, _>("duplicate_marked") != 0,
                created_at: r.get("created_at"),
                slides,
            });
        }
        Ok(songs)
    }

    pub async fn set_song_archived(&self, id: i64, archived: bool) -> Result<()> {
        sqlx::query("UPDATE songs SET archived = ? WHERE id = ?")
            .bind(archived as i32)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_songs_batch(&self, ids: &[i64]) -> Result<u64> {
        let mut total = 0;
        let mut tx = self.pool.begin().await?;
        for &id in ids {
            let r = sqlx::query("DELETE FROM songs WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            total += r.rows_affected();
        }
        tx.commit().await?;
        Ok(total)
    }

    pub async fn remove_exact_duplicates(&self) -> Result<u64> {
        let res = sqlx::query(
            r#"DELETE FROM songs
               WHERE normalized_hash != ''
                 AND id NOT IN (
                     SELECT MIN(id) FROM songs GROUP BY normalized_hash
                 )"#,
        )
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected())
    }

    async fn load_slides_for_song(&self, song_id: i64) -> Result<Vec<SongSlide>> {
        let rows = sqlx::query(
            r#"SELECT id, song_id, "order", text, forced_break, duration_secs
               FROM song_slides WHERE song_id = ? ORDER BY "order" ASC"#,
        )
        .bind(song_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| SongSlide {
                id: r.get("id"),
                song_id: r.get("song_id"),
                order: r.get("order"),
                text: r.get("text"),
                forced_break: r.get::<i32, _>("forced_break") != 0,
                duration_secs: r.get::<Option<i64>, _>("duration_secs").map(|d| d as u32),
            })
            .collect())
    }

    async fn load_slides_batch(
        &self,
        song_ids: &[i64],
    ) -> Result<std::collections::HashMap<i64, Vec<SongSlide>>> {
        let mut map: std::collections::HashMap<i64, Vec<SongSlide>> =
            std::collections::HashMap::with_capacity(song_ids.len());
        if song_ids.is_empty() {
            return Ok(map);
        }

        // Chunk by 400 to stay comfortably below SQLite's bind parameter limit
        for chunk in song_ids.chunks(400) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                r#"SELECT id, song_id, "order", text, forced_break, duration_secs
                   FROM song_slides
                   WHERE song_id IN ({placeholders})
                   ORDER BY song_id ASC, "order" ASC"#
            );
            let mut q = sqlx::query(&sql);
            for &id in chunk {
                q = q.bind(id);
            }
            let rows = q.fetch_all(&self.pool).await?;
            for r in rows {
                let song_id: i64 = r.get("song_id");
                map.entry(song_id).or_default().push(SongSlide {
                    id: r.get("id"),
                    song_id,
                    order: r.get("order"),
                    text: r.get("text"),
                    forced_break: r.get::<i32, _>("forced_break") != 0,
                    duration_secs: r.get::<Option<i64>, _>("duration_secs").map(|d| d as u32),
                });
            }
        }
        Ok(map)
    }

    // ==================== THEMES ====================

    pub async fn list_themes(&self) -> Result<Vec<Theme>> {
        let rows = sqlx::query("SELECT id, name, is_return_theme, json FROM themes ORDER BY id ASC")
            .fetch_all(&self.pool)
            .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let json_str: String = r.get("json");
            let config: ThemeConfig = serde_json::from_str(&json_str).unwrap_or_default();
            out.push(Theme {
                id: r.get("id"),
                name: r.get("name"),
                is_return_theme: r.get::<i32, _>("is_return_theme") != 0,
                config,
            });
        }
        Ok(out)
    }

    pub async fn save_theme(&self, theme: &Theme) -> Result<i64> {
        let json = serde_json::to_string(&theme.config)?;
        if theme.id > 0 {
            sqlx::query(
                "UPDATE themes SET name = ?, is_return_theme = ?, json = ? WHERE id = ?",
            )
            .bind(&theme.name)
            .bind(theme.is_return_theme as i32)
            .bind(&json)
            .bind(theme.id)
            .execute(&self.pool)
            .await?;
            Ok(theme.id)
        } else {
            let res = sqlx::query(
                "INSERT INTO themes (name, is_return_theme, json) VALUES (?, ?, ?)",
            )
            .bind(&theme.name)
            .bind(theme.is_return_theme as i32)
            .bind(&json)
            .execute(&self.pool)
            .await?;
            Ok(res.last_insert_rowid())
        }
    }

    pub async fn delete_theme(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM themes WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ==================== SERVICES (CULTOS) ====================

    pub async fn create_service(&self, date: &str, name: &str) -> Result<Service> {
        let res = sqlx::query("INSERT INTO services (date, name, archived) VALUES (?, ?, 0)")
            .bind(date)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(Service {
            id: res.last_insert_rowid(),
            date: date.to_string(),
            name: name.to_string(),
            is_temporary: false,
            archived: false,
            items: Vec::new(),
        })
    }

    pub async fn get_service(&self, id: i64) -> Result<Option<Service>> {
        let row = sqlx::query("SELECT id, date, name, archived FROM services WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        let Some(r) = row else {
            return Ok(None);
        };
        let items = self.list_service_items(id).await?;
        Ok(Some(Service {
            id: r.get("id"),
            date: r.get("date"),
            name: r.get("name"),
            is_temporary: false,
            archived: r.get::<i32, _>("archived") != 0,
            items,
        }))
    }

    pub async fn list_services(&self, include_archived: bool) -> Result<Vec<Service>> {
        let rows = sqlx::query(
            r#"SELECT id, date, name, archived FROM services
               WHERE (? = 1 OR archived = 0)
               ORDER BY date DESC, id DESC"#,
        )
        .bind(include_archived as i32)
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let id: i64 = r.get("id");
            let items = self.list_service_items(id).await?;
            out.push(Service {
                id,
                date: r.get("date"),
                name: r.get("name"),
                is_temporary: false,
                archived: r.get::<i32, _>("archived") != 0,
                items,
            });
        }
        Ok(out)
    }

    pub async fn replace_service_items(
        &self,
        service_id: i64,
        items: &[ServiceItem],
    ) -> Result<Vec<ServiceItem>> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM service_items WHERE service_id = ?")
            .bind(service_id)
            .execute(&mut *tx)
            .await?;

        let mut saved = Vec::with_capacity(items.len());
        for (idx, item) in items.iter().enumerate() {
            let params_json = serde_json::to_string(&item.params)?;
            let res = sqlx::query(
                r#"INSERT INTO service_items (service_id, "order", kind, ref_id, title, subtitle, params)
                   VALUES (?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(service_id)
            .bind(idx as i32)
            .bind(item.kind.as_str())
            .bind(item.ref_id)
            .bind(&item.title)
            .bind(&item.subtitle)
            .bind(&params_json)
            .execute(&mut *tx)
            .await?;

            let mut cloned = item.clone();
            cloned.id = res.last_insert_rowid();
            cloned.service_id = service_id;
            cloned.order = idx as i32;
            saved.push(cloned);
        }

        tx.commit().await?;
        Ok(saved)
    }

    pub async fn set_service_archived(&self, id: i64, archived: bool) -> Result<()> {
        sqlx::query("UPDATE services SET archived = ? WHERE id = ?")
            .bind(archived as i32)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_service(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM services WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_service_items(&self, service_id: i64) -> Result<Vec<ServiceItem>> {
        let rows = sqlx::query(
            r#"SELECT id, service_id, "order", kind, ref_id, title, subtitle, params
               FROM service_items WHERE service_id = ? ORDER BY "order" ASC"#,
        )
        .bind(service_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let kind_str: String = r.get("kind");
                let params_str: String = r.get("params");
                let params: ServiceItemParams =
                    serde_json::from_str(&params_str).unwrap_or_default();
                ServiceItem {
                    id: r.get("id"),
                    service_id: r.get("service_id"),
                    order: r.get("order"),
                    kind: ServiceItemKind::from_str_name(&kind_str),
                    ref_id: r.get("ref_id"),
                    title: r.get("title"),
                    subtitle: r.get("subtitle"),
                    params,
                }
            })
            .collect())
    }

    // ==================== MEDIA ====================

    pub async fn upsert_media(&self, item: &MediaItem) -> Result<i64> {
        let name = if item.name.trim().is_empty() {
            Path::new(&item.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| item.path.clone())
        } else {
            item.name.clone()
        };

        sqlx::query(
            r#"INSERT INTO media (name, path, kind, duration, archived)
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT(path) DO UPDATE SET
                   name = excluded.name,
                   kind = excluded.kind,
                   duration = excluded.duration,
                   archived = excluded.archived"#,
        )
        .bind(&name)
        .bind(&item.path)
        .bind(item.kind.as_str())
        .bind(item.duration_secs)
        .bind(item.archived as i32)
        .execute(&self.pool)
        .await?;

        let id: i64 = sqlx::query_scalar("SELECT id FROM media WHERE path = ?")
            .bind(&item.path)
            .fetch_one(&self.pool)
            .await?;
        Ok(id)
    }

    pub async fn list_media(&self, include_archived: bool) -> Result<Vec<MediaItem>> {
        let rows = sqlx::query(
            r#"SELECT id, name, path, kind, duration, archived
               FROM media
               WHERE (? = 1 OR archived = 0)
               ORDER BY name COLLATE NOCASE ASC"#,
        )
        .bind(include_archived as i32)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let kind_str: String = r.get("kind");
                MediaItem {
                    id: r.get("id"),
                    name: r.get("name"),
                    path: r.get("path"),
                    kind: MediaKind::from_str_name(&kind_str),
                    duration_secs: r.get("duration"),
                    archived: r.get::<i32, _>("archived") != 0,
                }
            })
            .collect())
    }

    pub async fn set_media_archived(&self, id: i64, archived: bool) -> Result<()> {
        sqlx::query("UPDATE media SET archived = ? WHERE id = ?")
            .bind(archived as i32)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ==================== BIBLE ====================

    pub async fn list_bible_versions(&self) -> Result<Vec<BibleVersion>> {
        let rows = sqlx::query("SELECT id, abbr, name, lang, license FROM bible_versions ORDER BY id ASC")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| BibleVersion {
                id: r.get("id"),
                abbr: r.get("abbr"),
                name: r.get("name"),
                lang: r.get("lang"),
                license: r.get("license"),
            })
            .collect())
    }

    pub async fn import_bible_version(
        &self,
        version: &BibleVersion,
        verses: &[BibleVerse],
    ) -> Result<i64> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"INSERT INTO bible_versions (abbr, name, lang, license)
               VALUES (?, ?, ?, ?)
               ON CONFLICT(abbr) DO UPDATE SET
                   name = excluded.name,
                   lang = excluded.lang,
                   license = excluded.license"#,
        )
        .bind(&version.abbr)
        .bind(&version.name)
        .bind(&version.lang)
        .bind(&version.license)
        .execute(&mut *tx)
        .await?;

        let version_id: i64 =
            sqlx::query_scalar("SELECT id FROM bible_versions WHERE abbr = ?")
                .bind(&version.abbr)
                .fetch_one(&mut *tx)
                .await?;

        for v in verses {
            let blob = fold_search_text(&format!("{} {} {} {}", v.book, v.chapter, v.verse, v.text));
            sqlx::query(
                r#"INSERT OR REPLACE INTO bible_verses
                   (version_id, book, book_order, chapter, verse, text, search_blob)
                   VALUES (?, ?, ?, ?, ?, ?, ?)"#,
            )
            .bind(version_id)
            .bind(&v.book)
            .bind(v.book_order)
            .bind(v.chapter)
            .bind(v.verse)
            .bind(&v.text)
            .bind(&blob)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(version_id)
    }

    /// Lists all verses of a specific book (`1..=66`) and chapter (`1..=N`) for the Holyrics Bible Grid.
    pub async fn list_bible_chapter_verses(
        &self,
        version_id: i64,
        book_order: i32,
        chapter: i32,
    ) -> Result<Vec<BibleVerse>> {
        let rows = sqlx::query(
            r#"SELECT version_id, book, book_order, chapter, verse, text
               FROM bible_verses
               WHERE version_id = ? AND book_order = ? AND chapter = ?
               ORDER BY verse ASC"#,
        )
        .bind(version_id)
        .bind(book_order)
        .bind(chapter)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(row_to_bible_verse).collect())
    }

    /// Returns the number of chapters available for `book_order`, falling back to canonical count.
    pub async fn count_bible_chapters(&self, version_id: i64, book_order: i32) -> Result<i32> {
        let max_ch: Option<i32> = sqlx::query_scalar(
            "SELECT MAX(chapter) FROM bible_verses WHERE version_id = ? AND book_order = ?",
        )
        .bind(version_id)
        .bind(book_order)
        .fetch_optional(&self.pool)
        .await?
        .flatten();

        let canonical = canonical_book_by_order(book_order)
            .map(|b| b.chapters)
            .unwrap_or(1);

        Ok(max_ch.unwrap_or(canonical).max(canonical))
    }

    pub async fn search_bible(
        &self,
        version_id: i64,
        query: &str,
        limit: i64,
    ) -> Result<Vec<BibleVerse>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            let rows = sqlx::query(
                r#"SELECT version_id, book, book_order, chapter, verse, text
                   FROM bible_verses
                   WHERE version_id = ?
                   ORDER BY book_order ASC, chapter ASC, verse ASC
                   LIMIT ?"#,
            )
            .bind(version_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
            return Ok(rows.into_iter().map(row_to_bible_verse).collect());
        }

        if let Some(parsed) = parse_bible_reference(trimmed) {
            let rows = sqlx::query(
                r#"SELECT version_id, book, book_order, chapter, verse, text
                   FROM bible_verses
                   WHERE version_id = ?
                     AND chapter = ?
                     AND (? IS NULL OR verse >= ?)
                     AND (? IS NULL OR verse <= ?)
                   ORDER BY book_order ASC, verse ASC"#,
            )
            .bind(version_id)
            .bind(parsed.chapter)
            .bind(parsed.verse_start)
            .bind(parsed.verse_start)
            .bind(parsed.verse_end)
            .bind(parsed.verse_end)
            .fetch_all(&self.pool)
            .await?;

            let book_fold = fold_search_text(&parsed.book_query);
            let matched: Vec<BibleVerse> = rows
                .into_iter()
                .map(row_to_bible_verse)
                .filter(|v| fold_search_text(&v.book).starts_with(&book_fold))
                .take(limit as usize)
                .collect();

            if !matched.is_empty() {
                return Ok(matched);
            }
        }

        // Fallback: accent-insensitive full-text search
        let folded = fold_search_text(trimmed);
        let tokens: Vec<&str> = folded.split_whitespace().collect();
        let mut sql = String::from(
            r#"SELECT version_id, book, book_order, chapter, verse, text
               FROM bible_verses WHERE version_id = ?"#,
        );
        for _ in &tokens {
            sql.push_str(" AND search_blob LIKE ?");
        }
        sql.push_str(" ORDER BY book_order ASC, chapter ASC, verse ASC LIMIT ?");

        let mut q = sqlx::query(&sql).bind(version_id);
        for tok in &tokens {
            q = q.bind(format!("%{tok}%"));
        }
        q = q.bind(limit);
        let rows = q.fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(row_to_bible_verse).collect())
    }

    pub async fn add_bible_favorite(&self, verse: &BibleVerse) -> Result<i64> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            r#"INSERT OR REPLACE INTO bible_favorites
               (version_id, book, chapter, verse, text, created_at)
               VALUES (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(verse.version_id)
        .bind(&verse.book)
        .bind(verse.chapter)
        .bind(verse.verse)
        .bind(&verse.text)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        let id: i64 = sqlx::query_scalar(
            "SELECT id FROM bible_favorites WHERE version_id = ? AND book = ? AND chapter = ? AND verse = ?",
        )
        .bind(verse.version_id)
        .bind(&verse.book)
        .bind(verse.chapter)
        .bind(verse.verse)
        .fetch_one(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn remove_bible_favorite(&self, id: i64) -> Result<()> {
        sqlx::query("DELETE FROM bible_favorites WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_bible_favorites(&self) -> Result<Vec<BibleFavorite>> {
        let rows = sqlx::query(
            r#"SELECT id, version_id, book, chapter, verse, text, created_at
               FROM bible_favorites ORDER BY id DESC"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| BibleFavorite {
                id: r.get("id"),
                version_id: r.get("version_id"),
                book: r.get("book"),
                chapter: r.get("chapter"),
                verse: r.get("verse"),
                text: r.get("text"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    pub async fn record_bible_history(&self, verse: &BibleVerse) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            r#"INSERT INTO bible_history (version_id, book, chapter, verse, text, accessed_at)
               VALUES (?, ?, ?, ?, ?, ?)"#,
        )
        .bind(verse.version_id)
        .bind(&verse.book)
        .bind(verse.chapter)
        .bind(verse.verse)
        .bind(&verse.text)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_bible_history(&self, limit: i64) -> Result<Vec<BibleVerse>> {
        let rows = sqlx::query(
            r#"SELECT version_id, book, 1 AS book_order, chapter, verse, text
               FROM bible_history ORDER BY id DESC LIMIT ?"#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_bible_verse).collect())
    }

    // ==================== TEAM SCHEDULES & REPORTS ====================

    pub async fn upsert_team_member(&self, name: &str) -> Result<TeamMember> {
        let trimmed = name.trim();
        sqlx::query(
            "INSERT INTO team_members (name, archived) VALUES (?, 0) ON CONFLICT(name) DO UPDATE SET archived = 0",
        )
        .bind(trimmed)
        .execute(&self.pool)
        .await?;

        let r = sqlx::query("SELECT id, name, archived FROM team_members WHERE name = ?")
            .bind(trimmed)
            .fetch_one(&self.pool)
            .await?;
        Ok(TeamMember {
            id: r.get("id"),
            name: r.get("name"),
            archived: r.get::<i32, _>("archived") != 0,
        })
    }

    pub async fn list_team_members(&self, include_archived: bool) -> Result<Vec<TeamMember>> {
        let rows = sqlx::query(
            "SELECT id, name, archived FROM team_members WHERE (? = 1 OR archived = 0) ORDER BY name COLLATE NOCASE ASC",
        )
        .bind(include_archived as i32)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| TeamMember {
                id: r.get("id"),
                name: r.get("name"),
                archived: r.get::<i32, _>("archived") != 0,
            })
            .collect())
    }

    pub async fn add_schedule_entry(
        &self,
        service_id: i64,
        member_id: i64,
        role: &str,
    ) -> Result<i64> {
        let res = sqlx::query(
            "INSERT OR IGNORE INTO schedules (service_id, member_id, role) VALUES (?, ?, ?)",
        )
        .bind(service_id)
        .bind(member_id)
        .bind(role.trim())
        .execute(&self.pool)
        .await?;
        Ok(res.last_insert_rowid())
    }

    pub async fn remove_schedule_entry(&self, schedule_id: i64) -> Result<()> {
        sqlx::query("DELETE FROM schedules WHERE id = ?")
            .bind(schedule_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn list_service_schedule(&self, service_id: i64) -> Result<Vec<ScheduleEntry>> {
        let rows = sqlx::query(
            r#"SELECT sc.id, sc.service_id, sc.member_id, tm.name AS member_name, sc.role
               FROM schedules sc
               JOIN team_members tm ON tm.id = sc.member_id
               WHERE sc.service_id = ?
               ORDER BY sc.role ASC, tm.name ASC"#,
        )
        .bind(service_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ScheduleEntry {
                id: r.get("id"),
                service_id: r.get("service_id"),
                member_id: r.get("member_id"),
                member_name: r.get("member_name"),
                role: r.get("role"),
            })
            .collect())
    }

    pub async fn report_member_schedule_counts(&self) -> Result<Vec<MemberScheduleCount>> {
        let rows = sqlx::query(
            r#"SELECT tm.id AS member_id, tm.name AS member_name, COUNT(sc.id) AS times_scheduled
               FROM team_members tm
               LEFT JOIN schedules sc ON sc.member_id = tm.id
               WHERE tm.archived = 0
               GROUP BY tm.id, tm.name
               ORDER BY times_scheduled DESC, tm.name COLLATE NOCASE ASC"#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| MemberScheduleCount {
                member_id: r.get("member_id"),
                member_name: r.get("member_name"),
                times_scheduled: r.get("times_scheduled"),
            })
            .collect())
    }

    pub async fn record_execution_at(
        &self,
        service_item_id: Option<i64>,
        song_id: Option<i64>,
        executed_at_iso: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO execution_log (service_item_id, song_id, executed_at) VALUES (?, ?, ?)",
        )
        .bind(service_item_id)
        .bind(song_id)
        .bind(executed_at_iso)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn record_execution(
        &self,
        service_item_id: Option<i64>,
        song_id: Option<i64>,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.record_execution_at(service_item_id, song_id, &now).await
    }

    /// Returns most played songs filtered optionally by ISO date period and/or day of week (`0 = Sunday .. 6 = Saturday`).
    pub async fn report_most_played_songs(
        &self,
        from_iso: Option<&str>,
        to_iso: Option<&str>,
        weekday: Option<u32>,
        limit: i64,
    ) -> Result<Vec<SongPlayStat>> {
        let rows = sqlx::query(
            r#"SELECT
                   s.id AS song_id,
                   s.title AS title,
                   s.artist AS artist,
                   COUNT(e.id) AS play_count,
                   CAST(strftime('%w', e.executed_at) AS INTEGER) AS wk
               FROM execution_log e
               JOIN songs s ON s.id = COALESCE(
                   e.song_id,
                   (SELECT si.ref_id FROM service_items si WHERE si.id = e.service_item_id AND si.kind = 'song')
               )
               WHERE (? IS NULL OR e.executed_at >= ?)
                 AND (? IS NULL OR e.executed_at <= ?)
                 AND (? IS NULL OR CAST(strftime('%w', e.executed_at) AS INTEGER) = ?)
               GROUP BY s.id, s.title, s.artist
               ORDER BY play_count DESC, s.title COLLATE NOCASE ASC
               LIMIT ?"#,
        )
        .bind(from_iso)
        .bind(from_iso)
        .bind(to_iso)
        .bind(to_iso)
        .bind(weekday.map(|w| w as i64))
        .bind(weekday.map(|w| w as i64))
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| SongPlayStat {
                song_id: r.get("song_id"),
                title: r.get("title"),
                artist: r.get("artist"),
                play_count: r.get("play_count"),
                weekday: weekday.or_else(|| r.get::<Option<i64>, _>("wk").map(|w| w as u32)),
            })
            .collect())
    }

    // ==================== SETTINGS ====================

    pub async fn load_settings(&self) -> Result<AppSettings> {
        let json: Option<String> =
            sqlx::query_scalar("SELECT value FROM app_settings WHERE key = 'app_settings'")
                .fetch_optional(&self.pool)
                .await?;
        match json {
            Some(s) => Ok(serde_json::from_str(&s).unwrap_or_default()),
            None => Ok(AppSettings::default()),
        }
    }

    pub async fn save_settings(&self, settings: &AppSettings) -> Result<()> {
        let json = serde_json::to_string(settings)?;
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES ('app_settings', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(&json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn build_song_search_blob(song: &Song) -> String {
    let mut raw = format!("{} {} {} ", song.title, song.artist, song.author);
    for s in &song.slides {
        raw.push_str(&s.text);
        raw.push(' ');
    }
    fold_search_text(&raw)
}

fn row_to_bible_verse(r: sqlx::sqlite::SqliteRow) -> BibleVerse {
    BibleVerse {
        version_id: r.get("version_id"),
        book: r.get("book"),
        book_order: r.get("book_order"),
        chapter: r.get("chapter"),
        verse: r.get("verse"),
        text: r.get("text"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn imports_500_songs_and_searches_under_100ms() -> Result<()> {
        let store = Storage::connect_memory().await?;

        for i in 0..500 {
            let mut song = Song::new_draft(
                format!("Hino de Louvor {i:03}"),
                format!("Cantor Congregacional {}", i % 25),
                "public_domain",
            );
            song.slides = vec![
                SongSlide {
                    id: 0,
                    song_id: 0,
                    order: 0,
                    text: format!("Cantai alegremente a Deus, fortaleza nossa {i}"),
                    forced_break: false,
                    duration_secs: None,
                },
                SongSlide {
                    id: 0,
                    song_id: 0,
                    order: 1,
                    text: "Celebrai com júbilo ao Deus de Jacó".to_string(),
                    forced_break: false,
                    duration_secs: None,
                },
            ];
            let outcome = store.insert_song(&song, true).await?;
            assert!(matches!(outcome, SongInsertOutcome::Inserted { .. }));
        }

        // Exact duplicate must be skipped when skip_exact_duplicate = true
        let mut dup = Song::new_draft("Outro Título", "Outro Artista", "public_domain");
        dup.slides = vec![
            SongSlide {
                id: 0,
                song_id: 0,
                order: 0,
                text: "Cantai alegremente a Deus, fortaleza nossa 42".to_string(),
                forced_break: false,
                duration_secs: None,
            },
            SongSlide {
                id: 0,
                song_id: 0,
                order: 1,
                text: "Celebrai com júbilo ao Deus de Jacó".to_string(),
                forced_break: false,
                duration_secs: None,
            },
        ];
        let dup_outcome = store.insert_song(&dup, true).await?;
        assert!(matches!(
            dup_outcome,
            SongInsertOutcome::SkippedExactDuplicate { .. }
        ));

        // Accent-insensitive search benchmark (< 100 ms acceptance criterion)
        let t0 = Instant::now();
        let results = store
            .search_songs("jubilo jaco 042", false, None, 50)
            .await?;
        let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Hino de Louvor 042");
        assert!(
            elapsed_ms < 100.0,
            "search took {elapsed_ms:.2} ms (must be < 100 ms)"
        );

        // Archive & restore
        let id = results[0].id;
        store.set_song_archived(id, true).await?;
        assert!(
            store
                .search_songs("042", false, None, 10)
                .await?
                .is_empty()
        );
        assert_eq!(store.search_songs("042", true, None, 10).await?.len(), 1);
        store.set_song_archived(id, false).await?;
        assert_eq!(store.search_songs("042", false, None, 10).await?.len(), 1);

        Ok(())
    }

    #[tokio::test]
    async fn bible_search_schedules_reports_and_backup_roundtrip() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let db_path = temp.path().join("db").join("overture.db");
        let media_dir = temp.path().join("media");
        std::fs::create_dir_all(&media_dir)?;
        std::fs::write(media_dir.join("fundo.png"), b"PNGDATA123")?;

        let store = Storage::connect_file(&db_path).await?;

        // Seeded Bible reference & text search (complete 66-book TB1917: "Pois assim amou Deus ao mundo...")
        let jo316 = store.search_bible(1, "João 3:16", 10).await?;
        assert_eq!(jo316.len(), 1);
        assert!(jo316[0].text.contains("amou") && jo316[0].text.contains("mundo"));

        let gen1 = store.list_bible_chapter_verses(1, 1, 1).await?;
        assert_eq!(gen1.len(), 31);
        let rev22 = store.list_bible_chapter_verses(1, 66, 22).await?;
        assert_eq!(rev22.len(), 21);

        let sl23 = store.search_bible(1, "Sl 23:1-3", 10).await?;
        assert_eq!(sl23.len(), 3);

        // Favorites & history
        let fav_id = store.add_bible_favorite(&jo316[0]).await?;
        assert_eq!(store.list_bible_favorites().await?.len(), 1);
        store.record_bible_history(&jo316[0]).await?;
        assert_eq!(store.list_bible_history(5).await?.len(), 1);
        store.remove_bible_favorite(fav_id).await?;
        assert!(store.list_bible_favorites().await?.is_empty());

        // Service, Schedule & Execution Report by weekday
        let mut song = Song::new_draft("Castelo Forte", "Martinho Lutero", "public_domain");
        song.slides.push(SongSlide {
            id: 0,
            song_id: 0,
            order: 0,
            text: "Castelo forte é nosso Deus".to_string(),
            forced_break: false,
            duration_secs: None,
        });
        let SongInsertOutcome::Inserted { id: song_id, .. } = store.insert_song(&song, true).await?
        else {
            panic!("expected insert");
        };

        let svc = store.create_service("2026-11-01", "Culto de Domingo").await?;
        let m1 = store.upsert_team_member("Ana Paula").await?;
        store.add_schedule_entry(svc.id, m1.id, "Vocal").await?;
        let counts = store.report_member_schedule_counts().await?;
        assert_eq!(counts[0].member_name, "Ana Paula");
        assert_eq!(counts[0].times_scheduled, 1);

        // 2026-11-01 is a Sunday (weekday = 0)
        store
            .record_execution_at(None, Some(song_id), "2026-11-01T19:30:00Z")
            .await?;
        store
            .record_execution_at(None, Some(song_id), "2026-11-01T20:00:00Z")
            .await?;
        let sun_stats = store
            .report_most_played_songs(Some("2026-11-01"), Some("2026-11-02"), Some(0), 10)
            .await?;
        assert_eq!(sun_stats.len(), 1);
        assert_eq!(sun_stats[0].play_count, 2);

        // Full backup & clean restore
        let backup_zip = temp.path().join("backup.overture-backup.zip");
        let manifest = create_full_backup(store.pool(), &media_dir, &backup_zip).await?;
        assert_eq!(manifest.media_file_count, 1);
        store.close().await;

        let restored_db = temp.path().join("restored").join("overture.db");
        let restored_media = temp.path().join("restored_media");
        restore_full_backup(&backup_zip, &restored_db, &restored_media)?;
        assert_eq!(
            std::fs::read(restored_media.join("fundo.png"))?,
            b"PNGDATA123"
        );

        let restored_store = Storage::connect_file(&restored_db).await?;
        let songs_after = restored_store.search_songs("Castelo", false, None, 10).await?;
        assert_eq!(songs_after.len(), 1);
        restored_store.close().await;

        Ok(())
    }
}

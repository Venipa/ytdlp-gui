//! Typed repo functions on `Db`. Timestamps are unix milliseconds.
use anyhow::{Context, Result};
use domain::{
    Clock, DownloadId, DownloadStatus, DownloadSummary, ErrorCode, JobSpec, SystemClock, ToolKind,
    ToolSource, domain_of,
};
use sqlx::FromRow;

use crate::Db;

/// Max log lines kept per download.
pub const LOG_CAP: i64 = 500;

fn now() -> i64 {
    SystemClock.now_ms()
}

/// Columns of `DownloadSummary`; deliberately excludes `options_json` (and `info_json` lives in another table).
macro_rules! summary_cols {
    () => {
        "id, url, domain, extractor, video_id, title, uploader, duration_s, thumbnail_url, thumbnail_path, \
    kind, preset, format_label, status, status_reason, phase, progress, bytes_done, bytes_total, file_path, file_size, \
    size_before_fix, file_missing, error_code, error_message, attempts, next_attempt_at, playlist_id, playlist_index, position, \
    created_at, started_at, finished_at, updated_at"
    };
}
#[cfg(test)]
const SUMMARY_COLS: &str = summary_cols!();

#[derive(FromRow)]
struct SummaryRow {
    id: i64,
    url: String,
    domain: String,
    extractor: Option<String>,
    video_id: Option<String>,
    title: Option<String>,
    uploader: Option<String>,
    duration_s: Option<f64>,
    thumbnail_url: Option<String>,
    thumbnail_path: Option<String>,
    kind: String,
    preset: String,
    format_label: Option<String>,
    status: String,
    status_reason: Option<String>,
    phase: Option<String>,
    progress: f64,
    bytes_done: Option<i64>,
    bytes_total: Option<i64>,
    file_path: Option<String>,
    file_size: Option<i64>,
    size_before_fix: Option<i64>,
    file_missing: i64,
    error_code: Option<String>,
    error_message: Option<String>,
    attempts: i64,
    next_attempt_at: Option<i64>,
    playlist_id: Option<i64>,
    playlist_index: Option<i64>,
    position: f64,
    created_at: i64,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    updated_at: i64,
}

impl SummaryRow {
    fn into_summary(self) -> Result<DownloadSummary> {
        let mut s = DownloadSummary {
            id: DownloadId(self.id),
            url: self.url,
            domain: self.domain,
            extractor: self.extractor,
            video_id: self.video_id,
            title: self.title,
            uploader: self.uploader,
            title_lc: String::new(),
            uploader_lc: String::new(),
            duration_s: self.duration_s,
            thumbnail_url: self.thumbnail_url,
            thumbnail_path: self.thumbnail_path,
            kind: self.kind.parse().map_err(anyhow::Error::msg)?,
            preset: self.preset,
            format_label: self.format_label,
            status: self.status.parse().map_err(anyhow::Error::msg)?,
            status_reason: self.status_reason,
            phase: self.phase,
            progress: self.progress,
            bytes_done: self.bytes_done,
            bytes_total: self.bytes_total,
            file_path: self.file_path,
            file_size: self.file_size,
            size_before_fix: self.size_before_fix,
            file_missing: self.file_missing != 0,
            error_code: self.error_code,
            error_message: self.error_message,
            attempts: self.attempts as u32,
            next_attempt_at: self.next_attempt_at,
            playlist_id: self.playlist_id,
            playlist_index: self.playlist_index,
            position: self.position,
            created_at: self.created_at,
            started_at: self.started_at,
            finished_at: self.finished_at,
            updated_at: self.updated_at,
        };
        s.refresh_lc();
        Ok(s)
    }
}

/// A download to insert (status starts as `queued`).
#[derive(Debug, Clone)]
pub struct NewDownload {
    pub url: String,
    /// Derived from `url` when empty.
    pub domain: String,
    pub webpage_url: Option<String>,
    pub extractor: Option<String>,
    pub video_id: Option<String>,
    pub title: Option<String>,
    pub uploader: Option<String>,
    pub duration_s: Option<f64>,
    pub thumbnail_url: Option<String>,
    pub kind: domain::MediaKind,
    pub preset: String,
    pub format_spec: String,
    pub format_label: Option<String>,
    pub options_json: String,
    pub output_dir: Option<String>,
    pub playlist_id: Option<i64>,
    pub playlist_index: Option<i64>,
    /// Trimmed info dict stored in `download_meta` (re-opens Choose formats).
    pub info_json: Option<String>,
}

impl NewDownload {
    /// Builds the insert from a job spec (kind, preset, format, `options_json` snapshot, domain).
    pub fn from_spec(spec: &JobSpec) -> Result<Self> {
        Ok(Self {
            url: spec.url.clone(),
            domain: domain_of(&spec.url),
            webpage_url: None,
            extractor: None,
            video_id: None,
            title: None,
            uploader: None,
            duration_s: None,
            thumbnail_url: None,
            kind: spec.preset.kind(),
            preset: spec.preset.to_string(),
            format_spec: spec.format_spec.clone(),
            format_label: None,
            options_json: serde_json::to_string(spec).context("cannot serialise job spec")?,
            output_dir: Some(spec.output_dir.clone()),
            playlist_id: None,
            playlist_index: spec.playlist_index.map(i64::from),
            info_json: None,
        })
    }
}

/// A row with explicit state for `Db::seed_downloads`.
#[derive(Debug, Clone)]
pub struct SeedRow {
    pub new: NewDownload,
    pub status: DownloadStatus,
    pub reason: Option<String>,
    pub progress: f64,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub error: Option<(ErrorCode, String)>,
    pub attempts: u32,
    pub next_attempt_at: Option<i64>,
    pub created_at: i64,
}

/// Full row for the detail panel / retry.
#[derive(Debug, Clone)]
pub struct DownloadDetail {
    pub summary: DownloadSummary,
    pub format_spec: String,
    pub options_json: String,
    pub output_dir: Option<String>,
    pub webpage_url: Option<String>,
    pub info_json: Option<String>,
}

/// One log line.
#[derive(Debug, Clone, PartialEq, FromRow)]
pub struct LogLine {
    pub id: i64,
    pub at: i64,
    pub level: String,
    pub message: String,
}

/// A playlist header row.
#[derive(Debug, Clone, PartialEq, Default, FromRow)]
pub struct Playlist {
    pub id: i64,
    pub url: String,
    pub extractor: Option<String>,
    pub remote_id: Option<String>,
    pub title: Option<String>,
    pub uploader: Option<String>,
    pub entry_count: Option<i64>,
    pub created_at: i64,
}

/// A cached tool row.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRow {
    pub name: ToolKind,
    pub source: ToolSource,
    pub path: String,
    pub version: Option<String>,
    pub sha256: Option<String>,
    pub installed_at: Option<i64>,
    pub checked_at: Option<i64>,
}

#[derive(FromRow)]
struct ToolRaw {
    name: String,
    source: String,
    path: String,
    version: Option<String>,
    sha256: Option<String>,
    installed_at: Option<i64>,
    checked_at: Option<i64>,
}

impl Db {
    // ---- downloads ----

    /// Inserts a queued download (and its meta) at the end of the queue.
    pub async fn insert_download(&self, d: &NewDownload) -> Result<DownloadId> {
        let t = now();
        let domain = if d.domain.is_empty() {
            domain_of(&d.url)
        } else {
            d.domain.clone()
        };
        let mut tx = self.pool.begin().await?;
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO downloads (url, webpage_url, extractor, video_id, domain, title, uploader, duration_s, thumbnail_url, \
             kind, preset, format_spec, format_label, options_json, status, output_dir, playlist_id, playlist_index, position, \
             created_at, updated_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'queued',?15,?16,?17, \
             (SELECT COALESCE(MAX(position), 0) + 1 FROM downloads), ?18, ?18) RETURNING id",
        )
        .bind(&d.url)
        .bind(&d.webpage_url)
        .bind(&d.extractor)
        .bind(&d.video_id)
        .bind(domain)
        .bind(&d.title)
        .bind(&d.uploader)
        .bind(d.duration_s)
        .bind(&d.thumbnail_url)
        .bind(d.kind.as_str())
        .bind(&d.preset)
        .bind(&d.format_spec)
        .bind(&d.format_label)
        .bind(&d.options_json)
        .bind(&d.output_dir)
        .bind(d.playlist_id)
        .bind(d.playlist_index)
        .bind(t)
        .fetch_one(&mut *tx)
        .await
        .context("cannot insert download")?;
        if let Some(info) = &d.info_json {
            sqlx::query("INSERT INTO download_meta (download_id, info_json) VALUES (?1, ?2)")
                .bind(id)
                .bind(info)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(DownloadId(id))
    }

    /// Sets status (+ reason); stamps `started_at` on first probing and `finished_at` on terminal states.
    pub async fn set_status(
        &self,
        id: DownloadId,
        status: DownloadStatus,
        reason: Option<&str>,
    ) -> Result<()> {
        let t = now();
        sqlx::query(
            "UPDATE downloads SET status = ?2, status_reason = ?3, updated_at = ?4, \
             started_at = CASE WHEN ?2 = 'probing' THEN COALESCE(started_at, ?4) ELSE started_at END, \
             finished_at = CASE WHEN ?2 IN ('completed','failed','cancelled') THEN ?4 ELSE NULL END \
             WHERE id = ?1",
        )
        .bind(id.0)
        .bind(status.as_str())
        .bind(reason)
        .bind(t)
        .execute(&self.pool)
        .await
        .context("cannot update status")?;
        Ok(())
    }

    /// Sets the phase label (`merging`, `postprocess:<name>`), or clears it.
    pub async fn set_phase(&self, id: DownloadId, phase: Option<&str>) -> Result<()> {
        sqlx::query("UPDATE downloads SET phase = ?2, updated_at = ?3 WHERE id = ?1")
            .bind(id.0)
            .bind(phase)
            .bind(now())
            .execute(&self.pool)
            .await
            .context("cannot update phase")?;
        Ok(())
    }

    /// Persists progress (callers throttle to a phase change or every 5 s).
    pub async fn set_progress(
        &self,
        id: DownloadId,
        progress: f64,
        bytes_done: Option<i64>,
        bytes_total: Option<i64>,
    ) -> Result<()> {
        sqlx::query("UPDATE downloads SET progress = ?2, bytes_done = ?3, bytes_total = ?4, updated_at = ?5 WHERE id = ?1")
            .bind(id.0)
            .bind(progress)
            .bind(bytes_done)
            .bind(bytes_total)
            .bind(now())
            .execute(&self.pool)
            .await
            .context("cannot update progress")?;
        Ok(())
    }

    /// Records the final file.
    /// Records the file a fix made: its path and size, and the size before the first fix.
    pub async fn set_fixed_file(
        &self,
        id: DownloadId,
        path: &str,
        size: Option<i64>,
        size_before_fix: Option<i64>,
    ) -> Result<()> {
        sqlx::query("UPDATE downloads SET file_path = ?2, file_size = ?3, size_before_fix = ?4, file_missing = 0, updated_at = ?5 WHERE id = ?1")
            .bind(id.0)
            .bind(path)
            .bind(size)
            .bind(size_before_fix)
            .bind(now())
            .execute(&self.pool)
            .await
            .context("cannot store the fixed file")?;
        Ok(())
    }

    /// Replaces the format words of `id` with what the finished file holds ("1080p · HEVC · AAC · MP4").
    pub async fn set_format_label(&self, id: DownloadId, label: &str) -> Result<()> {
        sqlx::query("UPDATE downloads SET format_label = ?2, updated_at = ?3 WHERE id = ?1")
            .bind(id.0)
            .bind(label)
            .bind(now())
            .execute(&self.pool)
            .await
            .context("cannot store the format")?;
        Ok(())
    }

    pub async fn set_final_path(
        &self,
        id: DownloadId,
        path: &str,
        size: Option<i64>,
    ) -> Result<()> {
        sqlx::query("UPDATE downloads SET file_path = ?2, file_size = ?3, file_missing = 0, updated_at = ?4 WHERE id = ?1")
            .bind(id.0)
            .bind(path)
            .bind(size)
            .bind(now())
            .execute(&self.pool)
            .await
            .context("cannot set final path")?;
        Ok(())
    }

    /// Flags a completed row whose file vanished (or clears the flag).
    pub async fn set_file_missing(&self, id: DownloadId, missing: bool) -> Result<()> {
        sqlx::query("UPDATE downloads SET file_missing = ?2 WHERE id = ?1")
            .bind(id.0)
            .bind(missing)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Stores or clears (`None`) the error.
    pub async fn set_error(&self, id: DownloadId, err: Option<(ErrorCode, &str)>) -> Result<()> {
        sqlx::query("UPDATE downloads SET error_code = ?2, error_message = ?3, updated_at = ?4 WHERE id = ?1")
            .bind(id.0)
            .bind(err.map(|e| e.0.as_str()))
            .bind(err.map(|e| e.1))
            .bind(now())
            .execute(&self.pool)
            .await
            .context("cannot set error")?;
        Ok(())
    }

    /// Stores the attempt count and backoff gate (survives restart).
    pub async fn set_retry(
        &self,
        id: DownloadId,
        attempts: u32,
        next_attempt_at: Option<i64>,
    ) -> Result<()> {
        sqlx::query("UPDATE downloads SET attempts = ?2, next_attempt_at = ?3, updated_at = ?4 WHERE id = ?1")
            .bind(id.0)
            .bind(attempts as i64)
            .bind(next_attempt_at)
            .bind(now())
            .execute(&self.pool)
            .await
            .context("cannot set retry")?;
        Ok(())
    }

    /// Updates probe-derived metadata after the fact.
    pub async fn set_meta(
        &self,
        id: DownloadId,
        title: Option<&str>,
        uploader: Option<&str>,
        thumbnail_path: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE downloads SET title = COALESCE(?2, title), uploader = COALESCE(?3, uploader), \
             thumbnail_path = COALESCE(?4, thumbnail_path), updated_at = ?5 WHERE id = ?1",
        )
        .bind(id.0)
        .bind(title)
        .bind(uploader)
        .bind(thumbnail_path)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// All rows for the list, newest first, without any blob columns.
    pub async fn list_summaries(&self) -> Result<Vec<DownloadSummary>> {
        let rows: Vec<SummaryRow> = sqlx::query_as(concat!(
            "SELECT ",
            summary_cols!(),
            " FROM downloads ORDER BY created_at DESC, id DESC"
        ))
        .fetch_all(&self.pool)
        .await
        .context("cannot list downloads")?;
        rows.into_iter().map(SummaryRow::into_summary).collect()
    }

    /// One row with spec and info (detail panel, retry, re-open Choose formats).
    pub async fn get_detail(&self, id: DownloadId) -> Result<Option<DownloadDetail>> {
        let Some(row): Option<SummaryRow> = sqlx::query_as(concat!(
            "SELECT ",
            summary_cols!(),
            " FROM downloads WHERE id = ?1"
        ))
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        let (format_spec, options_json, output_dir, webpage_url): (String, String, Option<String>, Option<String>) =
            sqlx::query_as("SELECT format_spec, options_json, output_dir, webpage_url FROM downloads WHERE id = ?1")
                .bind(id.0)
                .fetch_one(&self.pool)
                .await?;
        let info_json =
            sqlx::query_scalar("SELECT info_json FROM download_meta WHERE download_id = ?1")
                .bind(id.0)
                .fetch_optional(&self.pool)
                .await?;
        Ok(Some(DownloadDetail {
            summary: row.into_summary()?,
            format_spec,
            options_json,
            output_dir,
            webpage_url,
            info_json,
        }))
    }

    /// Deletes the row (meta and logs cascade). An active row must be cancelled first (caller's rule).
    pub async fn delete_download(&self, id: DownloadId) -> Result<bool> {
        let r = sqlx::query("DELETE FROM downloads WHERE id = ?1")
            .bind(id.0)
            .execute(&self.pool)
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Startup recovery: probing/downloading/processing become `paused` with reason `interrupted`.
    pub async fn recover_interrupted(&self) -> Result<u64> {
        let r = sqlx::query(
            "UPDATE downloads SET status = 'paused', status_reason = 'interrupted', updated_at = ?1 \
             WHERE status IN ('probing','downloading','processing')",
        )
        .bind(now())
        .execute(&self.pool)
        .await
        .context("cannot recover interrupted downloads")?;
        Ok(r.rows_affected())
    }

    /// Bulk-inserts finished-looking rows in one transaction (demo scenes, tests, `seed` example).
    pub async fn seed_downloads(&self, rows: &[SeedRow]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for (i, r) in rows.iter().enumerate() {
            let d = &r.new;
            let domain = if d.domain.is_empty() {
                domain_of(&d.url)
            } else {
                d.domain.clone()
            };
            let id: i64 = sqlx::query_scalar(
                "INSERT INTO downloads (url, extractor, video_id, domain, title, uploader, duration_s, kind, preset,                  format_spec, format_label, options_json, status, status_reason, progress, bytes_total, file_path, file_size,                  error_code, error_message, attempts, next_attempt_at, position, created_at, finished_at, updated_at, thumbnail_url)                  VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,                  CASE WHEN ?13 IN ('completed','failed','cancelled') THEN ?24 END, ?24, ?25) RETURNING id",
            )
            .bind(&d.url)
            .bind(&d.extractor)
            .bind(&d.video_id)
            .bind(domain)
            .bind(&d.title)
            .bind(&d.uploader)
            .bind(d.duration_s)
            .bind(d.kind.as_str())
            .bind(&d.preset)
            .bind(&d.format_spec)
            .bind(&d.format_label)
            .bind(&d.options_json)
            .bind(r.status.as_str())
            .bind(&r.reason)
            .bind(r.progress)
            .bind(r.file_size)
            .bind(&r.file_path)
            .bind(r.file_size)
            .bind(r.error.as_ref().map(|e| e.0.as_str()))
            .bind(r.error.as_ref().map(|e| e.1.as_str()))
            .bind(r.attempts as i64)
            .bind(r.next_attempt_at)
            .bind(i as f64 + 1.0)
            .bind(r.created_at)
            .bind(&d.thumbnail_url)
            .fetch_one(&mut *tx)
            .await
            .context("cannot seed download")?;
            if let Some(info) = &d.info_json {
                sqlx::query("INSERT INTO download_meta (download_id, info_json) VALUES (?1, ?2)")
                    .bind(id)
                    .bind(info)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    // ---- logs ----

    /// Appends a log line and trims the job to the last `LOG_CAP` lines.
    pub async fn append_log(&self, id: DownloadId, level: &str, message: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO download_logs (download_id, at, level, message) VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(id.0)
        .bind(now())
        .bind(level)
        .bind(message)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM download_logs WHERE download_id = ?1 AND id <= \
             (SELECT id FROM download_logs WHERE download_id = ?1 ORDER BY id DESC LIMIT 1 OFFSET ?2)",
        )
        .bind(id.0)
        .bind(LOG_CAP)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Log lines of one job, oldest first.
    pub async fn list_logs(&self, id: DownloadId) -> Result<Vec<LogLine>> {
        Ok(sqlx::query_as(
            "SELECT id, at, level, message FROM download_logs WHERE download_id = ?1 ORDER BY id",
        )
        .bind(id.0)
        .fetch_all(&self.pool)
        .await?)
    }

    // ---- app_state ----

    /// Reads a key.
    pub async fn state_get(&self, key: &str) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT value FROM app_state WHERE key = ?1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Writes a key (upsert).
    pub async fn state_set(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query("INSERT INTO app_state (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(key)
            .bind(value)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ---- tools ----

    /// Inserts or replaces the cached row for a tool.
    pub async fn upsert_tool(&self, t: &ToolRow) -> Result<()> {
        sqlx::query(
            "INSERT INTO tools (name, source, path, version, sha256, installed_at, checked_at) VALUES (?1,?2,?3,?4,?5,?6,?7) \
             ON CONFLICT(name) DO UPDATE SET source=excluded.source, path=excluded.path, version=excluded.version, \
             sha256=excluded.sha256, installed_at=excluded.installed_at, checked_at=excluded.checked_at",
        )
        .bind(t.name.as_str())
        .bind(t.source.as_str())
        .bind(&t.path)
        .bind(&t.version)
        .bind(&t.sha256)
        .bind(t.installed_at)
        .bind(t.checked_at)
        .execute(&self.pool)
        .await
        .context("cannot save tool")?;
        Ok(())
    }

    /// All cached tool rows (startup cache; re-probed by `tools`).
    pub async fn list_tools(&self) -> Result<Vec<ToolRow>> {
        let raw: Vec<ToolRaw> =
            sqlx::query_as("SELECT name, source, path, version, sha256, installed_at, checked_at FROM tools ORDER BY name")
                .fetch_all(&self.pool)
                .await?;
        raw.into_iter()
            .map(|r| {
                Ok(ToolRow {
                    name: r.name.parse().map_err(anyhow::Error::msg)?,
                    source: r.source.parse().map_err(anyhow::Error::msg)?,
                    path: r.path,
                    version: r.version,
                    sha256: r.sha256,
                    installed_at: r.installed_at,
                    checked_at: r.checked_at,
                })
            })
            .collect()
    }

    // ---- playlists ----

    /// Inserts a playlist header; `created_at` is set here.
    pub async fn insert_playlist(&self, p: &Playlist) -> Result<i64> {
        Ok(sqlx::query_scalar(
            "INSERT INTO playlists (url, extractor, remote_id, title, uploader, entry_count, created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7) RETURNING id",
        )
        .bind(&p.url)
        .bind(&p.extractor)
        .bind(&p.remote_id)
        .bind(&p.title)
        .bind(&p.uploader)
        .bind(p.entry_count)
        .bind(now())
        .fetch_one(&self.pool)
        .await?)
    }

    /// Reads a playlist header.
    pub async fn get_playlist(&self, id: i64) -> Result<Option<Playlist>> {
        Ok(sqlx::query_as("SELECT id, url, extractor, remote_id, title, uploader, entry_count, created_at FROM playlists WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Preset, Settings};

    fn spec(url: &str) -> JobSpec {
        let s = Settings::default();
        JobSpec {
            url: url.into(),
            preset: Preset::DEFAULT,
            format_spec: "bv*+ba/b".into(),
            output_dir: s.output_dir,
            outtmpl: s.outtmpl,
            playlist_index: None,
            embed_thumbnail: false,
            embed_metadata: true,
            embed_chapters: true,
            subtitles: Default::default(),
            sponsorblock: Default::default(),
            cookies: Default::default(),
            proxy: None,
            rate_limit: None,
            retries: None,
            ffmpeg_location: None,
            js_runtime: None,
            extra_args: vec!["--a b".into()],
            aria2c: None,
            aria2c_args: vec![],
            fix: Default::default(),
            smaller_quality: None,
            smaller_height: None,
        }
    }

    async fn add(db: &Db, url: &str) -> DownloadId {
        let mut n = NewDownload::from_spec(&spec(url)).unwrap();
        n.title = Some("Hello World".into());
        n.info_json = Some(r#"{"formats":[]}"#.into());
        db.insert_download(&n).await.unwrap()
    }

    #[tokio::test]
    async fn insert_list_detail_delete() {
        let db = Db::memory().await.unwrap();
        let a = add(&db, "https://www.example.com/a").await;
        let b = add(&db, "https://other.org/b").await;
        let list = db.list_summaries().await.unwrap();
        assert_eq!(list.iter().map(|s| s.id).collect::<Vec<_>>(), [b, a]); // newest first
        assert_eq!(list[1].domain, "example.com");
        assert_eq!(list[1].title_lc, "hello world");
        assert!(list[0].position > list[1].position);
        assert_eq!(list[0].status, DownloadStatus::Queued);

        let d = db.get_detail(a).await.unwrap().unwrap();
        assert_eq!(d.info_json.as_deref(), Some(r#"{"formats":[]}"#));
        let back: JobSpec = serde_json::from_str(&d.options_json).unwrap();
        assert_eq!(back, spec("https://www.example.com/a")); // retry reproduces the spec incl. extra args
        assert!(db.get_detail(DownloadId(999)).await.unwrap().is_none());

        db.append_log(a, "info", "x").await.unwrap();
        assert!(db.delete_download(a).await.unwrap());
        assert!(!db.delete_download(a).await.unwrap());
        assert_eq!(db.list_logs(a).await.unwrap().len(), 0); // cascade
        let meta: i64 = sqlx::query_scalar("SELECT count(*) FROM download_meta")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(meta, 1);
    }

    #[tokio::test]
    async fn summaries_no_blobs() {
        assert!(
            !SUMMARY_COLS.contains("options_json")
                && !SUMMARY_COLS.contains("info_json")
                && !SUMMARY_COLS.contains("format_spec")
        );
    }

    #[tokio::test]
    async fn updates() {
        let db = Db::memory().await.unwrap();
        let id = add(&db, "https://a.com/1").await;
        let get = || async { db.list_summaries().await.unwrap().remove(0) };

        db.set_status(id, DownloadStatus::Probing, None)
            .await
            .unwrap();
        let s = get().await;
        assert!(
            s.status == DownloadStatus::Probing
                && s.started_at.is_some()
                && s.finished_at.is_none()
        );
        db.set_phase(id, Some("merging")).await.unwrap();
        db.set_progress(id, 0.5, Some(50), Some(100)).await.unwrap();
        let s = get().await;
        assert_eq!(
            (s.phase.as_deref(), s.progress, s.bytes_done, s.bytes_total),
            (Some("merging"), 0.5, Some(50), Some(100))
        );

        db.set_retry(id, 2, Some(123)).await.unwrap();
        db.set_error(id, Some((ErrorCode::Network, "boom")))
            .await
            .unwrap();
        let s = get().await;
        assert_eq!(
            (
                s.attempts,
                s.next_attempt_at,
                s.error_code.as_deref(),
                s.error_message.as_deref()
            ),
            (2, Some(123), Some("network"), Some("boom"))
        );
        db.set_error(id, None).await.unwrap();
        assert!(get().await.error_code.is_none());

        db.set_final_path(id, "C:/v/a.mp4", Some(42)).await.unwrap();
        db.set_status(id, DownloadStatus::Completed, None)
            .await
            .unwrap();
        db.set_file_missing(id, true).await.unwrap();
        db.set_meta(id, Some("New"), None, Some("t.jpg"))
            .await
            .unwrap();
        let s = get().await;
        assert!(s.status == DownloadStatus::Completed && s.finished_at.is_some() && s.file_missing);
        assert_eq!(
            (
                s.file_path.as_deref(),
                s.file_size,
                s.thumbnail_path.as_deref()
            ),
            (Some("C:/v/a.mp4"), Some(42), Some("t.jpg"))
        );
        assert_eq!(s.title.as_deref(), Some("New"));
        assert_eq!(s.uploader, None);

        db.set_fixed_file(id, "C:/v/a.small.mp4", Some(21), Some(42))
            .await
            .unwrap();
        db.set_format_label(id, "1080p · HEVC · AAC · MP4")
            .await
            .unwrap();
        let s = get().await;
        assert_eq!(
            (
                s.file_path.as_deref(),
                s.file_size,
                s.size_before_fix,
                s.format_label.as_deref()
            ),
            (
                Some("C:/v/a.small.mp4"),
                Some(21),
                Some(42),
                Some("1080p · HEVC · AAC · MP4")
            )
        );
    }

    #[tokio::test]
    async fn logs_capped_per_job() {
        let db = Db::memory().await.unwrap();
        let (a, b) = (
            add(&db, "https://a.com/1").await,
            add(&db, "https://a.com/2").await,
        );
        for i in 0..(LOG_CAP + 20) {
            db.append_log(a, "info", &format!("line {i}"))
                .await
                .unwrap();
        }
        db.append_log(b, "warn", "other").await.unwrap();
        let logs = db.list_logs(a).await.unwrap();
        assert_eq!(logs.len() as i64, LOG_CAP);
        assert_eq!(logs[0].message, "line 20");
        assert_eq!(
            logs.last().unwrap().message,
            format!("line {}", LOG_CAP + 19)
        );
        assert_eq!(db.list_logs(b).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn recovery_pauses_active_only() {
        let db = Db::memory().await.unwrap();
        let mut ids = vec![];
        for (i, st) in DownloadStatus::ALL.iter().enumerate() {
            let id = add(&db, &format!("https://a.com/{i}")).await;
            db.set_status(id, *st, None).await.unwrap();
            ids.push((id, *st));
        }
        assert_eq!(db.recover_interrupted().await.unwrap(), 3);
        for s in db.list_summaries().await.unwrap() {
            let was = ids.iter().find(|(i, _)| *i == s.id).unwrap().1;
            if was.is_active() {
                assert_eq!(
                    (s.status, s.status_reason.as_deref()),
                    (DownloadStatus::Paused, Some("interrupted"))
                );
            } else {
                assert_eq!(s.status, was);
            }
        }
        assert_eq!(db.recover_interrupted().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn state_tools_playlists() {
        let db = Db::memory().await.unwrap();
        assert_eq!(db.state_get("k").await.unwrap(), None);
        db.state_set("k", "1").await.unwrap();
        db.state_set("k", "2").await.unwrap();
        assert_eq!(db.state_get("k").await.unwrap().as_deref(), Some("2"));

        let mut t = ToolRow {
            name: ToolKind::Ffmpeg,
            source: ToolSource::Managed,
            path: "C:/t/ffmpeg.exe".into(),
            version: Some("n8.1".into()),
            sha256: None,
            installed_at: Some(1),
            checked_at: Some(2),
        };
        db.upsert_tool(&t).await.unwrap();
        t.source = ToolSource::Custom;
        t.version = Some("n9".into());
        db.upsert_tool(&t).await.unwrap();
        assert_eq!(db.list_tools().await.unwrap(), vec![t]);

        let pid = db
            .insert_playlist(&Playlist {
                url: "https://a.com/pl".into(),
                title: Some("PL".into()),
                entry_count: Some(3),
                ..Default::default()
            })
            .await
            .unwrap();
        let mut n = NewDownload::from_spec(&spec("https://a.com/pl?v=1")).unwrap();
        n.playlist_id = Some(pid);
        n.playlist_index = Some(1);
        let id = db.insert_download(&n).await.unwrap();
        let p = db.get_playlist(pid).await.unwrap().unwrap();
        assert_eq!((p.title.as_deref(), p.entry_count), (Some("PL"), Some(3)));
        assert_eq!(db.list_summaries().await.unwrap()[0].playlist_id, Some(pid));
        sqlx::query("DELETE FROM playlists WHERE id = ?1")
            .bind(pid)
            .execute(db.pool())
            .await
            .unwrap();
        assert_eq!(
            db.get_detail(id)
                .await
                .unwrap()
                .unwrap()
                .summary
                .playlist_id,
            None
        ); // ON DELETE SET NULL
    }

    #[tokio::test]
    async fn schema_checks_and_tables() {
        let db = Db::memory().await.unwrap();
        let tables: Vec<String> =
            sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE '\\_sqlx%' ESCAPE '\\' AND name NOT LIKE 'sqlite_%' ORDER BY name")
                .fetch_all(db.pool())
                .await
                .unwrap();
        assert_eq!(
            tables,
            [
                "app_state",
                "download_logs",
                "download_meta",
                "downloads",
                "playlists",
                "tools"
            ]
        );
        let base = "INSERT INTO downloads (url, domain, kind, preset, format_spec, options_json, status, position, created_at, updated_at) VALUES ('u','d',?1,'p','f','{}',?2,1,0,0)";
        assert!(
            sqlx::query(base)
                .bind("video")
                .bind("queued")
                .execute(db.pool())
                .await
                .is_ok()
        );
        assert!(
            sqlx::query(base)
                .bind("image")
                .bind("queued")
                .execute(db.pool())
                .await
                .is_err()
        );
        assert!(
            sqlx::query(base)
                .bind("video")
                .bind("bogus")
                .execute(db.pool())
                .await
                .is_err()
        );
        assert!(
            sqlx::query("INSERT INTO tools (name, source, path) VALUES ('x','nope','p')")
                .execute(db.pool())
                .await
                .is_err()
        );
        assert!(
            sqlx::query(
                "INSERT INTO download_logs (download_id, at, level, message) VALUES (99,0,'i','m')"
            )
            .execute(db.pool())
            .await
            .is_err()
        ); // FK on
    }

    #[tokio::test]
    async fn file_db_pragmas_backup_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ytdlpd.db");
        let db = Db::open(&path, "1.0.0").await.unwrap();
        let pragma = |p: &'static str| {
            let (db, p) = (db.clone(), p);
            async move {
                sqlx::query_scalar::<_, i64>(p)
                    .fetch_one(db.pool())
                    .await
                    .unwrap()
            }
        };
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(mode, "wal");
        assert_eq!(
            (
                pragma("PRAGMA foreign_keys").await,
                pragma("PRAGMA busy_timeout").await,
                pragma("PRAGMA synchronous").await
            ),
            (1, 5000, 1)
        );
        add(&db, "https://a.com/1").await;
        assert!(!dir.path().join("ytdlpd.db.bak-1.0.0").exists()); // fresh file: nothing to back up
        db.close().await;

        // up to date: reopen keeps data and makes no backup
        let db = Db::open(&path, "1.0.1").await.unwrap();
        assert_eq!(db.list_summaries().await.unwrap().len(), 1);
        assert!(!dir.path().join("ytdlpd.db.bak-1.0.1").exists());
        // simulate a pending migration: forget it was applied
        sqlx::query("DELETE FROM _sqlx_migrations")
            .execute(db.pool())
            .await
            .unwrap();
        db.close().await;
        // the backup happens before migrating (re-running 0001 over existing tables then fails, which is fine here)
        assert!(Db::open(&path, "1.0.2").await.is_err());
        assert!(dir.path().join("ytdlpd.db.bak-1.0.2").exists());
    }
}

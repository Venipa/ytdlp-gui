//! SQLite persistence for ytdlpd: pool, embedded migrations, typed repo fns, data paths.
//! Queries are runtime-checked (`query`/`query_as`), no `DATABASE_URL`.

mod paths;
mod repo;

pub use paths::{Paths, app_dir_name};
pub use repo::*;

use anyhow::{Context, Result};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

/// Embedded migrations (`apps/desktop/migrations`).
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

/// Handle to the database; cheap to clone.
#[derive(Clone)]
pub struct Db {
    pub(crate) pool: SqlitePool,
}

fn options(opts: SqliteConnectOptions) -> SqliteConnectOptions {
    opts.journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_millis(5000))
}

impl Db {
    /// Opens (creating if needed) the database file, backs it up once per `app_version` when migrations
    /// are pending, then migrates. Pool of 4, WAL, foreign keys on, busy timeout 5 s, synchronous NORMAL.
    pub async fn open(path: &Path, app_version: &str) -> Result<Db> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
        }
        let existed = path.exists();
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options(
                SqliteConnectOptions::new()
                    .filename(path)
                    .create_if_missing(true),
            ))
            .await
            .context("cannot open database")?;
        let db = Db { pool };
        if existed && db.pending_migrations().await? > 0 {
            db.backup(path, app_version).await?;
        }
        MIGRATOR
            .run(&db.pool)
            .await
            .context("cannot migrate database")?;
        Ok(db)
    }

    /// Single-connection in-memory database (each memory connection is its own database), migrated.
    pub async fn memory() -> Result<Db> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options(SqliteConnectOptions::from_str("sqlite::memory:")?))
            .await
            .context("cannot open in-memory database")?;
        MIGRATOR
            .run(&pool)
            .await
            .context("cannot migrate database")?;
        Ok(Db { pool })
    }

    /// The underlying pool (tests, bulk tools).
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Migrations in the binary that are not applied yet (all of them for a fresh file).
    async fn pending_migrations(&self) -> Result<usize> {
        let has: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_master WHERE name = '_sqlx_migrations'",
        )
        .fetch_one(&self.pool)
        .await?;
        let applied: i64 = if has > 0 {
            sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
                .fetch_one(&self.pool)
                .await?
        } else {
            0
        };
        Ok(MIGRATOR.iter().count().saturating_sub(applied as usize))
    }

    /// Copies the database to `<name>.bak-<app_version>` once per version bump.
    async fn backup(&self, path: &Path, app_version: &str) -> Result<()> {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".bak-{app_version}"));
        let dest: PathBuf = path.with_file_name(name);
        if dest.exists() {
            return Ok(());
        }
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await?;
        std::fs::copy(path, &dest)
            .with_context(|| format!("cannot back up database to {}", dest.display()))?;
        Ok(())
    }

    /// Checkpoints the WAL and closes the pool (shutdown).
    pub async fn close(&self) {
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await
            .ok();
        self.pool.close().await;
    }
}

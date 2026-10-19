//! SQLite connection management and schema migrations.
//!
//! One connection guarded by a mutex is plenty for a single-user desktop app and
//! keeps every multi-statement operation trivially atomic. All calls run on the
//! blocking thread pool so the async API never stalls on disk I/O.

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

/// Ordered schema migrations. `PRAGMA user_version` records how many have run, so
/// new versions of the app can append entries and upgrade existing libraries
/// in place without ever dropping user data.
const MIGRATIONS: &[&str] = &[
    // 1 — initial schema (port of the original PostgreSQL / SQLAlchemy models)
    r#"
    CREATE TABLE projects (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        name        TEXT NOT NULL,
        description TEXT NOT NULL DEFAULT '',
        created_at  TEXT NOT NULL
    );

    CREATE TABLE categories (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        name        TEXT NOT NULL,
        description TEXT NOT NULL DEFAULT '',
        created_at  TEXT NOT NULL
    );
    CREATE INDEX idx_categories_project ON categories(project_id);

    CREATE TABLE papers (
        id                INTEGER PRIMARY KEY AUTOINCREMENT,
        project_id        INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        category_id       INTEGER NULL REFERENCES categories(id) ON DELETE SET NULL,
        title             TEXT NOT NULL,
        original_filename TEXT NOT NULL,
        storage_path      TEXT NOT NULL,
        citation_number   INTEGER NOT NULL,
        file_size         INTEGER NOT NULL DEFAULT 0,
        uploaded_at       TEXT NOT NULL
    );
    CREATE INDEX idx_papers_project ON papers(project_id);
    CREATE INDEX idx_papers_category ON papers(category_id);
    CREATE UNIQUE INDEX idx_papers_citation ON papers(project_id, citation_number);

    CREATE TABLE annotations (
        id              INTEGER PRIMARY KEY AUTOINCREMENT,
        paper_id        INTEGER NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
        page_number     INTEGER NOT NULL,
        annotation_type TEXT NOT NULL CHECK (annotation_type IN ('sentence', 'rectangle')),
        text            TEXT NOT NULL DEFAULT '',
        color           TEXT NOT NULL DEFAULT '#ffe66d',
        rects           TEXT NOT NULL DEFAULT '[]',
        created_at      TEXT NOT NULL
    );
    CREATE INDEX idx_annotations_paper ON annotations(paper_id);

    CREATE TABLE important_images (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        project_id   INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        paper_id     INTEGER NULL REFERENCES papers(id) ON DELETE SET NULL,
        title        TEXT NOT NULL,
        page_number  INTEGER NULL,
        storage_path TEXT NOT NULL,
        created_at   TEXT NOT NULL
    );
    CREATE INDEX idx_images_project ON important_images(project_id);
    CREATE INDEX idx_images_paper ON important_images(paper_id);

    CREATE TABLE latex_documents (
        id               INTEGER PRIMARY KEY AUTOINCREMENT,
        project_id       INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
        title            TEXT NOT NULL,
        storage_dir      TEXT NOT NULL,
        created_at       TEXT NOT NULL,
        updated_at       TEXT NOT NULL,
        last_compile_log TEXT NOT NULL DEFAULT '',
        compiled         INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_latex_project ON latex_documents(project_id);
    "#,
];

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Open (creating if needed) the library database and bring it up to date.
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::configure(&conn, true)?;
        Self::migrate(&conn)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    /// In-memory database (tests).
    pub fn open_in_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure(&conn, false)?;
        Self::migrate(&conn)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    fn configure(conn: &Connection, file_backed: bool) -> AppResult<()> {
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        if file_backed {
            conn.pragma_update(None, "journal_mode", "WAL")?;
            conn.pragma_update(None, "synchronous", "NORMAL")?;
        }
        Ok(())
    }

    fn migrate(conn: &Connection) -> AppResult<()> {
        let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        for (index, sql) in MIGRATIONS.iter().enumerate() {
            let version = index as i64 + 1;
            if version <= current {
                continue;
            }
            tracing::info!(version, "applying database migration");
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", version)?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Run a closure with exclusive access to the connection on the blocking pool.
    pub async fn call<T, F>(&self, f: F) -> AppResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> AppResult<T> + Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = conn
                .lock()
                .map_err(|_| AppError::internal("Database lock was poisoned"))?;
            f(&mut guard)
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_cleanly_and_are_idempotent() {
        let db = Db::open_in_memory().unwrap();
        let guard = db.conn.lock().unwrap();
        let version: i64 = guard.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        // Running again must be a no-op.
        Db::migrate(&guard).unwrap();
    }
}

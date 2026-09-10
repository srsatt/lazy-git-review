use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{AppError, Result};
use crate::model::SessionId;
use crate::settings::Settings;

const SCHEMA_VERSION: i64 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionRecord {
    pub id: SessionId,
    pub repository: PathBuf,
    pub revision: i64,
    pub state: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct Store {
    connection: Connection,
    path: PathBuf,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut connection = Connection::open(&path)?;
        connection.busy_timeout(Duration::from_secs(3))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        migrate(&mut connection, &path)?;
        Ok(Self { connection, path })
    }

    pub fn default_path() -> Result<PathBuf> {
        let settings = Settings::load(&Settings::default_path()?)?;
        Ok(settings.data_dir.join("sessions.sqlite3"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn root_dir(&self) -> &Path {
        self.path.parent().unwrap_or_else(|| Path::new("."))
    }

    pub fn create_session(&mut self, repository: impl AsRef<Path>) -> Result<SessionRecord> {
        let repository = fs::canonicalize(repository)?;
        let now = Utc::now();
        let record = SessionRecord {
            id: SessionId::new(),
            repository,
            revision: 0,
            state: serde_json::json!({}),
            created_at: now,
            updated_at: now,
        };
        self.connection.execute(
            "INSERT INTO sessions(id, repository, revision, state_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![record.id.as_str(), record.repository.to_string_lossy(), record.revision, record.state.to_string(), record.created_at, record.updated_at],
        )?;
        Ok(record)
    }

    pub fn get_session(&self, id: &SessionId) -> Result<SessionRecord> {
        let row = self.connection.query_row(
            "SELECT id, repository, revision, state_json, created_at, updated_at FROM sessions WHERE id = ?1",
            [id.as_str()],
            row_to_session,
        ).optional()?;
        let mut record = row.ok_or_else(|| AppError::SessionNotFound(id.to_string()))?;
        if let Some(snapshot_id) = record.state.get("snapshot_id").and_then(Value::as_str) {
            let local_snapshot = self
                .root_dir()
                .join("snapshots")
                .join(snapshot_id)
                .join("snapshot.json");
            if local_snapshot.is_file() {
                record.state["snapshot_path"] =
                    Value::String(local_snapshot.to_string_lossy().into_owned());
            }
        }
        Ok(record)
    }

    pub fn sessions_for_repository(
        &self,
        repository: impl AsRef<Path>,
    ) -> Result<Vec<SessionRecord>> {
        let repository = fs::canonicalize(repository)?;
        let mut statement = self.connection.prepare(
            "SELECT id, repository, revision, state_json, created_at, updated_at
             FROM sessions WHERE repository = ?1 ORDER BY updated_at DESC",
        )?;
        let rows = statement.query_map([repository.to_string_lossy().as_ref()], row_to_session)?;
        let mut records = Vec::new();
        for row in rows {
            let record = row?;
            records.push(self.get_session(&record.id)?);
        }
        Ok(records)
    }

    pub fn update_state(
        &mut self,
        id: &SessionId,
        expected_revision: i64,
        state: &Value,
    ) -> Result<SessionRecord> {
        let tx = self.connection.transaction()?;
        let actual: Option<i64> = tx
            .query_row(
                "SELECT revision FROM sessions WHERE id = ?1",
                [id.as_str()],
                |row| row.get(0),
            )
            .optional()?;
        let actual = actual.ok_or_else(|| AppError::SessionNotFound(id.to_string()))?;
        if actual != expected_revision {
            return Err(AppError::RevisionConflict {
                expected: expected_revision,
                actual,
            });
        }
        let now = Utc::now();
        tx.execute(
            "UPDATE sessions SET revision = revision + 1, state_json = ?1, updated_at = ?2 WHERE id = ?3 AND revision = ?4",
            params![state.to_string(), now, id.as_str(), expected_revision],
        )?;
        tx.commit()?;
        self.get_session(id)
    }
}

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRecord> {
    let raw_id: String = row.get(0)?;
    let raw_state: String = row.get(3)?;
    Ok(SessionRecord {
        id: SessionId::parse(raw_id).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
        })?,
        repository: PathBuf::from(row.get::<_, String>(1)?),
        revision: row.get(2)?,
        state: serde_json::from_str(&raw_state).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, e.into())
        })?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn migrate(connection: &mut Connection, path: &Path) -> Result<()> {
    let current: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current > SCHEMA_VERSION {
        return Err(AppError::UnsupportedSchema {
            path: path.to_path_buf(),
            version: current,
        });
    }
    if current == SCHEMA_VERSION {
        return Ok(());
    }
    if current > 0 && path.exists() {
        let backup = path.with_extension(format!(
            "sqlite3.backup-v{current}-{}",
            Utc::now().timestamp()
        ));
        fs::copy(path, backup)?;
    }
    let tx = connection.transaction()?;
    migrate_transaction(&tx, current, SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(())
}

fn migrate_transaction(tx: &Transaction<'_>, from: i64, to: i64) -> Result<()> {
    if from < 1 && to >= 1 {
        tx.execute_batch(
            "CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                repository TEXT NOT NULL,
                revision INTEGER NOT NULL,
                state_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX sessions_repository ON sessions(repository);
            PRAGMA user_version = 1;",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn reopens_and_isolates_repositories() {
        let dir = TempDir::new().unwrap();
        let repo_a = dir.path().join("a");
        let repo_b = dir.path().join("b");
        fs::create_dir_all(&repo_a).unwrap();
        fs::create_dir_all(&repo_b).unwrap();
        let db = dir.path().join("sessions.sqlite3");
        let (a, b) = {
            let mut store = Store::open(&db).unwrap();
            (
                store.create_session(&repo_a).unwrap(),
                store.create_session(&repo_b).unwrap(),
            )
        };
        let store = Store::open(&db).unwrap();
        assert_eq!(
            store.get_session(&a.id).unwrap().repository,
            repo_a.canonicalize().unwrap()
        );
        assert_eq!(
            store.get_session(&b.id).unwrap().repository,
            repo_b.canonicalize().unwrap()
        );
        assert_eq!(store.sessions_for_repository(&repo_a).unwrap().len(), 1);
        assert_eq!(store.sessions_for_repository(&repo_b).unwrap().len(), 1);
    }

    #[test]
    fn stale_update_is_atomic() {
        let dir = TempDir::new().unwrap();
        let mut store = Store::open(dir.path().join("sessions.sqlite3")).unwrap();
        let session = store.create_session(dir.path()).unwrap();
        let updated = store
            .update_state(&session.id, 0, &serde_json::json!({"selected":"h1"}))
            .unwrap();
        assert_eq!(updated.revision, 1);
        let error = store
            .update_state(&session.id, 0, &serde_json::json!({"selected":"h2"}))
            .unwrap_err();
        assert!(matches!(
            error,
            AppError::RevisionConflict {
                expected: 0,
                actual: 1
            }
        ));
        assert_eq!(
            store.get_session(&session.id).unwrap().state,
            serde_json::json!({"selected":"h1"})
        );
    }

    #[test]
    fn unsupported_migration_rolls_back_data() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("sessions.sqlite3");
        let mut store = Store::open(&path).unwrap();
        let session = store.create_session(dir.path()).unwrap();
        store
            .connection
            .pragma_update(None, "user_version", 999)
            .unwrap();
        drop(store);
        let error = Store::open(&path).err().unwrap();
        assert!(matches!(
            error,
            AppError::UnsupportedSchema { version: 999, .. }
        ));
        let connection = Connection::open(path).unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sessions WHERE id = ?1",
                [session.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}

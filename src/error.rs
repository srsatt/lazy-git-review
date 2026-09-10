use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{message}")]
    InvalidInput { code: &'static str, message: String },
    #[error("session {0} was not found")]
    SessionNotFound(String),
    #[error("session revision conflict: expected {expected}, current {actual}")]
    RevisionConflict { expected: i64, actual: i64 },
    #[error("database at {path} uses unsupported schema version {version}")]
    UnsupportedSchema { path: PathBuf, version: i64 },
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("git command failed: {command}: {message}")]
    Git { command: String, message: String },
    #[error("language server error: {0}")]
    Lsp(String),
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput { code, .. } => code,
            Self::SessionNotFound(_) => "session_not_found",
            Self::RevisionConflict { .. } => "revision_conflict",
            Self::UnsupportedSchema { .. } => "unsupported_schema",
            Self::Database(_) => "database_error",
            Self::Io(_) => "io_error",
            Self::Json(_) => "serialization_error",
            Self::Git { .. } => "git_error",
            Self::Lsp(_) => "lsp_error",
        }
    }

    pub fn exit_code(&self) -> u8 {
        match self {
            Self::InvalidInput { .. } => 2,
            Self::SessionNotFound(_) => 4,
            Self::RevisionConflict { .. } => 5,
            _ => 1,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: String,
}

impl From<&AppError> for ErrorBody {
    fn from(value: &AppError) -> Self {
        Self {
            code: value.code(),
            message: value.to_string(),
        }
    }
}

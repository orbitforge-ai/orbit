use thiserror::Error;

#[derive(Debug, Error)]
pub enum OrbitError {
    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("Connection pool error: {0}")]
    Pool(#[from] r2d2::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Task join error: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("{0}")]
    Other(String),
}

impl OrbitError {
    pub fn other(msg: impl Into<String>) -> Self {
        OrbitError::Other(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, OrbitError>;

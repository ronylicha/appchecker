use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Git error: {0}")]
    Git(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Registry error: {0}")]
    Registry(String),

    #[error("Notification error: {0}")]
    Notification(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("{0}")]
    Other(String),
}

impl From<gix::open::Error> for AppError {
    fn from(e: gix::open::Error) -> Self {
        AppError::Git(e.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        AppError::Registry(e.to_string())
    }
}

impl From<notify_rust::error::Error> for AppError {
    fn from(e: notify_rust::error::Error) -> Self {
        AppError::Notification(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, HybridSearchError>;

#[derive(Debug, Error)]
pub enum HybridSearchError {
    #[error("missing configuration: {0}")]
    MissingConfig(String),
    #[error("invalid parameters: {0}")]
    InvalidParams(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("response parse error: {0}")]
    Parse(String),
    #[error("request timed out: {0}")]
    Timeout(String),
    #[error("not found: {0}")]
    NotFound(String),
}

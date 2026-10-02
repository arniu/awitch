#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unknown protocol: {name}")]
    UnknownProtocol { name: String },
    #[error("extended thinking is not supported")]
    ThinkingUnsupported,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

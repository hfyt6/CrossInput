use thiserror::Error;

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("Encryption error: {0}")]
    Encryption(String),
    
    #[error("Authentication failed")]
    AuthenticationFailed,
    
    #[error("Connection timeout")]
    ConnectionTimeout,
    
    #[error("Invalid message format")]
    InvalidMessageFormat,
    
    #[error("Unknown error: {0}")]
    Unknown(String),
}
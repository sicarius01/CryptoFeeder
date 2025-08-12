use thiserror::Error;

#[derive(Error, Debug)]
pub enum CryptoFeederError {
    #[error("WebSocket 연결 오류: {0}")]
    WebSocketError(#[from] tokio_tungstenite::tungstenite::Error),
    
    #[error("JSON 파싱 오류: {0}")]
    JsonParseError(String),
    
    #[error("UDP 전송 오류: {0}")]
    UdpError(#[from] std::io::Error),
    
    #[error("설정 오류: {0}")]
    ConfigError(#[from] config::ConfigError),
    
    #[error("URL 파싱 오류: {0}")]
    UrlParseError(#[from] url::ParseError),
    
    #[error("직렬화 오류: {0}")]
    SerializationError(String),
    
    #[error("일반 오류: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CryptoFeederError>;
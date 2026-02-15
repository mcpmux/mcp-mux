//! Server logging types and management

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Server log entry (stored as JSON Lines)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerLog {
    /// Timestamp (ISO 8601)
    #[serde(rename = "ts")]
    pub timestamp: DateTime<Utc>,

    /// Log level
    #[serde(rename = "lvl")]
    pub level: LogLevel,

    /// Log source
    #[serde(rename = "src")]
    pub source: LogSource,

    /// Message
    #[serde(rename = "msg")]
    pub message: String,

    /// Optional metadata (JSON object)
    #[serde(rename = "meta", skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl ServerLog {
    /// Create a new log entry
    pub fn new(level: LogLevel, source: LogSource, message: impl Into<String>) -> Self {
        Self {
            timestamp: Utc::now(),
            level,
            source,
            message: message.into(),
            metadata: None,
        }
    }

    /// Add metadata to the log entry
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

/// Log level
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "trace" => Some(Self::Trace),
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warn" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

/// Log source
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LogSource {
    /// McpMux application logs
    App,
    /// STDIO stdout
    Stdout,
    /// STDIO stderr
    Stderr,
    /// HTTP request
    HttpRequest,
    /// HTTP response
    HttpResponse,
    /// SSE events
    SseEvent,
    /// Connection events
    Connection,
    /// OAuth flow
    OAuth,
    /// MCP protocol logging notifications (notifications/message from server)
    Server,
}

impl LogSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::HttpRequest => "http-request",
            Self::HttpResponse => "http-response",
            Self::SseEvent => "sse-event",
            Self::Connection => "connection",
            Self::OAuth => "oauth",
            Self::Server => "server",
        }
    }
}

/// Configuration for log rotation
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Base directory for logs
    pub base_dir: PathBuf,

    /// Maximum file size before rotation (bytes)
    pub max_file_size: u64,

    /// Maximum number of rotated files to keep
    pub max_files: usize,

    /// Whether to compress rotated files
    pub compress: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            base_dir: PathBuf::from("logs"),
            max_file_size: 10 * 1024 * 1024, // 10MB
            max_files: 30,                   // 30 files
            compress: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_serialization() {
        let log = ServerLog::new(LogLevel::Info, LogSource::App, "Test message")
            .with_metadata(serde_json::json!({"key": "value"}));

        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("\"lvl\":\"info\""));
        assert!(json.contains("\"src\":\"app\""));
        assert!(json.contains("\"msg\":\"Test message\""));

        let deserialized: ServerLog = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.level, LogLevel::Info);
        assert_eq!(deserialized.source, LogSource::App);
        assert_eq!(deserialized.message, "Test message");
    }

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Trace < LogLevel::Debug);
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Error);
    }
}

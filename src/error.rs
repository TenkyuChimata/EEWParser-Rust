use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("input is empty")]
    EmptyInput,

    #[error("invalid UTF-8 input: {0}")]
    InvalidUtf8(#[from] std::str::Utf8Error),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("input format conflict: expected {expected}, but input looks like {actual}")]
    InputFormatConflict {
        expected: &'static str,
        actual: &'static str,
    },

    #[error("invalid command line: {0}")]
    InvalidArguments(String),

    #[error("invalid or incomplete Telegram: {0}")]
    InvalidTelegram(String),

    #[error("invalid XML: {0}")]
    InvalidXml(String),

    #[error("unsupported EEW XML: {0}")]
    UnsupportedXml(String),

    #[error(
        "unsupported EEW XML (event_id={event_id:?}, xml_type={xml_type:?}, info_type={info_type:?}, field={field:?}): {reason}"
    )]
    Unsupported {
        event_id: Option<String>,
        xml_type: Option<String>,
        info_type: Option<String>,
        field: Option<String>,
        reason: String,
    },

    #[error("required XML field is missing or invalid: {0}")]
    MissingXmlField(String),

    #[error("failed to serialize JSON: {0}")]
    Json(#[from] serde_json::Error),
}

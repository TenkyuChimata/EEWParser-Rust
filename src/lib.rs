mod error;
pub mod input;
mod parser;
mod state;
mod telegram_encoder;
mod xml;

use serde::{Deserialize, Serialize};

pub use error::AppError as EewError;
pub use parser::{Ebi, Telegram as DecodedEew};
pub use state::{EventDecision, EventState, EventTracker};

use crate::input::{InputFormat, detect, telegram_for_json, telegram_through_terminator};
use crate::parser::decoder;
use crate::xml::{InfoType, XmlProduct};

// Keeping the report directly in the public enum avoids an allocation on every
// normal real-time message and preserves the ergonomic Report(DecodedEew) API.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EewOutcome {
    Report(DecodedEew),
    Cancellation(CancellationEvent),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum CancellationKind {
    Forecast,
    Warning,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CancellationEvent {
    #[serde(rename = "Type")]
    pub event_type: String,
    #[serde(rename = "EventID")]
    pub event_id: String,
    #[serde(rename = "CancellationKind")]
    pub kind: CancellationKind,
    #[serde(rename = "ReportDateTime")]
    pub report_datetime: Option<String>,
    #[serde(rename = "TargetDateTime")]
    pub target_datetime: Option<String>,
    #[serde(rename = "Title")]
    pub title: Option<String>,
    #[serde(rename = "InfoKind")]
    pub info_kind: Option<String>,
    #[serde(rename = "Telegram")]
    pub telegram: Option<String>,
}

/// Decodes a JMA code Telegram directly from a network/file payload.
pub fn parse_telegram(input: &[u8]) -> Result<DecodedEew, EewError> {
    let source = utf8(input)?;
    match detect(source) {
        None => return Err(EewError::EmptyInput),
        Some(InputFormat::Xml) => {
            return Err(EewError::InputFormatConflict {
                expected: "telegram",
                actual: "xml",
            });
        }
        Some(InputFormat::Telegram | InputFormat::Auto) => {}
    }
    let telegram = telegram_through_terminator(source)?;
    decoder(telegram, telegram_for_json(telegram))
}

/// Parses JMA EEW XML. Normal reports are always re-encoded and passed to the
/// same Telegram decoder used by [`parse_telegram`].
pub fn parse_xml(input: &[u8]) -> Result<EewOutcome, EewError> {
    let source = utf8(input)?;
    match detect(source) {
        None => return Err(EewError::EmptyInput),
        Some(InputFormat::Telegram) => {
            return Err(EewError::InputFormatConflict {
                expected: "xml",
                actual: "telegram",
            });
        }
        Some(InputFormat::Xml | InputFormat::Auto) => {}
    }

    let document = xml::parse(source)?;
    if document.info_type == InfoType::Cancellation {
        let kind = match document.product {
            XmlProduct::Forecast => CancellationKind::Forecast,
            XmlProduct::Warning => CancellationKind::Warning,
            XmlProduct::Unknown => CancellationKind::Unknown,
        };
        return Ok(EewOutcome::Cancellation(CancellationEvent {
            event_type: "Cancellation".to_string(),
            event_id: document.event_id,
            kind,
            report_datetime: Some(document.report_datetime),
            target_datetime: Some(document.target_datetime),
            title: Some(document.title),
            info_kind: Some(document.info_kind),
            telegram: None,
        }));
    }

    let telegram = telegram_encoder::encode(&document)?;
    let logical = telegram_for_json(&telegram);
    decoder(&telegram, logical).map(EewOutcome::Report)
}

pub fn parse_auto(input: &[u8]) -> Result<EewOutcome, EewError> {
    let source = utf8(input)?;
    match detect(source).ok_or(EewError::EmptyInput)? {
        InputFormat::Xml => parse_xml(input),
        InputFormat::Telegram => parse_telegram(input).map(EewOutcome::Report),
        InputFormat::Auto => unreachable!("detected input is never auto"),
    }
}

fn utf8(input: &[u8]) -> Result<&str, EewError> {
    Ok(std::str::from_utf8(input)?)
}

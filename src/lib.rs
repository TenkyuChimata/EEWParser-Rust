//! Offline JMA EEW XML and code Telegram decoding into a shared semantic model.
//! XML never passes through the lossy legacy Telegram encoder.
mod codes;
mod error;
pub mod input;
mod legacy_xml;
pub mod model;
mod parser;
mod semantic_telegram;
mod state;
mod telegram_encoder;
mod xml;

pub use error::AppError as EewError;
use input::{InputFormat, detect, telegram_for_json, telegram_through_terminator};
pub use model::*;
pub use parser::{Ebi, Telegram as DecodedEew};
pub use semantic_telegram::decode_serial;
pub use state::{EventDecision, EventKey, EventState, EventTracker, Lifecycle, ProductFamily};

/// Maximum accepted payload size. Protects both DOM construction and tokenization.
pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;

pub fn parse_telegram(input: &[u8]) -> Result<EewInformation, EewError> {
    let source = checked_input(input, InputFormat::Telegram)?;
    let telegram = telegram_through_terminator(source)?;
    semantic_telegram::parse(telegram)
}
/// Decode historical NCN corrections only when their protocol convention is
/// known by the caller. Current parsing never guesses the retirement date.
pub fn parse_telegram_with_convention(
    input: &[u8],
    convention: NcnConvention,
) -> Result<EewInformation, EewError> {
    let source = checked_input(input, InputFormat::Telegram)?;
    semantic_telegram::parse_with_convention(telegram_through_terminator(source)?, convention)
}
pub fn parse_xml(input: &[u8]) -> Result<EewInformation, EewError> {
    xml::parse(checked_input(input, InputFormat::Xml)?)
}
pub fn parse_auto(input: &[u8]) -> Result<EewInformation, EewError> {
    let source = utf8(input)?;
    match detect(source).ok_or(EewError::EmptyInput)? {
        InputFormat::Xml => xml::parse(source),
        InputFormat::Telegram => semantic_telegram::parse(telegram_through_terminator(source)?),
        InputFormat::Auto => unreachable!("detected format"),
    }
}
/// Explicit legacy display projection. Prefer [`parse_telegram`] for protocol
/// codes, lifecycle flags, optional values and structured information.
pub fn decode_telegram_legacy(input: &[u8]) -> Result<DecodedEew, EewError> {
    let raw = telegram_through_terminator(checked_input(input, InputFormat::Telegram)?)?;
    parser::decoder(raw, telegram_for_json(raw))
}
/// Explicit, potentially lossy format conversion. This is a generated Telegram,
/// never the original JMA input. Unrepresentable products/states return errors.
pub fn encode_xml_as_telegram(input: &[u8]) -> Result<String, EewError> {
    let raw = checked_input(input, InputFormat::Xml)?;
    telegram_encoder::encode(&legacy_xml::parse(raw)?)
}
fn checked_input(input: &[u8], expected: InputFormat) -> Result<&str, EewError> {
    let source = utf8(input)?;
    let actual = detect(source).ok_or(EewError::EmptyInput)?;
    if actual != expected {
        return Err(EewError::InputFormatConflict {
            expected: expected.as_str(),
            actual: actual.as_str(),
        });
    }
    Ok(source)
}
fn utf8(input: &[u8]) -> Result<&str, EewError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(EewError::InputTooLarge {
            limit: MAX_INPUT_BYTES,
        });
    }
    Ok(std::str::from_utf8(input)?)
}

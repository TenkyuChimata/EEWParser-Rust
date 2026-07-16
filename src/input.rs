use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::str::FromStr;

use crate::error::AppError;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InputFormat {
    #[default]
    Auto,
    Telegram,
    Xml,
}

impl InputFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Telegram => "telegram",
            Self::Xml => "xml",
        }
    }
}

impl FromStr for InputFormat {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "telegram" => Ok(Self::Telegram),
            "xml" => Ok(Self::Xml),
            _ => Err(AppError::InvalidArguments(format!(
                "unknown input format {value:?}; expected auto, telegram, or xml"
            ))),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct RawInput {
    pub content: String,
    pub format: InputFormat,
}

pub fn read_file(path: impl AsRef<Path>, requested: InputFormat) -> Result<RawInput, AppError> {
    let bytes = fs::read(path)?;
    classify(std::str::from_utf8(&bytes)?.to_owned(), requested)
}

pub fn read_stdin(requested: InputFormat) -> Result<RawInput, AppError> {
    read_stream(io::stdin().lock(), requested)
}

/// Reads XML through EOF. Telegram streams may finish as soon as their first `=` is read.
pub fn read_stream<R: Read>(mut reader: R, requested: InputFormat) -> Result<RawInput, AppError> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8192];
    let mut detected = None;

    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);

        if detected.is_none() {
            detected = detect_bytes(&bytes);
        }

        let effective = match requested {
            InputFormat::Auto => detected,
            explicit => Some(explicit),
        };
        if effective == Some(InputFormat::Telegram)
            && let Some(end) = bytes.iter().position(|byte| *byte == b'=')
        {
            bytes.truncate(end + 1);
            break;
        }
    }

    classify(std::str::from_utf8(&bytes)?.to_owned(), requested)
}

pub fn classify(content: String, requested: InputFormat) -> Result<RawInput, AppError> {
    let actual = detect(&content).ok_or(AppError::EmptyInput)?;
    if requested != InputFormat::Auto && requested != actual {
        return Err(AppError::InputFormatConflict {
            expected: requested.as_str(),
            actual: actual.as_str(),
        });
    }

    Ok(RawInput {
        content,
        format: actual,
    })
}

pub fn detect(content: &str) -> Option<InputFormat> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let first = content.trim_start().chars().next()?;
    Some(if first == '<' {
        InputFormat::Xml
    } else {
        InputFormat::Telegram
    })
}

fn detect_bytes(bytes: &[u8]) -> Option<InputFormat> {
    std::str::from_utf8(bytes).ok().and_then(detect)
}

pub fn telegram_through_terminator(content: &str) -> Result<&str, AppError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let content = content.trim_start_matches(char::is_whitespace);
    let end = content
        .find('=')
        .ok_or_else(|| AppError::InvalidTelegram("missing terminal '='".to_string()))?;
    Ok(&content[..=end])
}

pub fn telegram_for_json(content: &str) -> String {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn detects_bom_xml() {
        assert_eq!(detect("\u{feff}  \n<Report/>"), Some(InputFormat::Xml));
        assert_eq!(detect("\u{feff}\u{3000}<Report/>"), Some(InputFormat::Xml));
    }

    #[test]
    fn telegram_stream_stops_at_equals() {
        let input = Cursor::new(b"35 03 00 020117093014 C11 9999=ignored");
        let raw = read_stream(input, InputFormat::Auto).unwrap();
        assert_eq!(raw.format, InputFormat::Telegram);
        assert!(raw.content.ends_with('='));
        assert!(!raw.content.contains("ignored"));
    }

    #[test]
    fn xml_stream_does_not_stop_at_attribute_equals() {
        let xml = br#"<?xml version="1.0"?><Report test="yes"></Report>"#;
        let raw = read_stream(Cursor::new(xml), InputFormat::Auto).unwrap();
        assert_eq!(raw.format, InputFormat::Xml);
        assert!(raw.content.ends_with("</Report>"));

        let unicode_whitespace = "\u{3000}<?xml version=\"1.0\"?><Report test=\"yes\"></Report>";
        let raw = read_stream(Cursor::new(unicode_whitespace), InputFormat::Auto).unwrap();
        assert_eq!(raw.format, InputFormat::Xml);
        assert!(raw.content.ends_with("</Report>"));
    }

    #[test]
    fn explicit_format_conflict_is_an_error() {
        let error = classify("<Report/>".to_string(), InputFormat::Telegram).unwrap_err();
        assert!(matches!(error, AppError::InputFormatConflict { .. }));
    }

    #[test]
    fn reads_complete_xml_file() {
        let path = format!(
            "{}/tests/fixtures/official_100gal_first.xml",
            env!("CARGO_MANIFEST_DIR")
        );
        let raw = read_file(path, InputFormat::Auto).unwrap();
        assert_eq!(raw.format, InputFormat::Xml);
        assert!(raw.content.contains("<ForecastInt>"));
        assert!(raw.content.trim_end().ends_with("</Report>"));
    }

    #[test]
    fn empty_input_is_an_error() {
        assert!(matches!(
            classify("\u{feff} \n\t".to_string(), InputFormat::Auto),
            Err(AppError::EmptyInput)
        ));
    }
}

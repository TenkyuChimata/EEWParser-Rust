use std::fs;
use std::io::{self, Read};
use std::path::Path;
use std::str::FromStr;

use crate::MAX_INPUT_BYTES;
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
    let file = fs::File::open(path)?;
    read_stream(file, requested)
}

pub fn read_stdin(requested: InputFormat) -> Result<RawInput, AppError> {
    read_stream(io::stdin().lock(), requested)
}

/// Reads XML through EOF. Telegram streams may finish as soon as their first `=` is read.
pub fn read_stream<R: Read>(mut reader: R, requested: InputFormat) -> Result<RawInput, AppError> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8192];
    let mut detected = None;
    let mut detection_position = 0;

    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        if bytes.len() + count > MAX_INPUT_BYTES {
            return Err(AppError::InputTooLarge {
                limit: MAX_INPUT_BYTES,
            });
        }
        let previous_length = bytes.len();
        bytes.extend_from_slice(&chunk[..count]);

        if detected.is_none() {
            detected = detect_bytes(&bytes, &mut detection_position);
        }

        let effective = match requested {
            InputFormat::Auto => detected,
            explicit => Some(explicit),
        };
        if effective == Some(InputFormat::Telegram)
            && let Some(end) = bytes[previous_length..]
                .iter()
                .position(|byte| *byte == b'=')
        {
            bytes.truncate(previous_length + end + 1);
            break;
        }
    }

    classify(std::str::from_utf8(&bytes)?.to_owned(), requested)
}

pub fn classify(content: String, requested: InputFormat) -> Result<RawInput, AppError> {
    if content.len() > MAX_INPUT_BYTES {
        return Err(AppError::InputTooLarge {
            limit: MAX_INPUT_BYTES,
        });
    }
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

// Examine only the undecided UTF-8 prefix. A split multibyte character or BOM
// is retried when complete, without repeatedly scanning all prior whitespace.
fn detect_bytes(bytes: &[u8], position: &mut usize) -> Option<InputFormat> {
    if *position == 0 {
        const BOM: &[u8] = b"\xef\xbb\xbf";
        if bytes.len() < BOM.len() && BOM.starts_with(bytes) {
            return None;
        }
        if bytes.starts_with(BOM) {
            *position = BOM.len();
        }
    }
    let suffix = &bytes[*position..];
    let valid = match std::str::from_utf8(suffix) {
        Ok(s) => s,
        Err(e) => std::str::from_utf8(&suffix[..e.valid_up_to()]).expect("validated UTF-8 prefix"),
    };
    for c in valid.chars() {
        if !c.is_whitespace() {
            return Some(if c == '<' {
                InputFormat::Xml
            } else {
                InputFormat::Telegram
            });
        }
        *position += c.len_utf8();
    }
    None
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

// roxmltree 0.20 recursively parses element content. Bound nesting before
// constructing either XML DOM; the byte/node limits do not bound stack usage.
// This is only a lexical resource guard. roxmltree still validates XML syntax.
pub(crate) fn check_xml_depth(mut xml: &str) -> Result<(), AppError> {
    const MAX_DEPTH: usize = 128;
    let invalid = |message: &str| AppError::InvalidXml(message.to_owned());
    let mut depth = 0_usize;
    'markup: while let Some(start) = xml.find('<') {
        xml = &xml[start..];
        for (prefix, end) in [("<!--", "-->"), ("<![CDATA[", "]]>"), ("<?", "?>")] {
            if let Some(content) = xml.strip_prefix(prefix) {
                let end_pos = content.find(end).ok_or_else(|| {
                    invalid("unterminated XML comment, CDATA or processing instruction")
                })?;
                xml = &content[end_pos + end.len()..];
                continue 'markup;
            }
        }
        if xml.starts_with("<!") {
            return Err(invalid("DTD and markup declarations are not supported"));
        }
        let closing = xml.starts_with("</");
        let mut quote = None;
        let mut end = None;
        for (index, byte) in xml.bytes().enumerate().skip(1) {
            match (quote, byte) {
                (Some(q), b) if q == b => quote = None,
                (Some(_), _) => {}
                (None, b'\'' | b'"') => quote = Some(byte),
                (None, b'>') => {
                    end = Some(index);
                    break;
                }
                (None, b'<') => return Err(invalid("unexpected < inside an XML tag")),
                _ => {}
            }
        }
        let end = end.ok_or_else(|| invalid("unterminated XML tag"))?;
        if closing {
            depth = depth.saturating_sub(1);
        } else {
            if depth >= MAX_DEPTH {
                return Err(invalid("XML element nesting exceeds the 128-level limit"));
            }
            if xml.as_bytes()[end - 1] != b'/' {
                depth += 1;
            }
        }
        xml = &xml[end + 1..];
    }
    Ok(())
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
    #[test]
    fn one_byte_chunks_preserve_split_bom_and_unicode_whitespace() {
        struct OneByte(Cursor<Vec<u8>>);
        impl Read for OneByte {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                let len = out.len().min(1);
                self.0.read(&mut out[..len])
            }
        }
        let payload = "\u{feff}\u{3000}  <Report a=\"x\"/>";
        let result = read_stream(
            OneByte(Cursor::new(payload.as_bytes().to_vec())),
            InputFormat::Auto,
        )
        .unwrap();
        assert_eq!(result.format, InputFormat::Xml);
        assert_eq!(result.content, payload);
    }
    #[test]
    fn unterminated_and_whitespace_streams_obey_the_memory_bound() {
        let input = io::repeat(b' ').take(MAX_INPUT_BYTES as u64 + 1);
        assert!(matches!(
            read_stream(input, InputFormat::Auto),
            Err(AppError::InputTooLarge { .. })
        ));
        let input = Cursor::new(
            b"37 "
                .iter()
                .copied()
                .chain(std::iter::repeat_n(b'0', 24_000))
                .collect::<Vec<_>>(),
        );
        assert!(read_stream(input, InputFormat::Telegram).is_ok());
        // read_stream classifies complete bytes; parse_telegram diagnoses the missing terminator.
        let input = vec![b'0'; 24_000];
        assert!(matches!(
            crate::parse_telegram(&input),
            Err(AppError::InvalidTelegram(_))
        ));
    }

    #[test]
    fn xml_depth_guard_counts_elements_and_skips_quoted_or_literal_markup() {
        let at_limit = format!("{}<leaf/>{}", "<n>".repeat(127), "</n>".repeat(127));
        assert!(check_xml_depth(&at_limit).is_ok());
        assert!(roxmltree::Document::parse(&at_limit).is_ok());
        let too_deep = format!("<n>{at_limit}</n>");
        assert!(matches!(
            check_xml_depth(&too_deep),
            Err(AppError::InvalidXml(_))
        ));
        let literal = "<n>".repeat(5_000);
        let xml = format!(
            "<n a='>' b=\"/>\"><!--{literal}--><![CDATA[{literal}]]><?test {literal}?><leaf/></n>"
        );
        assert!(check_xml_depth(&xml).is_ok());
        assert!(roxmltree::Document::parse(&xml).is_ok());
        for malformed in [
            "<n",
            "<n a='",
            "<n <n>",
            "<!--",
            "<![CDATA[",
            "<?test",
            "<!DOCTYPE n>",
        ] {
            assert!(check_xml_depth(malformed).is_err(), "{malformed}");
        }
    }
}

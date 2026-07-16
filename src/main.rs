use std::env;
use std::process;
use std::str::FromStr;

use eew_parser::input::{InputFormat, read_file, read_stdin};
use eew_parser::{CancellationKind, EewError, EewOutcome, parse_telegram, parse_xml};

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), EewError> {
    let options = Options::parse(env::args().skip(1))?;
    let input = if options.stdin {
        read_stdin(options.input_format)?
    } else {
        read_file(
            options.path.as_deref().unwrap_or("./data.txt"),
            options.input_format,
        )?
    };

    let outcome = match input.format {
        InputFormat::Telegram => {
            parse_telegram(input.content.as_bytes()).map(EewOutcome::Report)?
        }
        InputFormat::Xml => parse_xml(input.content.as_bytes())?,
        InputFormat::Auto => unreachable!("classified input is never auto"),
    };

    match outcome {
        EewOutcome::Report(report) => println!("{}", serde_json::to_string(&report)?),
        EewOutcome::Cancellation(cancellation) if options.emit_control_events => {
            println!("{}", serde_json::to_string(&cancellation)?)
        }
        EewOutcome::Cancellation(cancellation) => {
            let xml_type = match cancellation.kind {
                CancellationKind::Forecast => "VXSE44/VXSE45",
                CancellationKind::Warning => "VXSE43",
                CancellationKind::Unknown => "unknown",
            };
            return Err(EewError::Unsupported {
                event_id: Some(cancellation.event_id),
                xml_type: Some(xml_type.to_string()),
                info_type: Some("取消".to_string()),
                field: Some("Body/Text".to_string()),
                reason:
                    "cancellation is a control event; pass --emit-control-events to emit its JSON"
                        .to_string(),
            });
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Options {
    path: Option<String>,
    stdin: bool,
    input_format: InputFormat,
    emit_control_events: bool,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, EewError> {
        let mut path = None;
        let mut stdin = false;
        let mut input_format = InputFormat::Auto;
        let mut emit_control_events = false;
        let mut args = args.peekable();

        while let Some(argument) = args.next() {
            match argument.as_str() {
                "-p" => {
                    path = Some(args.next().ok_or_else(|| {
                        EewError::InvalidArguments("-p requires a path".to_string())
                    })?);
                }
                "-s" => stdin = true,
                "--emit-control-events" => emit_control_events = true,
                "--input-format" => {
                    let value = args.next().ok_or_else(|| {
                        EewError::InvalidArguments(
                            "--input-format requires auto, telegram, or xml".to_string(),
                        )
                    })?;
                    input_format = InputFormat::from_str(&value)?;
                }
                _ => {
                    return Err(EewError::InvalidArguments(format!(
                        "unknown argument {argument:?}"
                    )));
                }
            }
        }
        if stdin && path.is_some() {
            return Err(EewError::InvalidArguments(
                "-p and -s cannot be used together".to_string(),
            ));
        }
        Ok(Self {
            path,
            stdin,
            input_format,
            emit_control_events,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_input_format_option() {
        let options = Options::parse(
            ["--input-format", "xml", "-p", "sample.xml"]
                .into_iter()
                .map(str::to_string),
        )
        .unwrap();
        assert_eq!(options.input_format, InputFormat::Xml);
        assert_eq!(options.path.as_deref(), Some("sample.xml"));
        assert!(!options.emit_control_events);
    }

    #[test]
    fn parses_emit_control_events_option() {
        let options = Options::parse(
            ["--emit-control-events", "-s"]
                .into_iter()
                .map(str::to_string),
        )
        .unwrap();
        assert!(options.emit_control_events);
    }
}

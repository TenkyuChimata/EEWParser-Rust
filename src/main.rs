use std::env;
use std::process;
use std::str::FromStr;

use eew_parser::input::{InputFormat, read_file, read_stdin};
use eew_parser::{EewError, parse_telegram, parse_xml};

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

    let information = match input.format {
        InputFormat::Telegram => parse_telegram(input.content.as_bytes())?,
        InputFormat::Xml => parse_xml(input.content.as_bytes())?,
        InputFormat::Auto => unreachable!("classified input is never auto"),
    };
    println!("{}", serde_json::to_string(&information)?);
    Ok(())
}

#[derive(Debug)]
struct Options {
    path: Option<String>,
    stdin: bool,
    input_format: InputFormat,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, EewError> {
        let mut path = None;
        let mut stdin = false;
        let mut input_format = InputFormat::Auto;

        let mut args = args.peekable();

        while let Some(argument) = args.next() {
            match argument.as_str() {
                "-p" => {
                    path = Some(args.next().ok_or_else(|| {
                        EewError::InvalidArguments("-p requires a path".to_string())
                    })?);
                }
                "-s" => stdin = true,
                "--emit-control-events" => {} // accepted for old CLI scripts; all messages now share one schema
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
    }

    #[test]
    fn parses_emit_control_events_option() {
        let options = Options::parse(
            ["--emit-control-events", "-s"]
                .into_iter()
                .map(str::to_string),
        )
        .unwrap();
        assert!(options.stdin);
    }
}

use eew_parser::{
    MAX_INPUT_BYTES,
    input::{InputFormat, read_stream},
    parse_telegram,
};
use std::io::{self, Cursor, Read};

struct Chunked<R> {
    reader: R,
    size: usize,
    interrupt: bool,
}
impl<R: Read> Read for Chunked<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if std::mem::take(&mut self.interrupt) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let size = bytes.len().min(self.size);
        self.reader.read(&mut bytes[..size])
    }
}

#[test]
fn transient_interruption_does_not_discard_an_otherwise_valid_message() {
    let input = include_bytes!("fixtures/official_telegram/manual-38.telegram");
    let reader = Chunked {
        reader: Cursor::new(input),
        size: 3,
        interrupt: true,
    };
    let result = read_stream(reader, InputFormat::Auto).unwrap();
    assert_eq!(
        parse_telegram(result.content.as_bytes()).unwrap(),
        parse_telegram(input).unwrap()
    );
}

#[test]
fn telegram_size_bound_counts_only_bytes_through_its_terminator() {
    let mut input = b"38 03 00 261107000000 C11 9999 ".to_vec();
    input.resize(MAX_INPUT_BYTES - 1, b'a');
    input.push(b'=');
    let expected_length = input.len();
    input.extend(std::iter::repeat_n(b'x', 6000));
    let reader = Chunked {
        reader: Cursor::new(input),
        size: 6000,
        interrupt: false,
    };
    let result = read_stream(reader, InputFormat::Auto).unwrap();
    assert_eq!(result.content.len(), expected_length);
    assert!(result.content.ends_with('='));
}

#[test]
fn mutated_and_truncated_bytes_never_panic_in_public_decoders() {
    // Deterministic adversarial smoke corpus, independent of semantic expected
    // values. Invalid input can return an error; it must not unwind or hang.
    let seeds: &[&[u8]] = &[
        include_bytes!("fixtures/official_telegram/manual-35.telegram"),
        include_bytes!("fixtures/official_telegram/manual-37.telegram"),
        include_bytes!("fixtures/official_telegram/manual-38.telegram"),
        include_bytes!("fixtures/official_telegram/manual-47.telegram"),
        include_bytes!("fixtures/official_telegram/manual-61.telegram"),
        include_bytes!("fixtures/official_100gal_first.xml"),
        include_bytes!("fixtures/official_plum_forecast.xml"),
        include_bytes!("fixtures/official_noto_cancellation.xml"),
    ];
    let check = |bytes: &[u8]| {
        let result = std::panic::catch_unwind(|| {
            let _ = eew_parser::parse_auto(bytes);
            let _ = eew_parser::parse_xml(bytes);
            let _ = eew_parser::parse_telegram(bytes);
            let _ = eew_parser::parse_telegram_with_convention(
                bytes,
                eew_parser::NcnConvention::Legacy2013,
            );
            let _ = eew_parser::decode_telegram_legacy(bytes);
            let _ = eew_parser::encode_xml_as_telegram(bytes);
        });
        assert!(result.is_ok(), "public decoder panicked for {bytes:?}");
    };
    let mut random = 0x73a2_ef19_32b0_5dc1_u64;
    for seed in seeds {
        check(seed);
        // Exercise UTF-8 boundaries as well as truncated protocol tokens.
        for end in 0..seed.len() {
            check(&seed[..end]);
        }
        for _ in 0..256 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let position = random as usize % seed.len();
            let mut input = seed.to_vec();
            match random % 4 {
                0 => input[position] = (random >> 32) as u8,
                1 => {
                    input.remove(position);
                }
                2 => input
                    .splice(position..position, [b'<', b'/', 0xff])
                    .for_each(drop),
                _ => input
                    .splice(
                        position..position,
                        seed[position..].iter().copied().take(12),
                    )
                    .for_each(drop),
            }
            check(&input);
        }
    }
}

#[test]
fn xml_allocation_estimates_are_bounded_even_when_dom_node_count_is_tiny() {
    let base = include_str!("fixtures/official_noto_first.xml");
    for content in [
        "=".repeat(250_001),
        format!("<![CDATA[{}]]>", "<".repeat(250_001)),
        format!("<!--{}-->", "<".repeat(250_001)),
    ] {
        let xml = base.replace("</Body>", &format!("<Text>{content}</Text></Body>"));
        assert!(xml.len() < MAX_INPUT_BYTES);
        for result in [
            eew_parser::parse_xml(xml.as_bytes()).map(|_| ()),
            eew_parser::parse_auto(xml.as_bytes()).map(|_| ()),
            eew_parser::encode_xml_as_telegram(xml.as_bytes()).map(|_| ()),
        ] {
            assert!(
                matches!(result, Err(eew_parser::EewError::InvalidXml(message))
                if message.contains("allocation markers"))
            );
        }
    }
}

#[test]
fn large_xml_text_remains_valid_within_the_allocation_budget() {
    let text = "x".repeat(1_000_000);
    let xml = include_str!("fixtures/official_noto_first.xml")
        .replace("</Body>", &format!("<Text>{text}</Text></Body>"));
    assert_eq!(
        eew_parser::parse_xml(xml.as_bytes()).unwrap().body.text,
        Some(text)
    );
}

#[test]
fn many_adjacent_cdata_sections_preserve_the_complete_text() {
    let xml = include_str!("fixtures/official_noto_first.xml").replace(
        "</Body>",
        &format!("<Text>{}</Text></Body>", "<![CDATA[x]]>".repeat(20_000)),
    );
    assert_eq!(
        eew_parser::parse_xml(xml.as_bytes()).unwrap().body.text,
        Some("x".repeat(20_000))
    );
}

#[test]
fn xml_attribute_work_is_bounded_per_tag_without_counting_quoted_equals() {
    let base = include_str!("fixtures/official_noto_first.xml");
    for count in [256, 257] {
        let attrs = (0..count)
            .map(|n| format!(" x{n}=\"a\""))
            .collect::<String>();
        let xml = base.replace("</Body>", &format!("<Text{attrs}>text</Text></Body>"));
        for result in [
            eew_parser::parse_xml(xml.as_bytes()).map(|_| ()),
            eew_parser::parse_auto(xml.as_bytes()).map(|_| ()),
            eew_parser::encode_xml_as_telegram(xml.as_bytes()).map(|_| ()),
        ] {
            if count == 256 {
                assert!(result.is_ok());
            } else {
                assert!(
                    matches!(result, Err(eew_parser::EewError::InvalidXml(message))
                    if message.contains("256-attribute"))
                );
            }
        }
    }
    let xml = base.replace(
        "</Body>",
        &format!("<Text data=\"{}\">text</Text></Body>", "=".repeat(300)),
    );
    assert_eq!(
        eew_parser::parse_xml(xml.as_bytes())
            .unwrap()
            .body
            .text
            .as_deref(),
        Some("text")
    );
}

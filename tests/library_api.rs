use std::process::Command;

use eew_parser::{
    CancellationEvent, CancellationKind, EewError, EewOutcome, EventDecision, EventTracker,
    parse_auto, parse_telegram, parse_xml,
};

const DATA: &[u8] = include_bytes!("../data.txt");
const GAL_100: &[u8] = include_bytes!("fixtures/official_100gal_first.xml");
const PLUM_WARNING: &[u8] = include_bytes!("fixtures/official_plum_warning.xml");
const PLUM_FORECAST: &[u8] = include_bytes!("fixtures/official_plum_forecast.xml");
const PLUM_CANCEL: &str = include_str!("fixtures/official_plum_cancellation.xml");
const NOTO_FIRST: &[u8] = include_bytes!("fixtures/official_noto_first.xml");
const NOTO_FINAL: &[u8] = include_bytes!("fixtures/official_noto_final.xml");

fn report(outcome: EewOutcome) -> eew_parser::DecodedEew {
    match outcome {
        EewOutcome::Report(report) => report,
        EewOutcome::Cancellation(_) => panic!("expected a report"),
    }
}

fn cancellation(outcome: EewOutcome) -> CancellationEvent {
    match outcome {
        EewOutcome::Cancellation(cancellation) => cancellation,
        EewOutcome::Report(_) => panic!("expected a cancellation"),
    }
}

#[test]
fn telegram_and_auto_library_apis_accept_byte_payloads() {
    let direct = parse_telegram(DATA).unwrap();
    let auto = report(parse_auto(DATA).unwrap());
    assert_eq!(direct, auto);
    assert_eq!(direct.event_id(), "20240101161010");
    assert_eq!(direct.serial(), Some(46));
    assert!(direct.is_final());
}

#[test]
fn all_normal_xml_fixtures_preserve_telegram_and_decoder_semantics() {
    let cases: &[(&[u8], &str)] = &[
        (
            GAL_100,
            include_str!("fixtures/expected/official_100gal_first.telegram"),
        ),
        (
            PLUM_WARNING,
            include_str!("fixtures/expected/official_plum_warning.telegram"),
        ),
        (
            PLUM_FORECAST,
            include_str!("fixtures/expected/official_plum_forecast.telegram"),
        ),
        (
            NOTO_FIRST,
            include_str!("fixtures/expected/official_noto_first.telegram"),
        ),
        (
            NOTO_FINAL,
            include_str!("fixtures/expected/official_noto_final.telegram"),
        ),
    ];

    for (xml, expected_telegram) in cases {
        let actual = report(parse_xml(xml).unwrap());
        let expected_telegram = expected_telegram.trim_end();
        assert_eq!(actual.Telegram, expected_telegram);

        // Decoding the frozen pre-hardening Telegram through the public common
        // decoder must produce the complete same result, not just key fields.
        let expected = parse_telegram(expected_telegram.as_bytes()).unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn cancellation_xml_returns_structured_control_events_without_telegram() {
    let forecast = cancellation(parse_xml(PLUM_CANCEL.as_bytes()).unwrap());
    assert_eq!(forecast.event_id, "20230906063640");
    assert_eq!(forecast.kind, CancellationKind::Forecast);
    assert_eq!(
        forecast.report_datetime.as_deref(),
        Some("2023-09-06T06:37:52+09:00")
    );
    assert_eq!(
        forecast.target_datetime.as_deref(),
        Some("2023-09-06T06:37:52+09:00")
    );
    assert_eq!(forecast.info_kind.as_deref(), Some("緊急地震速報"));
    assert_eq!(forecast.telegram, None);

    // VXSE43 and cancellation bodies use the same official control structure;
    // switching both official Title elements exercises the warning classifier.
    let warning_xml = PLUM_CANCEL.replace("緊急地震速報（地震動予報）", "緊急地震速報（警報）");
    let warning = cancellation(parse_xml(warning_xml.as_bytes()).unwrap());
    assert_eq!(warning.kind, CancellationKind::Warning);
    assert!(warning.telegram.is_none());

    let unknown_xml = PLUM_CANCEL.replace("緊急地震速報（地震動予報）", "緊急地震速報");
    let unknown = cancellation(parse_xml(unknown_xml.as_bytes()).unwrap());
    assert_eq!(unknown.kind, CancellationKind::Unknown);
    assert!(unknown.telegram.is_none());
}

#[test]
fn cancellation_without_event_id_is_an_error() {
    let xml = PLUM_CANCEL.replace("<EventID>20230906063640</EventID>", "");
    assert!(matches!(
        parse_xml(xml.as_bytes()),
        Err(EewError::MissingXmlField(_))
    ));
}

#[test]
fn correction_rank_zero_and_test_status_remain_contextual_unsupported_errors() {
    let correction = std::str::from_utf8(GAL_100)
        .unwrap()
        .replace("<InfoType>発表</InfoType>", "<InfoType>訂正</InfoType>");
    assert!(matches!(
        parse_xml(correction.as_bytes()),
        Err(EewError::Unsupported {
            event_id: Some(event_id),
            info_type: Some(info_type),
            field: Some(field),
            ..
        }) if event_id == "20230513161031" && info_type == "訂正" && field == "Head/InfoType"
    ));

    let rank_zero = std::str::from_utf8(NOTO_FIRST).unwrap().replacen(
        "<Epicenter rank=\"3\"",
        "<Epicenter rank=\"0\"",
        1,
    );
    assert!(matches!(
        parse_xml(rank_zero.as_bytes()),
        Err(EewError::Unsupported { field: Some(field), reason, .. })
            if field.ends_with("Epicenter/@rank") && reason.contains("36 or 37")
    ));

    let trial = std::str::from_utf8(GAL_100)
        .unwrap()
        .replace("<Status>通常</Status>", "<Status>試験</Status>");
    assert!(matches!(
        parse_xml(trial.as_bytes()),
        Err(EewError::Unsupported { field: Some(field), reason, .. })
            if field == "Control/Status" && reason.contains("20 or 30")
    ));

    let trial_cancellation = PLUM_CANCEL.replace("<Status>通常</Status>", "<Status>試験</Status>");
    assert!(matches!(
        parse_xml(trial_cancellation.as_bytes()),
        Err(EewError::Unsupported { field: Some(field), .. })
            if field == "Control/Status"
    ));
}

#[test]
fn invalid_utf8_is_rejected_without_lossy_conversion() {
    assert!(matches!(
        parse_auto(&[0xff, 0xfe, b'<']),
        Err(EewError::InvalidUtf8(_))
    ));
}

#[test]
fn event_tracker_classifies_serials_final_and_isolates_events() {
    let first = report(parse_xml(NOTO_FIRST).unwrap());
    let final_report = report(parse_xml(NOTO_FINAL).unwrap());
    let other = report(parse_xml(GAL_100).unwrap());
    let mut tracker = EventTracker::new();

    assert_eq!(
        tracker.observe(&EewOutcome::Report(first.clone())),
        EventDecision::Accepted
    );
    assert_eq!(
        tracker.observe(&EewOutcome::Report(first.clone())),
        EventDecision::Duplicate
    );

    let mut serial_two = first.clone();
    serial_two.WarnNum = "02".to_string();
    assert_eq!(
        tracker.observe(&EewOutcome::Report(serial_two)),
        EventDecision::Accepted
    );
    assert_eq!(
        tracker.observe(&EewOutcome::Report(first)),
        EventDecision::OlderThanLatest
    );
    assert_eq!(
        tracker.observe(&EewOutcome::Report(final_report)),
        EventDecision::Final
    );
    let state = tracker.state("20240101161010").unwrap();
    assert_eq!(state.latest_serial, Some(46));
    assert!(state.is_final);

    assert_eq!(
        tracker.observe(&EewOutcome::Report(other)),
        EventDecision::Accepted
    );
    assert_eq!(tracker.len(), 2);
    assert_eq!(
        tracker.state("20230513161031").unwrap().latest_serial,
        Some(1)
    );
}

#[test]
fn tracker_does_not_turn_unparseable_serial_into_zero() {
    let mut unordered = report(parse_xml(NOTO_FIRST).unwrap());
    unordered.WarnNum = "//".to_string();
    let mut tracker = EventTracker::new();
    assert_eq!(
        tracker.observe(&EewOutcome::Report(unordered.clone())),
        EventDecision::Accepted
    );
    assert_eq!(
        tracker.state(unordered.event_id()).unwrap().latest_serial,
        None
    );
    assert_eq!(
        tracker.observe(&EewOutcome::Report(unordered)),
        EventDecision::Duplicate
    );
}

#[test]
fn tracker_always_reports_cancellation_and_marks_existence() {
    let first = report(parse_xml(NOTO_FIRST).unwrap());
    let mut known_cancel = cancellation(parse_xml(PLUM_CANCEL.as_bytes()).unwrap());
    known_cancel.event_id = first.event_id().to_string();
    let unknown_cancel = cancellation(parse_xml(PLUM_CANCEL.as_bytes()).unwrap());
    let mut tracker = EventTracker::new();

    assert_eq!(
        tracker.observe(&EewOutcome::Report(first)),
        EventDecision::Accepted
    );
    assert_eq!(
        tracker.observe(&EewOutcome::Cancellation(known_cancel)),
        EventDecision::Cancelled { existed: true }
    );
    assert!(tracker.state("20240101161010").unwrap().is_cancelled);
    assert_eq!(
        tracker.observe(&EewOutcome::Cancellation(unknown_cancel)),
        EventDecision::Cancelled { existed: false }
    );
    assert!(tracker.state("20230906063640").unwrap().is_cancelled);
}

#[test]
fn cli_requires_opt_in_before_emitting_control_events() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/official_plum_cancellation.xml"
    );
    let strict = Command::new(env!("CARGO_BIN_EXE_EEWParser-Rust"))
        .args(["--input-format", "xml", "-p", fixture])
        .output()
        .unwrap();
    assert!(!strict.status.success());
    assert!(
        String::from_utf8(strict.stderr)
            .unwrap()
            .contains("--emit-control-events")
    );

    let emitted = Command::new(env!("CARGO_BIN_EXE_EEWParser-Rust"))
        .args([
            "--emit-control-events",
            "--input-format",
            "xml",
            "-p",
            fixture,
        ])
        .output()
        .unwrap();
    assert!(emitted.status.success());
    let value: serde_json::Value = serde_json::from_slice(&emitted.stdout).unwrap();
    assert_eq!(value["Type"], "Cancellation");
    assert_eq!(value["EventID"], "20230906063640");
    assert_eq!(value["CancellationKind"], "Forecast");
    assert!(value["Telegram"].is_null());
}

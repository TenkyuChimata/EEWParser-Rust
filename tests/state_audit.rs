//! Independent state-policy regressions. All mutations here are synthetic.
use eew_parser::*;

fn xml_message() -> EewInformation {
    parse_xml(include_bytes!("fixtures/official_noto_first.xml")).unwrap()
}

#[test]
fn vxse44_and_vxse45_with_the_same_serial_are_distinct_information_units() {
    let ground_motion = xml_message();
    assert_eq!(ground_motion.product(), Product::GroundMotionForecast);
    let mut forecast = ground_motion.clone();
    forecast.product = Product::Forecast;
    forecast.header.information_type = Some("緊急地震速報（予報）".into());
    let mut tracker = EventTracker::new();

    // DMDATA can deliver the VXSE44 projection before the richer VXSE45 report.
    assert_eq!(tracker.observe(&forecast), EventDecision::Accepted);
    assert_eq!(tracker.observe(&ground_motion), EventDecision::Accepted);
    assert_eq!(tracker.len(), 2);
    assert_ne!(
        EventTracker::key(&forecast),
        EventTracker::key(&ground_motion)
    );

    let mut cancellation = ground_motion.clone();
    cancellation.header.info_type = Some(InfoType::Cancellation);
    cancellation.body.is_canceled = true;
    cancellation.body.is_last_info = true;
    assert_eq!(
        tracker.observe(&cancellation),
        EventDecision::Cancelled { existed: true }
    );
    assert!(
        !tracker
            .state(&EventTracker::key(&forecast).unwrap())
            .unwrap()
            .is_cancelled
    );
}

#[test]
fn source_less_unnumbered_updates_are_not_duplicates_merely_because_headers_match() {
    let mut first = xml_message();
    first.source = None;
    first.header.serial_no = None;
    let mut changed = first.clone();
    changed
        .body
        .earthquake
        .as_mut()
        .unwrap()
        .magnitude
        .as_mut()
        .unwrap()
        .value = Some("7.0".into());
    let mut tracker = EventTracker::new();
    assert_eq!(tracker.observe(&first), EventDecision::Accepted);
    assert_eq!(tracker.observe(&changed), EventDecision::Accepted);
    assert_eq!(tracker.observe(&changed), EventDecision::Duplicate);
}

#[test]
fn same_second_corrections_are_distinguished_by_their_payload() {
    let mut first = xml_message();
    first.source = None;
    first.header.info_type = Some(InfoType::Correction);
    let mut correction = first.clone();
    correction.body.comments = Some(Comments {
        free: Some("訂正内容".into()),
        warning: None,
    });
    let mut tracker = EventTracker::new();
    assert_eq!(tracker.observe(&first), EventDecision::Accepted);
    assert_eq!(tracker.observe(&correction), EventDecision::Accepted);
    assert_eq!(tracker.observe(&correction), EventDecision::Duplicate);
}

#[test]
fn a_newer_correction_replaces_an_erroneous_serial_and_allows_following_reports() {
    let mut first = xml_message();
    first.header.serial_no = Some("99".into());
    first.header.press_date_time = Some("2024-01-01T07:10:00Z".into());
    let mut correction = first.clone();
    correction.header.serial_no = Some("1".into());
    correction.header.info_type = Some(InfoType::Correction);
    correction.header.press_date_time = Some("2024-01-01T07:10:01Z".into());
    let mut second = correction.clone();
    second.header.serial_no = Some("2".into());
    second.header.info_type = Some(InfoType::Publication);
    second.header.press_date_time = Some("2024-01-01T07:10:02Z".into());
    let mut tracker = EventTracker::new();
    assert_eq!(tracker.observe(&first), EventDecision::Accepted);
    assert_eq!(tracker.observe(&correction), EventDecision::Accepted);
    assert_eq!(tracker.observe(&second), EventDecision::Accepted);
    assert_eq!(
        tracker
            .state(&EventTracker::key(&second).unwrap())
            .unwrap()
            .latest_serial,
        Some(2)
    );
    assert_eq!(tracker.observe(&correction), EventDecision::OlderThanLatest);
}

#[test]
fn report_time_does_not_override_control_time_for_revisions() {
    let mut first = xml_message();
    first.header.press_date_time = Some("2024-01-01T07:10:01Z".into());
    first.header.report_date_time = Some("2024-01-01T16:10:01+09:00".into());
    let mut correction = first.clone();
    correction.header.info_type = Some(InfoType::Correction);
    correction.header.press_date_time = Some("2024-01-01T07:10:02Z".into());
    correction.header.report_date_time = Some("2024-01-01T16:09:00+09:00".into());
    let mut tracker = EventTracker::new();
    tracker.observe(&first);
    assert_eq!(tracker.observe(&correction), EventDecision::Accepted);
    let mut stale = correction.clone();
    stale.header.press_date_time = first.header.press_date_time;
    stale.header.report_date_time = Some("2024-01-01T16:11:00+09:00".into());
    assert_eq!(tracker.observe(&stale), EventDecision::OlderThanLatest);
}

#[test]
fn a_lower_correction_serial_requires_evidence_of_a_newer_revision() {
    let mut first = xml_message();
    first.source = None;
    first.header.serial_no = Some("99".into());
    first.header.press_date_time = None;
    first.header.report_date_time = None;
    let mut correction = first.clone();
    correction.header.serial_no = Some("1".into());
    correction.header.info_type = Some(InfoType::Correction);
    let mut tracker = EventTracker::new();
    tracker.observe(&first);
    assert_eq!(tracker.observe(&correction), EventDecision::OlderThanLatest);
    assert_eq!(
        tracker
            .state(&EventTracker::key(&first).unwrap())
            .unwrap()
            .latest_serial,
        Some(99)
    );
}

#[test]
fn incomplete_json_event_identity_cannot_create_a_shared_empty_series() {
    for invalid_id in [
        "",
        " ",
        "2024010116100",
        "2024010116100X",
        "２０２４０１０１１６１０００",
    ] {
        let mut message = xml_message();
        message.header.event_id = Some(invalid_id.into());
        let mut tracker = EventTracker::new();
        assert_eq!(EventTracker::key(&message), None);
        assert_eq!(tracker.observe(&message), EventDecision::MissingIdentity);
        assert!(tracker.is_empty());
    }
}

#[test]
fn a_warning_cannot_be_reinstated_by_the_nested_forecasts_legacy_ncn7() {
    let cancellation = parse_telegram(include_bytes!(
        "fixtures/official_telegram/manual-48.telegram"
    ))
    .unwrap();
    let raw = include_str!("fixtures/official_telegram/manual-47.telegram")
        .replace("NCN001", "NCN701")
        .replace("061004150000", "061004150400");
    let warning =
        parse_telegram_with_convention(raw.as_bytes(), NcnConvention::Legacy2013).unwrap();
    assert_eq!(warning.product(), Product::Warning);
    assert_eq!(warning.header.info_type, Some(InfoType::Publication));
    let mut tracker = EventTracker::new();
    assert_eq!(
        tracker.observe(&cancellation),
        EventDecision::Cancelled { existed: false }
    );
    assert_eq!(tracker.observe(&warning), EventDecision::AfterCancellation);
    assert!(
        tracker
            .state(&EventTracker::key(&warning).unwrap())
            .unwrap()
            .is_cancelled
    );
}

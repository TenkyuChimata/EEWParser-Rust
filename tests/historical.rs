//! Observed DMDATA JSON is a reference, never passed to an XML/Telegram parser.
//! Any message built from a reference in lifecycle tests is explicitly synthetic.
use eew_parser::*;
use serde_json::Value;
use std::{fs, path::Path};
fn observed() -> Vec<EewInformation> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/observed/20261016123810");
    let mut all = Vec::new();
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let original: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        let typed: EewInformation = serde_json::from_value(original.clone()).unwrap();
        // Complete field-value equality, including nested arrays, names, codes,
        // ranges, nulls, missing fields, LPGM and comments, in all 43 documents.
        let mut expected = original;
        expected.as_object_mut().unwrap().remove("_originalId");
        expected.as_object_mut().unwrap().remove("_schema");
        let mut actual = serde_json::to_value(&typed).unwrap();
        actual.as_object_mut().unwrap().remove("product");
        assert_eq!(actual, expected, "{}", p.display());
        assert_eq!(typed.product(), Product::GroundMotionForecast);
        all.push(typed);
    }
    all.sort_by(|a, b| {
        a.message_time()
            .cmp(&b.message_time())
            .then_with(|| a.serial().cmp(&b.serial()))
    });
    assert_eq!(all.len(), 43);
    all
}
#[test]
fn all_43_references_preserve_every_field_and_known_transitions() {
    let all = observed();
    let magnitudes = [
        "6.2", "6.7", "6.5", "6.6", "6.7", "6.7", "7.4", "6.9", "7.6", "7.2", "7.2", "6.7", "7.3",
        "7.3",
    ];
    for (idx, m) in all[..42].iter().enumerate() {
        let n = idx + 1;
        assert_eq!(m.serial(), Some(n as u32));
        assert_eq!(m.event_id(), Some("20261016123810"));
        assert_eq!(m.header.status, Some(Status::Normal));
        assert_eq!(m.header.info_type, Some(InfoType::Publication));
        assert!(!m.is_canceled());
        assert_eq!(m.is_last_info(), n == 42);
        assert_eq!(m.is_warning(), Some(n >= 17));
        let eq = m.body.earthquake.as_ref().unwrap();
        assert_eq!(
            eq.magnitude.as_ref().unwrap().value.as_deref(),
            Some(if n < 15 { magnitudes[idx] } else { "9.9" })
        );
        assert_eq!(m.body.intensity.is_none(), n == 4);
        if n >= 17 {
            let intensity = m.body.intensity.as_ref().unwrap();
            let regions = intensity.regions.as_ref().unwrap();
            assert_eq!(regions.len(), 105);
            assert_eq!(
                intensity.forecast_max_lg_int.as_ref().unwrap().from,
                LgIntensityValue::Four
            );
            assert!(regions.iter().all(|r| r.forecast_max_lg_int.is_some()));
        }
    }
    assert_eq!(all[2].message_time(), all[3].message_time());
    assert_ne!(all[2].serial(), all[3].serial());
    assert_eq!(all[10].message_time(), all[11].message_time());
    assert_eq!(all[0].message_time(), Some("2026-10-16T12:38:27+09:00"));
    assert_eq!(all[36].message_time(), Some("2026-10-16T12:46:41+09:00"));
    assert_eq!(all[41].message_time(), Some("2026-10-16T12:48:10+09:00"));
    let cancel = &all[42];
    assert_eq!(cancel.serial(), Some(37));
    assert_eq!(cancel.message_time(), Some("2026-10-16T12:48:50+09:00"));
    assert!(cancel.is_last_info());
    assert!(cancel.is_canceled());
    assert_eq!(cancel.is_warning(), None);
    assert!(cancel.body.earthquake.is_none());
    assert!(cancel.body.intensity.is_none());
    assert_eq!(cancel.header.info_type, Some(InfoType::Cancellation));
}
#[test]
fn full_observed_sequence_tracker_accepts_same_second_distinct_serials_and_late_cancel() {
    let all = observed();
    let mut tracker = EventTracker::new();
    for (i, m) in all.iter().enumerate() {
        assert_eq!(
            tracker.observe(m),
            match i {
                41 => EventDecision::Final,
                42 => EventDecision::Cancelled { existed: true },
                _ => EventDecision::Accepted,
            },
            "report {}",
            i + 1
        );
    }
    let key = EventTracker::key(&all[42]).unwrap();
    let state = tracker.state(&key).unwrap();
    assert_eq!(state.latest_serial, Some(42));
    assert_eq!(state.cancellation_serial, Some(37));
    assert!(state.is_final);
    assert!(state.is_cancelled);
    assert_eq!(state.lifecycle, Lifecycle::Canceled);
    assert_eq!(tracker.observe(&all[42]), EventDecision::Duplicate);
    assert_eq!(tracker.observe(&all[36]), EventDecision::AfterCancellation);
}
#[test]
fn actual_cancel_telegram_and_observed_json_share_lifecycle_identity() {
    let all = observed();
    let cancel=parse_telegram(b"39 03 10 261016124850 C11 261016123810 ND20261016123810 NCN037 JD////////////// JN/// /// //// ///// /// // // RK///// RT///// RC///// 9999=").unwrap();
    let mut tracker = EventTracker::new();
    assert_eq!(tracker.observe(&all[36]), EventDecision::Accepted);
    assert_eq!(tracker.observe(&all[41]), EventDecision::Final);
    assert_eq!(
        tracker.observe(&cancel),
        EventDecision::Cancelled { existed: true }
    );
    assert_eq!(
        tracker
            .state(&EventTracker::key(&cancel).unwrap())
            .unwrap()
            .latest_serial,
        Some(42)
    );
    assert_eq!(cancel.message_time(), all[42].message_time());
    assert_eq!(cancel.serial(), all[42].serial());
    assert_eq!(cancel.is_last_info(), all[42].is_last_info());
    assert_eq!(cancel.is_canceled(), all[42].is_canceled());
}
#[test]
fn different_products_and_training_do_not_cancel_each_other() {
    let all = observed();
    let mut warning = all[36].clone();
    warning.product = Product::Warning;
    let mut training = all[36].clone();
    training.header.status = Some(Status::Training);
    let mut tracker = EventTracker::new();
    for m in [&all[36], &warning, &training] {
        assert_eq!(tracker.observe(m), EventDecision::Accepted);
    }
    assert_eq!(tracker.len(), 3);
    assert_eq!(
        tracker.observe(&all[42]),
        EventDecision::Cancelled { existed: true }
    );
    assert!(
        !tracker
            .state(&EventTracker::key(&warning).unwrap())
            .unwrap()
            .is_cancelled
    );
    assert!(
        !tracker
            .state(&EventTracker::key(&training).unwrap())
            .unwrap()
            .is_cancelled
    );
}
#[test]
fn stale_cancellation_does_not_cancel_newer_reports() {
    let all = observed();
    let mut old_cancel = all[42].clone();
    old_cancel.header.report_date_time = all[0].header.report_date_time.clone();
    old_cancel.header.press_date_time = all[0].header.press_date_time.clone();
    let mut tracker = EventTracker::new();
    tracker.observe(&all[41]);
    assert_eq!(tracker.observe(&old_cancel), EventDecision::OlderThanLatest);
    assert!(
        !tracker
            .state(&EventTracker::key(&all[41]).unwrap())
            .unwrap()
            .is_cancelled
    );
}
#[test]
fn corrections_to_latest_serial_are_accepted_without_reviving_canceled_series() {
    let all = observed();
    let mut correction = all[36].clone();
    correction.header.info_type = Some(InfoType::Correction);
    correction.header.press_date_time = Some("2026-10-16T03:46:42Z".into());
    let mut tracker = EventTracker::new();
    tracker.observe(&all[36]);
    assert_eq!(tracker.observe(&correction), EventDecision::Accepted);
    assert_eq!(tracker.observe(&correction), EventDecision::Duplicate);
    tracker.observe(&all[42]);
    correction.header.press_date_time = Some("2026-10-16T03:49:00Z".into());
    assert_eq!(
        tracker.observe(&correction),
        EventDecision::CorrectionAfterCancellation
    );
    assert!(
        tracker
            .state(&EventTracker::key(&correction).unwrap())
            .unwrap()
            .is_cancelled
    );
}
#[test]
fn final_marker_is_not_a_permanent_event_lock() {
    let all = observed();
    let mut tracker = EventTracker::new();
    tracker.observe(&all[41]);
    let mut later = all[41].clone();
    later.header.serial_no = Some("43".into());
    later.header.report_date_time = Some("2026-10-16T12:48:20+09:00".into());
    later.header.press_date_time = Some("2026-10-16T03:48:20Z".into());
    later.body.is_last_info = false;
    assert_eq!(tracker.observe(&later), EventDecision::Accepted);
    assert_eq!(
        tracker
            .state(&EventTracker::key(&later).unwrap())
            .unwrap()
            .lifecycle,
        Lifecycle::Active
    );
    assert_eq!(
        tracker.observe(&all[42]),
        EventDecision::Cancelled { existed: true }
    );
}
#[test]
fn unknown_serial_is_not_a_zero_watermark_and_retention_is_explicit() {
    let all = observed();
    let mut m = all[0].clone();
    m.header.serial_no = None;
    let mut tracker = EventTracker::new();
    assert_eq!(tracker.observe(&m), EventDecision::Accepted);
    assert_eq!(tracker.observe(&m), EventDecision::Duplicate);
    let key = EventTracker::key(&m).unwrap();
    assert_eq!(tracker.state(&key).unwrap().latest_serial, None);
    assert!(tracker.remove(&key).is_some());
    assert!(tracker.is_empty());
}

#[test]
fn synthetic_xml_for_every_observed_report_matches_all_representable_reference_fields() {
    for m in observed() {
        let name = format!(
            "{:02}-{}.xml",
            m.serial().unwrap(),
            if m.is_canceled() { "cancel" } else { "report" }
        );
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/synthetic/20261016123810")
            .join(&name);
        let parsed = parse_xml(&fs::read(path).unwrap()).unwrap();
        assert_eq!(parsed.header, m.header, "{name}");
        let mut actual = parsed.body;
        actual.next_advisory = None;
        assert_eq!(actual, m.body, "{name}");
    }
}

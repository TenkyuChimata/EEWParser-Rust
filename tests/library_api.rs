use eew_parser::*;
use serde_json::Value;
use std::{fs, path::Path, process::Command};
const CANCEL: &[u8] = b"39 03 10 261016124850 C11 261016123810 ND20261016123810 NCN037 JD////////////// JN/// /// //// ///// /// // // RK///// RT///// RC///// 9999=";
fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}
fn semantic_value(m: &EewInformation) -> Value {
    let mut value = serde_json::to_value(m).unwrap();
    let map = value.as_object_mut().unwrap();
    map.remove("product");
    map.remove("source");
    map.get_mut("body")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("nextAdvisory");
    value
}
fn reference(mut v: Value) -> Value {
    let m = v.as_object_mut().unwrap();
    m.remove("_schema");
    m.remove("_originalId");
    v
}
#[test]
fn matching_official_xml_and_dmdata_compare_every_supported_field() {
    for name in [
        "rti-first",
        "rti-last",
        "plum-first",
        "plum-warning-first",
        "100gal-first",
        "noto-first",
        "noto-last",
        "cancel-first",
        "cancel-last",
    ] {
        let xml = fixture(&format!("official/{name}.xml"));
        let expected: Value =
            serde_json::from_slice(&fixture(&format!("official/{name}.json"))).unwrap();
        let actual = parse_xml(&xml).unwrap();
        assert_eq!(semantic_value(&actual), reference(expected), "{name}");
        assert_eq!(actual.source.as_ref().unwrap().raw.as_bytes(), xml);
        assert!(actual.source.unwrap().telegram.is_none());
    }
}
#[test]
fn all_current_jma_eew_samples_parse_without_telegram_projection() {
    let mut count = 0;
    for entry in
        fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/official")).unwrap()
    {
        let p = entry.unwrap().path();
        let n = p.file_name().unwrap().to_str().unwrap();
        if !n.ends_with(".xml") || !n.contains("VXSE4") {
            continue;
        }
        let m = parse_xml(&fs::read(&p).unwrap()).unwrap_or_else(|e| panic!("{n}: {e}"));
        assert_ne!(m.product(), Product::Unknown, "{n}");
        count += 1;
        if m.product() == Product::Warning && !m.is_canceled() {
            assert!(!m.is_last_info());
        }
    }
    assert_eq!(count, 70);
}
#[test]
fn all_public_schema_references_round_trip_without_field_loss() {
    for entry in
        fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/official")).unwrap()
    {
        let p = entry.unwrap().path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") || p.ends_with("manifest.json") {
            continue;
        }
        let v: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
        let m: EewInformation = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(semantic_value(&m), reference(v), "{}", p.display());
    }
}
#[test]
fn test_product_matches_official_dmdata_reference() {
    let actual = parse_xml(&fixture("official/54_01_01_100514_VXSE42.xml")).unwrap();
    let expected: Value =
        serde_json::from_slice(&fixture("official/vxse42_rjtd_20090811050000.json")).unwrap();
    assert_eq!(semantic_value(&actual), reference(expected));
    assert_eq!(actual.is_training_or_test(), Some(true));
    assert!(!actual.is_last_info());
    assert_eq!(actual.is_warning(), None);
    let mut tracker = EventTracker::new();
    assert_eq!(tracker.observe(&actual), EventDecision::IgnoredTest);
    assert!(tracker.is_empty());
}
#[test]
fn direct_cancel_has_last_info_without_ncn9_or_invented_payload() {
    let m = parse_telegram(CANCEL).unwrap();
    assert_eq!(m.event_id(), Some("20261016123810"));
    assert_eq!(m.serial(), Some(37));
    assert!(m.is_last_info());
    assert!(m.is_canceled());
    assert!(!m.is_final());
    assert_eq!(m.is_warning(), None);
    assert_eq!(m.header.status, Some(Status::Normal));
    assert_eq!(m.header.info_type, Some(InfoType::Cancellation));
    assert_eq!(m.message_time(), Some("2026-10-16T12:48:50+09:00"));
    assert!(m.body.earthquake.is_none());
    assert!(m.body.intensity.is_none());
    assert!(m.header.report_date_time.is_none()); // code field is transmission time
    assert!(m.header.info_kind_version.is_none());
    assert!(m.header.publishing_office.is_none());
    let v = serde_json::to_value(m).unwrap();
    assert_eq!(v["body"]["isLastInfo"], true);
    assert!(v["body"].get("isWarning").is_none());
}
#[test]
fn library_byte_apis_share_one_structured_result() {
    let data = include_bytes!("../data.txt");
    let m = parse_telegram(data).unwrap();
    assert_eq!(parse_auto(data).unwrap(), m);
    assert_eq!(m.serial(), Some(46));
    assert!(m.is_final());
    assert!(m.is_last_info());
    assert_eq!(m.body.intensity.unwrap().regions.unwrap().len(), 45);
}
#[test]
fn xml_and_telegram_keep_source_specific_information() {
    let xml = parse_xml(&fixture("official_noto_first.xml")).unwrap();
    let tg = parse_telegram(&fixture("expected/official_noto_first.telegram")).unwrap();
    assert_eq!(xml.event_id(), tg.event_id());
    assert_eq!(xml.serial(), tg.serial());
    let x = xml.body.earthquake.unwrap();
    let t = tg.body.earthquake.unwrap();
    assert_eq!(x.hypocenter.area.code, t.hypocenter.area.code);
    assert_eq!(
        x.hypocenter.coordinate.latitude,
        t.hypocenter.coordinate.latitude
    );
    assert_eq!(x.magnitude.unwrap().value, t.magnitude.unwrap().value);
    assert!(x.arrival_time.is_some());
    assert!(t.arrival_time.is_none());
    assert!(xml.header.info_kind.is_some());
    assert!(tg.header.info_kind.is_none());
    assert!(xml.source.unwrap().telegram.is_none());
}
#[test]
fn correction_and_test_status_do_not_require_a_telegram_mapping() {
    let raw = String::from_utf8(fixture("official_noto_first.xml")).unwrap();
    for (old, new) in [
        ("<InfoType>発表</InfoType>", "<InfoType>訂正</InfoType>"),
        ("<Status>通常</Status>", "<Status>試験</Status>"),
        ("<Status>通常</Status>", "<Status>訓練</Status>"),
        ("<Epicenter rank=\"3\"", "<Epicenter rank=\"0\""),
    ] {
        assert!(parse_xml(raw.replace(old, new).as_bytes()).is_ok());
    }
}
#[test]
fn product_uses_control_identity_instead_of_headline_warning_words() {
    let raw = String::from_utf8(fixture("official_noto_first.xml")).unwrap();
    let cut = raw.find("<Head ").unwrap();
    let mut text = raw[..cut].to_owned();
    text.push_str(&raw[cut..].replacen(
        "<Title>緊急地震速報（地震動予報）</Title>",
        "<Title>警報を伴う予報</Title>",
        1,
    ));
    assert_eq!(
        parse_xml(text.as_bytes()).unwrap().product(),
        Product::GroundMotionForecast
    );
}
#[test]
fn missing_intensity_is_a_valid_forecast() {
    let raw = String::from_utf8(fixture("official_noto_first.xml")).unwrap();
    let a = raw.find("<Intensity>").unwrap();
    let b = raw.find("</Intensity>").unwrap() + "</Intensity>".len();
    let raw = format!("{}{}", &raw[..a], &raw[b..]);
    let m = parse_xml(raw.as_bytes()).unwrap();
    assert!(m.body.intensity.is_none());
}
#[test]
fn unknown_xml_quantities_remain_null_and_coordinate_absent() {
    let raw = String::from_utf8(fixture("official/100gal-first.xml")).unwrap();
    let raw = raw.replace("+29.8+129.9-10000/", "");
    let m = parse_xml(raw.as_bytes()).unwrap();
    let eq = m.body.earthquake.unwrap();
    assert!(eq.hypocenter.coordinate.latitude.is_none());
    assert!(eq.hypocenter.coordinate.longitude.is_none());
    assert!(eq.hypocenter.depth.unwrap().value.is_none());
    assert!(eq.magnitude.unwrap().value.is_none());
    let legacy = decode_telegram_legacy(CANCEL).unwrap();
    assert!(legacy.Magnitude.is_none());
    assert!(legacy.ShinouLat.is_none());
}
#[test]
fn xml_does_not_drop_low_intensity_lpgm_regions_or_future_codes() {
    let raw = String::from_utf8(fixture("official_noto_first.xml")).unwrap();
    let area = "<Pref><Name>合成</Name><Code>9999</Code><Area><Name>LPGMのみ</Name><Code>999</Code><Category><Kind><Name>将来拡張</Name><Code>29</Code></Kind></Category><ForecastInt><From>1</From><To>2</To></ForecastInt><ForecastLgInt><From>3</From><To>over</To></ForecastLgInt></Area></Pref>";
    let raw = raw.replace("</Forecast>", &format!("{area}</Forecast>"));
    let m = parse_xml(raw.as_bytes()).unwrap();
    let i = m.body.intensity.unwrap();
    let r = i.regions.unwrap().pop().unwrap();
    assert_eq!(r.area.code, "999");
    assert_eq!(r.forecast_max_lg_int.unwrap().to, LgIntensityValue::Over);
    assert_eq!(r.is_warning, None);
    assert_eq!(r.is_plum, None);
    assert_eq!(r.kind.code, "29");
    assert!(m.source.unwrap().raw.contains("将来拡張"));
}
#[test]
fn serial_encoding_and_unset() {
    for (s, n) in [
        ("01", 1),
        ("99", 99),
        ("A0", 100),
        ("A9", 109),
        ("B0", 110),
        ("Z9", 359),
    ] {
        assert_eq!(decode_serial(s).unwrap(), Some(n));
    }
    assert_eq!(decode_serial("//").unwrap(), None);
    for s in ["00", "A/", "a0", "000", "Ａ0"] {
        assert!(decode_serial(s).is_err());
    }
    let raw = String::from_utf8(CANCEL.to_vec()).unwrap();
    assert_eq!(
        parse_telegram(raw.replace("NCN037", "NCN0A0").as_bytes())
            .unwrap()
            .serial(),
        Some(100)
    );
}
#[test]
fn warning_cancellation_has_independent_identity_and_no_earthquake() {
    let raw=b"48 03 10 061004150300 C11 061004145930 ND20061004145955 NCPNA0 9762 N336 E1362 040 PRC///// CAI //// CPI //// CBI /// PAI //// PPI //// PBI /// 9999=";
    let m = parse_telegram(raw).unwrap();
    assert_eq!(m.product(), Product::Warning);
    assert_eq!(m.serial(), Some(100));
    assert!(m.is_last_info());
    assert!(m.is_canceled());
    assert!(!m.is_final());
    assert!(m.body.earthquake.is_none());
}
#[test]
fn warning_ncp_uses_official_layout_and_preserves_all_lists() {
    let m = parse_telegram(&fixture("expected/official_plum_warning.telegram")).unwrap();
    let codes = m.source.unwrap().telegram.unwrap();
    assert_eq!(codes.ncpn.as_deref(), Some("01"));
    assert_eq!(codes.ncn.as_deref(), Some("0//"));
    assert_eq!(codes.pai, ["9934"]);
    assert_eq!(codes.ppi, ["9170"]);
    assert_eq!(codes.pbi, ["390"]);
    assert_eq!(
        m.body.zones.unwrap()[0]
            .kind
            .last_kind
            .as_ref()
            .unwrap()
            .code,
        "00"
    );
    assert_eq!(
        m.body.regions.unwrap()[0].area.name.as_deref(),
        Some("石川県能登")
    );
}
#[test]
fn ebi_ranges_are_encoded_upper_then_lower_and_times_have_no_invented_date() {
    let raw = "37 03 00 261016235959 C11 261016235900 ND20261016235900 NCN001 JD////////////// JN/// 912 N482 E1534 140 99 07 RK44554 RT11/// RC00/// EBI 165 S075- 000005 10 166 S6-// 235958 09 167 S6+5+ ////// 11 9999=";
    let m = parse_telegram(raw.as_bytes()).unwrap();
    let rs = m.body.intensity.unwrap().regions.unwrap();
    assert_eq!(
        rs[0].forecast_max_int.as_ref().unwrap().from,
        IntensityValue::FiveLower
    );
    assert_eq!(
        rs[0].forecast_max_int.as_ref().unwrap().to,
        IntensityValue::Seven
    );
    assert!(rs[0].arrival_time.is_none());
    assert_eq!(rs[0].arrival_time_of_day.as_deref(), Some("000005"));
    assert_eq!(
        rs[1].forecast_max_int.as_ref().unwrap().to,
        IntensityValue::Over
    );
    assert_eq!(rs[1].is_plum, Some(true));
    assert_eq!(rs[2].condition.as_deref(), Some("既に主要動到達と推測"));
}
#[test]
fn eip_includes_negative_values_and_unset_without_zero_substitution() {
    let raw = "61 04 00 160414212642 C11 160414212634 ND20160414212659 NCN005 JD////////////// JN/// 741 N327 E1308 010 65 6+ RK22422 RT01/// RC02/// EIP 4310300 P56 4321300 P48 43201AA P-1 431201B0 PA4 9999999 P// 9999998 PH0 9999=";
    let m = parse_telegram(raw.as_bytes()).unwrap();
    assert_eq!(m.product(), Product::RealTimeIntensity);
    assert_eq!(m.is_warning(), None);
    let s = m.body.intensity.unwrap().stations.unwrap();
    assert_eq!(s.len(), 6);
    assert_eq!(s[0].k.as_deref(), Some("5.6"));
    assert_eq!(s[0].int, IntensityValue::SixLower);
    assert_eq!(s[1].int, IntensityValue::FiveLower);
    assert_eq!(s[2].k.as_deref(), Some("-0.1"));
    assert_eq!(s[3].k.as_deref(), Some("-1.4"));
    assert_eq!(s[4].k, None);
    assert_eq!(s[5].k.as_deref(), Some("-8.0"));
}
#[test]
fn code61_cancellation_and_code38_test_have_correct_flags() {
    let raw = String::from_utf8(CANCEL.to_vec())
        .unwrap()
        .replacen("39 ", "61 ", 1);
    assert!(parse_telegram(raw.as_bytes()).unwrap().is_canceled());
    let m = parse_telegram(b"38 03 20 261016120000 C11 9999=").unwrap();
    assert_eq!(m.product(), Product::Test);
    assert!(!m.is_last_info());
    assert!(m.body.earthquake.is_none());
    assert_eq!(m.is_warning(), None);
}
#[test]
fn malicious_and_incomplete_inputs_fail_explicitly() {
    for data in [
        b"".as_slice(),
        b"37 03",
        b"37 03 00 261016123827 C11 9999=",
        b"<Report/>",
        b"\xff",
    ] {
        assert!(parse_auto(data).is_err());
    }
    let excessive_tokens = "0 ".repeat(65_537) + "9999=";
    assert!(
        matches!(parse_telegram(excessive_tokens.as_bytes()), Err(EewError::InvalidTelegram(message)) if message.contains("tokens"))
    );
    let big = vec![b' '; MAX_INPUT_BYTES + 1];
    assert!(matches!(
        parse_auto(&big),
        Err(EewError::InputTooLarge { .. })
    ));
    let raw = String::from_utf8(CANCEL.to_vec()).unwrap();
    for (a, b) in [
        ("C11", "C21"),
        ("261016124850", "261016126099"),
        ("ND20261016123810", "ND20261016ABCDEF"),
    ] {
        assert!(parse_telegram(raw.replace(a, b).as_bytes()).is_err());
    }
    let xml=b"<!DOCTYPE Report [<!ENTITY x 'boom'>]><Report xmlns='http://xml.kishou.go.jp/jmaxml1/'>&x;</Report>";
    assert!(parse_xml(xml).is_err());
}
#[test]
fn fmt_detection_whitespace_bom_and_telegram_terminal() {
    let mut xml = b"\xef\xbb\xbf \n".to_vec();
    xml.extend(fixture("official_noto_first.xml"));
    assert!(parse_auto(&xml).is_ok());
    let mut t = b"\xef\xbb\xbf \n".to_vec();
    t.extend(CANCEL);
    t.extend(b"next transport frame");
    assert!(parse_auto(&t).unwrap().is_canceled());
    assert!(matches!(
        parse_telegram(b"<Report/>"),
        Err(EewError::InputFormatConflict { .. })
    ));
}
#[test]
fn cli_emits_cancellations_in_the_same_schema_by_default() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/official_plum_cancellation.xml");
    let out = Command::new(env!("CARGO_BIN_EXE_EEWParser-Rust"))
        .arg("-p")
        .arg(path)
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["body"]["isLastInfo"], true);
    assert_eq!(v["body"]["isCanceled"], true);
    assert!(v["body"].get("earthquake").is_none());
}

#[test]
fn all_eight_supported_code_types_accept_official_manual_examples() {
    for code in ["35", "36", "37", "38", "39", "47", "48", "61"] {
        let m = parse_telegram(&fixture(&format!(
            "official_telegram/manual-{code}.telegram"
        )))
        .unwrap_or_else(|e| panic!("{code}: {e}"));
        assert_eq!(
            m.source
                .as_ref()
                .unwrap()
                .telegram
                .as_ref()
                .unwrap()
                .code_type,
            code
        );
        if code == "38" {
            assert!(m.body.text.unwrap().contains("テスト電文"));
        }
        if code == "36" {
            assert_eq!(
                m.body.earthquake.unwrap().magnitude.unwrap().bound,
                Some(QuantityBound::Lower)
            );
            assert_eq!(
                m.body.intensity.unwrap().forecast_max_int.unwrap().to,
                IntensityValue::Over
            );
        }
    }
}
#[test]
fn intensity_and_lpgm_enum_variants_round_trip_and_keep_unknown_codes() {
    for value in [
        "0", "1", "2", "3", "4", "5-", "5+", "6-", "6+", "7", "over", "不明", "future",
    ] {
        let n = IntensityValue::from_text(value);
        assert_eq!(serde_json::to_value(&n).unwrap(), value);
        assert_eq!(
            serde_json::from_value::<IntensityValue>(serde_json::json!(value)).unwrap(),
            n
        );
    }
    for value in ["0", "1", "2", "3", "4", "over", "不明", "future"] {
        assert_eq!(
            serde_json::to_value(LgIntensityValue::from_text(value)).unwrap(),
            value
        );
    }
}
#[test]
fn depth_special_values_and_real_zero_coordinates_are_distinct_from_unknown() {
    let base = String::from_utf8(fixture("official/100gal-first.xml")).unwrap();
    for (coordinate, depth, condition) in [
        ("+0.0+0.0+0/", Some("0"), Some("ごく浅い")),
        ("+0.0+0.0-700000/", Some("700"), Some("７００ｋｍ以上")),
        ("+0.0+0.0/", None, Some("不明")),
    ] {
        let m = parse_xml(base.replace("+29.8+129.9-10000/", coordinate).as_bytes()).unwrap();
        let h = m.body.earthquake.unwrap().hypocenter;
        assert_eq!(h.coordinate.latitude.unwrap().value, "0.0000");
        let d = h.depth.unwrap();
        assert_eq!(d.value.as_deref(), depth);
        assert_eq!(d.condition.as_deref(), condition);
    }
    let source = "37 03 00 261016120000 C11 261016115959 ND20261016115959 NCN001 JD////////////// JN/// 912 N000 E0000 000 00 01 RK00000 RT00/// RC00/// 9999=";
    let eq = parse_telegram(source.as_bytes())
        .unwrap()
        .body
        .earthquake
        .unwrap();
    assert_eq!(eq.magnitude.unwrap().value.as_deref(), Some("0.0"));
    assert_eq!(eq.hypocenter.depth.unwrap().value.as_deref(), Some("0"));
    assert_eq!(eq.hypocenter.coordinate.latitude.unwrap().value, "0.0000");
    assert_eq!(
        eq.hypocenter.accuracy.unwrap().epicenters,
        [Some("0".into()), Some("0".into())]
    );
}
#[test]
fn final_and_canceled_flags_obey_product_rules_in_xml_and_telegram() {
    let first = String::from_utf8(fixture("official/plum-warning-first.xml")).unwrap();
    let tagged = first.replace(
        "</Body>",
        "<NextAdvisory>この情報をもって、緊急地震速報：最終報とします。</NextAdvisory></Body>",
    );
    let warning = parse_xml(tagged.as_bytes()).unwrap();
    assert!(warning.is_final());
    assert!(!warning.is_last_info());
    assert!(!warning.is_canceled());
    let nested = String::from_utf8(fixture("expected/official_plum_warning.telegram"))
        .unwrap()
        .replace("NCN0//", "NCN9//");
    let m = parse_telegram(nested.as_bytes()).unwrap();
    assert!(!m.is_final());
    assert!(!m.is_last_info());
    let source = String::from_utf8(fixture("official_plum_cancellation.xml"))
        .unwrap()
        .replace("緊急地震速報（地震動予報）", "緊急地震速報（警報）");
    let m = parse_xml(source.as_bytes()).unwrap();
    assert_eq!(m.product(), Product::Warning);
    assert!(m.is_last_info());
    assert!(m.is_canceled());
    assert!(!m.is_final());
}
#[test]
fn historical_accuracy_ranks_are_retained_across_the_2023_protocol_change() {
    let base = "37 03 00 230926135959 C11 230926135950 ND20230926135950 NCN001 JD////////////// JN/// 912 N482 E1534 140 99 07 RK66259 RT11/// RC00/// 9999=";
    let before = decode_telegram_legacy(base.as_bytes()).unwrap();
    let after =
        decode_telegram_legacy(base.replace("230926135959", "230926140000").as_bytes()).unwrap();
    assert!(before.Rk3.contains("防災科研"));
    assert_eq!(after.Rk3, "速度マグニチュード");
    let m = parse_telegram(base.as_bytes()).unwrap();
    let a = m
        .body
        .earthquake
        .as_ref()
        .unwrap()
        .hypocenter
        .accuracy
        .as_ref()
        .unwrap();
    assert_eq!(a.epicenters, [Some("6".into()), Some("9".into())]);
    assert_eq!(a.magnitude_calculation.as_deref(), Some("2"));
    assert!(!m.is_final());
}
#[test]
fn namespace_aliases_and_all_supported_info_kind_versions_are_accepted() {
    let base = String::from_utf8(fixture("official_noto_first.xml")).unwrap();
    for version in ["1.0_0", "1.1_0", "1.2_0"] {
        let raw = base
            .replace("1.2_0", version)
            .replace("jmx_eb:", "element:")
            .replace("xmlns:jmx_eb", "xmlns:element");
        let m = parse_xml(raw.as_bytes()).unwrap();
        assert_eq!(m.header.info_kind_version.as_deref(), Some(version));
    }
    assert!(matches!(
        parse_xml(base.replace("1.2_0", "9.9_9").as_bytes()),
        Err(EewError::UnsupportedXml(_))
    ));
}
#[test]
fn malformed_bounds_missing_magnitude_and_mismatched_nested_ids_fail() {
    let base = String::from_utf8(fixture("official_noto_first.xml")).unwrap();
    let missing_type = base.replace("type=\"Mj\"", "");
    assert_ne!(missing_type, base);
    assert!(matches!(
        parse_xml(missing_type.as_bytes()),
        Err(EewError::MissingXmlField(_))
    ));
    let mut raw = base.clone();
    let start = raw.find("<jmx_eb:Magnitude").unwrap();
    let end = raw.find("</jmx_eb:Magnitude>").unwrap() + "</jmx_eb:Magnitude>".len();
    raw.replace_range(start..end, "");
    assert!(parse_xml(raw.as_bytes()).is_err());
    let start = base.find("<ForecastInt>").unwrap();
    let end = base[start..].find("</ForecastInt>").unwrap() + start + "</ForecastInt>".len();
    let bad = format!(
        "{}<ForecastInt><From>7</From><To>1</To></ForecastInt>{}",
        &base[..start],
        &base[end..]
    );
    assert!(parse_xml(bad.as_bytes()).is_err());
    let raw = String::from_utf8(fixture("expected/official_plum_warning.telegram")).unwrap();
    let pos = raw.find("NCP ND").unwrap();
    let wrong = format!(
        "{}{}",
        &raw[..pos],
        raw[pos..].replace("ND20230505215718", "ND20230505215719")
    );
    assert!(parse_telegram(wrong.as_bytes()).is_err());
}
#[test]
fn warning_encoder_handles_the_full_official_ncpn_serial_range() {
    let source = String::from_utf8(fixture("official/plum-warning-first.xml")).unwrap();
    for (number, code) in [(100, "A0"), (359, "Z9")] {
        let raw = source
            .replace("<Serial>1</Serial>", &format!("<Serial>{number}</Serial>"))
            .replace("<Code>00</Code>", "<Code>31</Code>");
        let generated = encode_xml_as_telegram(raw.as_bytes()).unwrap();
        assert!(generated.contains(&format!("NCPN{code}")));
        assert_eq!(
            parse_telegram(generated.as_bytes()).unwrap().serial(),
            Some(number)
        );
    }
}
#[test]
fn reference_or_test_message_type_is_not_a_fabricated_xml_status() {
    let raw = String::from_utf8(fixture("expected/official_noto_first.telegram"))
        .unwrap()
        .replacen("03 00", "03 20", 1);
    let m = parse_telegram(raw.as_bytes()).unwrap();
    assert!(m.header.status.is_none());
    assert_eq!(m.is_training_or_test(), None);
    assert_eq!(
        EventTracker::new().observe(&m),
        EventDecision::MissingIdentity
    );
}

#[test]
fn historical_ncn_correction_semantics_require_explicit_protocol_evidence() {
    let base = "37 03 00 130401120000 C11 130401115950 ND20130401115950 NCN601 JD////////////// JN/// 486 N343 E1384 010 55 5- RK224// RT01000 RC00000 9999=";
    assert!(parse_telegram(base.as_bytes()).is_err());
    let correction =
        parse_telegram_with_convention(base.as_bytes(), NcnConvention::Legacy2013).unwrap();
    assert_eq!(correction.header.info_type, Some(InfoType::Correction));
    assert!(!correction.is_final());
    let last = parse_telegram_with_convention(
        base.replace("NCN601", "NCN801").as_bytes(),
        NcnConvention::Legacy2013,
    )
    .unwrap();
    assert!(last.is_final());
    assert!(last.is_last_info());
    assert!(!last.is_canceled());
    let cancel = "39 03 10 130401120001 C11 130401115950 ND20130401115950 NCN001 JD////////////// JN/// /// //// ///// /// // // RK///// RT///// RC///// 9999=";
    let mut tracker = EventTracker::new();
    tracker.observe(&correction);
    tracker.observe(&parse_telegram(cancel.as_bytes()).unwrap());
    let restored = parse_telegram_with_convention(
        base.replace("NCN601", "NCN701")
            .replace("130401120000", "130401120002")
            .as_bytes(),
        NcnConvention::Legacy2013,
    )
    .unwrap();
    assert_eq!(tracker.observe(&restored), EventDecision::Reinstated);
    assert!(
        !tracker
            .state(&EventTracker::key(&restored).unwrap())
            .unwrap()
            .is_cancelled
    );
}

#[test]
fn unknown_warning_kind_is_preserved_and_does_not_invent_a_negative_flag() {
    let base = String::from_utf8(fixture("official/100gal-first.xml")).unwrap();
    let extra = "<Information type=\"緊急地震速報（細分区域）\"><Item><Kind><Code>32</Code><Name>将来の警報種別</Name></Kind><LastKind><Code>00</Code><Name>なし</Name></LastKind><Areas codeType=\"地震情報／細分区域\"><Area><Name>合成</Name><Code>999</Code></Area></Areas></Item></Information>";
    let raw = base.replace("</Headline>", &format!("{extra}</Headline>"));
    let m = parse_xml(raw.as_bytes()).unwrap();
    assert_eq!(m.is_warning(), None);
    assert_eq!(m.body.regions.unwrap()[0].kind.current.code, "32");
}

#[test]
fn complete_optional_header_and_text_fields_preserve_their_values() {
    let base = String::from_utf8(fixture("official/plum-warning-first.xml")).unwrap();
    let raw=base.replace("</TargetDateTime>","</TargetDateTime><TargetDTDubious>頃</TargetDTDubious><TargetDuration>PT1H</TargetDuration><ValidDateTime>2023-05-05T22:58:16+09:00</ValidDateTime>").replace("<Code>0201</Code>","<Code>0201 0203</Code>").replace("</Comments>","<FreeFormComment>追加情報 &amp; 原文</FreeFormComment></Comments>").replace("<Serial>1</Serial>","<Serial>001</Serial>");
    let m = parse_xml(raw.as_bytes()).unwrap();
    assert_eq!(m.header.target_date_time_dubious.as_deref(), Some("頃"));
    assert_eq!(m.header.target_duration.as_deref(), Some("PT1H"));
    assert_eq!(
        m.header.valid_date_time.as_deref(),
        Some("2023-05-05T22:58:16+09:00")
    );
    assert_eq!(m.serial(), Some(1));
    assert_eq!(m.header.serial_no.as_deref(), Some("001"));
    let c = m.body.comments.unwrap();
    assert_eq!(c.free.as_deref(), Some("追加情報 & 原文"));
    assert_eq!(c.warning.unwrap().codes, ["0201", "0203"]);
}

#[test]
fn unknown_lifecycle_markers_return_errors_instead_of_inventing_flags() {
    let base = String::from_utf8(fixture("official_noto_first.xml")).unwrap();
    let unknown = base.replace("<InfoType>発表</InfoType>", "<InfoType>将来状態</InfoType>");
    assert!(matches!(
        parse_xml(unknown.as_bytes()),
        Err(EewError::UnsupportedXml(_))
    ));
    let unknown = base.replace(
        "</Body>",
        "<NextAdvisory>未定義の状態</NextAdvisory></Body>",
    );
    assert!(matches!(
        parse_xml(unknown.as_bytes()),
        Err(EewError::UnsupportedXml(_))
    ));
}

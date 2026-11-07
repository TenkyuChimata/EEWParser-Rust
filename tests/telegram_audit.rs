//! Independent code-Telegram checks against JMA no40202, February 2023:
//! basic-2 (9999 versus '='), earthquake-4/24 (NCN/NCPN), earthquake-8
//! (independent EBI flags and PLUM time), earthquake-33/34 (EIP).
use eew_parser::{
    InfoType, NcnConvention, decode_telegram_legacy, parse_telegram, parse_telegram_with_convention,
};

const FORECAST: &str = "37 03 00 261106235959 C11 261106235950 ND20261106235950 NCN001 JD////////////// JN/// 741 N328 E1307 010 55 5- RK44554 RT01/// RC00/// EBI 741 S5-04 000005 10 9999=";
const WARNING: &str = include_str!("fixtures/official_telegram/manual-47.telegram");
const REALTIME: &str = include_str!("fixtures/official_telegram/manual-61.telegram");

#[test]
fn code_and_message_terminators_are_separate_in_every_list_path() {
    for source in [
        FORECAST,
        WARNING,
        REALTIME,
        include_str!("fixtures/official_telegram/manual-48.telegram"),
    ] {
        let expected = parse_telegram(source.as_bytes()).unwrap();
        for suffix in ["9999\n=", "9999\n追加の解説文\n="] {
            let input = source.replace("9999=", suffix);
            let parsed = parse_telegram(input.as_bytes()).unwrap();
            assert_eq!(parsed.header, expected.header);
            assert_eq!(parsed.body.earthquake, expected.body.earthquake);
            assert_eq!(parsed.body.intensity, expected.body.intensity);
            assert_eq!(
                parsed.body.text.as_deref(),
                suffix.contains("追加").then_some("追加の解説文")
            );
            assert!(decode_telegram_legacy(input.as_bytes()).is_ok());
        }
    }
}

#[test]
fn plum_time_is_prediction_time_without_an_invented_calendar_date() {
    let input = FORECAST.replace("000005 10", "000005 19");
    let parsed = parse_telegram(input.as_bytes()).unwrap();
    let area = &parsed
        .body
        .intensity
        .as_ref()
        .unwrap()
        .regions
        .as_ref()
        .unwrap()[0];
    assert_eq!(area.is_plum, Some(true));
    assert!(area.arrival_time.is_none());
    assert!(area.arrival_time_of_day.is_none());
    assert_eq!(area.plum_prediction_time_of_day.as_deref(), Some("000005"));
    assert_eq!(
        parsed.source.unwrap().telegram.unwrap().ebi_times,
        ["000005"]
    );
}

#[test]
fn ebi_flags_preserve_independent_known_and_unknown_information() {
    for (flags, warning, plum) in [
        ("1/", Some(true), None),
        ("29", None, Some(true)),
        ("/0", None, Some(false)),
        ("/1", None, Some(false)),
        ("//", None, None),
    ] {
        let input = FORECAST.replace("000005 10", &format!("000005 {flags}"));
        let parsed = parse_telegram(input.as_bytes()).unwrap();
        let area = &parsed.body.intensity.unwrap().regions.unwrap()[0];
        assert_eq!(area.is_warning, warning, "{flags}");
        assert_eq!(area.is_plum, plum, "{flags}");
        assert_eq!(area.arrival_time_of_day.is_some(), flags.ends_with('0'));
        assert_eq!(
            area.plum_prediction_time_of_day.is_some(),
            flags.ends_with('9')
        );
    }
}

#[test]
fn invalid_numeric_lexemes_are_rejected_by_both_public_decoders() {
    for (from, to) in [
        ("37 03 00", "37 XX 00"),
        ("261106235959", "261106250000"),
        ("261106235950", "261132235950"),
        ("ND20261106235950", "ND2026110623595X"),
        ("NCN001", "NCN000"),
        ("NCN001", "NCN0a0"),
        ("741 N328", "ABC N328"),
        ("N328", "N+28"),
        ("E1307", "E+307"),
        ("010 55", "010 +5"),
        ("55 5-", "55 00"),
        ("EBI 741", "EBI 7/1"),
        ("S5-04", "SXX04"),
        ("000005 10", "240005 10"),
        ("000005 10", "000005 1X"),
    ] {
        let input = FORECAST.replacen(from, to, 1);
        assert_ne!(input, FORECAST, "mutation {from}");
        assert!(
            parse_telegram(input.as_bytes()).is_err(),
            "semantic accepted {to}"
        );
        assert!(
            decode_telegram_legacy(input.as_bytes()).is_err(),
            "legacy accepted {to}"
        );
    }
}

#[test]
fn nested_ncn_and_outer_ncpn_are_validated_independently() {
    for bad in ["NCN000", "NCN0a0", "NCN0/1"] {
        let input = WARNING.replace("NCN001", bad);
        assert!(parse_telegram(input.as_bytes()).is_err(), "{bad}");
        assert!(decode_telegram_legacy(input.as_bytes()).is_err(), "{bad}");
    }
    let unset_warning = WARNING.replace("NCPN01", "NCPN//");
    assert!(parse_telegram(unset_warning.as_bytes()).is_err());
    let unset_nested = WARNING.replace("NCN001", "NCN0//");
    assert_eq!(
        parse_telegram(unset_nested.as_bytes()).unwrap().serial(),
        Some(1)
    );
    let expanded_nested = WARNING.replace("NCN001", "NCN0A0");
    assert!(parse_telegram(expanded_nested.as_bytes()).is_ok());
    assert!(
        parse_telegram_with_convention(expanded_nested.as_bytes(), NcnConvention::Legacy2013)
            .is_err()
    );
}

#[test]
fn nested_legacy_forecast_correction_is_not_a_warning_correction() {
    for status in ['6', '7', '8'] {
        let input = WARNING.replace("NCN001", &format!("NCN{status}01"));
        let parsed =
            parse_telegram_with_convention(input.as_bytes(), NcnConvention::Legacy2013).unwrap();
        assert_eq!(parsed.header.info_type, Some(InfoType::Publication));
        assert!(!parsed.is_final());
    }
}

#[test]
fn misplaced_empty_and_duplicate_observation_blocks_are_errors() {
    let base = FORECAST.replace("EBI 741 S5-04 000005 10 ", "");
    for input in [
        base.replace("9999=", "EBI 9999="),
        base.replace("9999=", "EIP 4310300 P56 9999="),
        FORECAST.replace("9999=", "741 S6+5+ 000010 10 9999="),
        base.replacen("37 ", "61 ", 1)
            .replace("9999=", "EBI 741 S5-04 000005 10 9999="),
        base.replacen("37 ", "61 ", 1).replace("9999=", "EIP 9999="),
        REALTIME.replace("9999=", "4310300 P57 9999="),
    ] {
        assert!(
            parse_telegram(input.as_bytes()).is_err(),
            "accepted {input}"
        );
    }
}

#[test]
fn historical_unsplit_intensity_has_no_invented_strong_weak_subdivision() {
    let sample = include_str!("fixtures/official_telegram/manual-37.telegram");
    let parsed = decode_telegram_legacy(sample.as_bytes()).unwrap();
    assert_eq!(parsed.Ebis.last().unwrap().Shindo1, "5");
}

#[test]
fn station_names_use_the_current_table_and_preserve_documented_retired_aliases() {
    // JMA jmaxml_20261104_Code.zip, earthquake workbook, sheet 25.
    let input = REALTIME.replace("4310300 P56", "3520601 P56 4420503 P56");
    let parsed = parse_telegram(input.as_bytes()).unwrap();
    let stations = parsed.body.intensity.unwrap().stations.unwrap();
    assert_eq!(stations[0].station.name.as_deref(), Some("防府市寿町"));
    assert_eq!(stations[1].station.code, "4420503");
    assert_eq!(stations[1].station.name.as_deref(), Some("佐伯蒲江"));
    assert_eq!(
        parsed.source.unwrap().code_table_version.as_deref(),
        Some("2026-11-04")
    );
}

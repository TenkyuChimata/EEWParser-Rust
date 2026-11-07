//! Explicit legacy projection contracts, including the narrow loss whitelist.
//! XML remains the authority for XML-only fields; these are generated telegrams.
use chrono::{DateTime, FixedOffset};
use eew_parser::*;
use std::{
    fs,
    path::{Path, PathBuf},
};

const NOTO: &str = include_str!("fixtures/official_noto_first.xml");
const GAL: &str = include_str!("fixtures/official_100gal_first.xml");
const PLUM: &str = include_str!("fixtures/production_20261106/20261106194044-02.xml");
const WARNING: &str = include_str!("fixtures/official/37_01_02_240613_VXSE43.xml");
fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}
fn change(raw: &str, old: &str, new: &str) -> String {
    assert!(raw.contains(old), "mutation did not match {old:?}");
    raw.replacen(old, new, 1)
}
fn remove(raw: &str, tag: &str) -> String {
    let doc = roxmltree::Document::parse(raw).unwrap();
    let range = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == tag)
        .unwrap()
        .range();
    format!("{}{}", &raw[..range.start], &raw[range.end..])
}

fn codes(m: &EewInformation) -> &TelegramCodes {
    m.source.as_ref().unwrap().telegram.as_ref().unwrap()
}
fn number(s: &str) -> f64 {
    s.parse().unwrap()
}
fn unknown_rank(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|s| *s != "0")
}
fn same_time(a: &str, b: &str) {
    assert_eq!(
        DateTime::parse_from_rfc3339(a).unwrap(),
        DateTime::parse_from_rfc3339(b).unwrap()
    );
}
fn check_format(t: &str) {
    assert!(t.is_ascii());
    assert!(t.ends_with("\n9999="));
    assert!(t.len() <= 3800);
    for line in t.lines() {
        assert!(line.len() <= 69, "overlong line: {line}");
    }
    // Independent field grammar, not just acceptance by our Telegram reader.
    let ts: Vec<_> = t.split_whitespace().collect();
    assert!(matches!(ts[0], "35" | "36" | "37" | "47"));
    assert_eq!(ts[4], "C11");
    assert_eq!(ts[3].len(), 12);
    for token in &ts {
        if let Some(rk) = token.strip_prefix("RK") {
            assert_eq!(rk.len(), 5);
            for (ch, allowed) in
                rk.chars()
                    .zip(["/12345678", "/12345678", "/234568", "/12345", "/12349"])
            {
                assert!(allowed.contains(ch), "invalid RK {rk}");
            }
        }
        if let Some(rt) = token.strip_prefix("RT") {
            assert_eq!(rt.len(), 5);
            for (ch, allowed) in rt.chars().zip(["/01", "/01", "/9", "/", "/"]) {
                assert!(allowed.contains(ch));
            }
        }
        if let Some(rc) = token.strip_prefix("RC") {
            assert_eq!(rc.len(), 5);
            for (ch, allowed) in rc.chars().zip(["/012", "/012349", "/", "/", "/"]) {
                assert!(allowed.contains(ch));
            }
        }
    }
}
fn compare_warning(a: &Option<Vec<WarningArea>>, b: &Option<Vec<WarningArea>>) {
    let values = |areas: &Option<Vec<WarningArea>>| {
        let mut v: Vec<_> = areas
            .iter()
            .flatten()
            .map(|w| {
                (
                    w.area.code.clone(),
                    w.kind.current.code.clone(),
                    w.kind.last_kind.as_ref().map(|k| k.code.clone()),
                )
            })
            .collect();
        v.sort();
        v
    };
    assert_eq!(values(a), values(b));
}
fn compare_projection(x: &EewInformation, t: &EewInformation) {
    assert_eq!(x.event_id(), t.event_id());
    assert_eq!(x.serial(), t.serial());
    assert_eq!(x.header.status, t.header.status);
    assert_eq!(x.header.info_type, t.header.info_type);
    assert_eq!(x.is_canceled(), t.is_canceled());
    assert_eq!(x.is_last_info(), t.is_last_info());
    assert_eq!(x.is_warning(), t.is_warning());
    assert_eq!(x.is_final(), t.is_final());
    assert_eq!(
        t.product(),
        if x.product() == Product::Warning {
            Product::Warning
        } else {
            Product::TelegramForecast
        }
    );
    same_time(
        x.header.press_date_time.as_ref().unwrap(),
        t.source.as_ref().unwrap().sent_at.as_ref().unwrap(),
    );
    let a = x.body.earthquake.as_ref().unwrap();
    let b = t.body.earthquake.as_ref().unwrap();
    if codes(t).code_type == "35" {
        assert!(a.origin_time.is_none() && b.origin_time.is_none());
        same_time(
            a.arrival_time.as_ref().unwrap(),
            b.arrival_time.as_ref().unwrap(),
        );
    } else {
        same_time(
            a.origin_time.as_ref().unwrap(),
            b.origin_time.as_ref().unwrap(),
        );
    }
    assert_eq!(a.condition, b.condition);
    assert_eq!(a.hypocenter.area.code, b.hypocenter.area.code);
    for (a, b) in [
        (
            &a.hypocenter.coordinate.latitude,
            &b.hypocenter.coordinate.latitude,
        ),
        (
            &a.hypocenter.coordinate.longitude,
            &b.hypocenter.coordinate.longitude,
        ),
    ] {
        assert_eq!(
            a.as_ref().map(|v| number(&v.value)),
            b.as_ref().map(|v| number(&v.value))
        );
    }
    assert_eq!(
        a.hypocenter.depth.as_ref().unwrap().value_f64(),
        b.hypocenter.depth.as_ref().unwrap().value_f64()
    );
    assert_eq!(a.hypocenter.land_or_sea, b.hypocenter.land_or_sea);
    let aa = a.hypocenter.accuracy.as_ref().unwrap();
    let ba = b.hypocenter.accuracy.as_ref().unwrap();
    for (a, b) in [
        (&aa.epicenters[0], &ba.epicenters[0]),
        (&aa.epicenters[1], &ba.epicenters[1]),
        (&aa.depth, &ba.depth),
        (&aa.magnitude_calculation, &ba.magnitude_calculation),
        (
            &aa.number_of_magnitude_calculation,
            &ba.number_of_magnitude_calculation,
        ),
    ] {
        assert_eq!(unknown_rank(a), b.as_deref());
    }
    let am = a.magnitude.as_ref().unwrap();
    let bm = b.magnitude.as_ref().unwrap();
    assert_eq!(am.value_f64(), bm.value_f64());
    // Whitelist: XML Mj has no bound attribute; code 36 defines an M lower
    // bound, except the PLUM M1.0 placeholder. It never becomes a measured M.
    assert_eq!(
        bm.bound,
        (am.value.is_some()
            && a.condition.is_none()
            && (codes(t).code_type == "36"
                || codes(t).code_type == "47"
                    && matches!(aa.epicenters[0].as_deref(), Some("1" | "2"))))
        .then_some(QuantityBound::Lower)
    );
    if x.product() == Product::Warning {
        compare_warning(&x.body.zones, &t.body.zones);
        compare_warning(&x.body.prefectures, &t.body.prefectures);
        compare_warning(&x.body.regions, &t.body.regions);
        assert_eq!(
            a.hypocenter.reduce.as_ref().unwrap().code,
            b.hypocenter.reduce.as_ref().unwrap().code
        );
        assert_eq!(codes(t).ncn.as_ref().unwrap(), "0//"); // XML does not carry forecast serial.
    }
    let xi = x.body.intensity.as_ref();
    let ti = t.body.intensity.as_ref();
    let xm = xi.and_then(|i| i.forecast_max_int.as_ref());
    let tm = ti.and_then(|i| i.forecast_max_int.as_ref());
    if let Some(r) = xm {
        if matches!(r.from, IntensityValue::Zero | IntensityValue::Unknown) {
            assert!(tm.is_none());
        } else {
            let m = tm.unwrap();
            assert_eq!(r.from, m.from);
            let lower = codes(t).code_type == "35"
                || codes(t).code_type == "36"
                || a.condition.is_some()
                || codes(t).code_type == "47"
                    && matches!(aa.epicenters[0].as_deref(), Some("1" | "2"));
            assert_eq!(
                m.to,
                if lower {
                    IntensityValue::Over
                } else {
                    r.to.clone()
                }
            );
        }
    } else {
        assert!(tm.is_none());
    }
    let xa = xi.and_then(|i| i.appendix.as_ref());
    let ta = ti.and_then(|i| i.appendix.as_ref());
    assert_eq!(
        xa.and_then(|a| a.max_int_change.as_ref()),
        ta.and_then(|a| a.max_int_change.as_ref())
    );
    assert_eq!(
        xa.and_then(|a| a.max_int_change_reason.as_ref()),
        ta.and_then(|a| a.max_int_change_reason.as_ref())
    );
    // Whitelist: legacy EBI omits regions whose maximum is below 4; LPGM
    // values/change flags, grouping, date component and publisher names vanish.
    let mut xr: Vec<_> = xi
        .and_then(|i| i.regions.as_ref())
        .into_iter()
        .flatten()
        .filter(|r| {
            let i = r.forecast_max_int.as_ref().unwrap();
            i.from.rank().is_some_and(|n| n >= 4) || i.to.rank().is_some_and(|n| n >= 4)
        })
        .collect();
    let mut tr: Vec<_> = ti
        .and_then(|i| i.regions.as_ref())
        .into_iter()
        .flatten()
        .collect();
    xr.sort_by_key(|r| &r.area.code);
    tr.sort_by_key(|r| &r.area.code);
    assert_eq!(xr.len(), tr.len());
    for (a, b) in xr.into_iter().zip(tr) {
        assert_eq!(a.area.code, b.area.code);
        assert_eq!(a.kind.code, b.kind.code);
        assert_eq!(a.forecast_max_int, b.forecast_max_int);
        assert_eq!(a.is_plum, b.is_plum);
        assert_eq!(a.is_warning, b.is_warning);
        assert_eq!(a.condition, b.condition);
        let time = a.arrival_time.as_ref().map(|s| {
            DateTime::parse_from_rfc3339(s)
                .unwrap()
                .with_timezone(&FixedOffset::east_opt(32400).unwrap())
                .format("%H%M%S")
                .to_string()
        });
        if a.is_plum == Some(true) {
            assert_eq!(time, b.plum_prediction_time_of_day);
            assert!(b.arrival_time_of_day.is_none());
        } else {
            assert_eq!(time, b.arrival_time_of_day);
            assert!(b.plum_prediction_time_of_day.is_none());
        }
    }
}
fn roundtrip(raw: &[u8]) -> (String, EewInformation) {
    let x = parse_xml(raw).unwrap();
    assert_eq!(x.source.as_ref().unwrap().raw.as_bytes(), raw);
    let text = encode_xml_as_telegram(raw).unwrap();
    check_format(&text);
    let t = parse_telegram(text.as_bytes()).unwrap();
    compare_projection(&x, &t);
    (text, t)
}
fn unsupported(raw: &str, field: &str) {
    parse_xml(raw.as_bytes()).unwrap();
    match encode_xml_as_telegram(raw.as_bytes()).unwrap_err() {
        EewError::Unsupported {
            field: Some(actual),
            reason,
            ..
        } => {
            assert!(actual.contains(field), "{actual}: {reason}");
            assert!(!reason.is_empty());
        }
        error => panic!("expected diagnostic Unsupported: {error}"),
    }
}
fn rejected(raw: &str) {
    assert!(matches!(
        encode_xml_as_telegram(raw.as_bytes()),
        Err(EewError::MissingXmlField(_) | EewError::InvalidXml(_) | EewError::UnsupportedXml(_))
    ));
}

#[test]
fn production_22_original_messages_match_frozen_codes_and_shared_semantics() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&fixture("production_20261106/manifest.json")).unwrap();
    let expected: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(&fixture("production_20261106/expected_telegrams.json")).unwrap();
    let samples = manifest["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 22);
    let mut plum = 0;
    for row in samples {
        let name = row["file"].as_str().unwrap();
        let raw = fixture(&format!("production_20261106/{name}"));
        assert_eq!(raw.len() as u64, row["bytes"].as_u64().unwrap());
        assert_eq!(row["bytes"], row["log_declared_bytes"]);
        let (text, t) = roundtrip(&raw);
        assert_eq!(text, expected[name], "{name}");
        assert_eq!(t.event_id(), row["event_id"].as_str());
        assert_eq!(t.serial().unwrap() as u64, row["serial"].as_u64().unwrap());
        if t.event_id() == Some("20261106194044") {
            plum += 1;
            assert_eq!(codes(&t).rk.as_deref(), Some("118/1"));
            assert_eq!(codes(&t).rt.as_deref(), Some("/09//"));
            assert!(
                t.body
                    .earthquake
                    .as_ref()
                    .unwrap()
                    .magnitude
                    .as_ref()
                    .unwrap()
                    .bound
                    .is_none()
            );
            assert_eq!(t.is_final(), t.serial() == Some(5));
            let rs = t.body.intensity.as_ref().unwrap().regions.as_ref().unwrap();
            assert_eq!(rs.len(), usize::from(t.serial() != Some(1)));
            for r in rs {
                assert_eq!(r.kind.code, "09");
                assert_eq!(r.is_plum, Some(true));
            }
        }
    }
    assert_eq!(plum, 5);
}
fn xml_files(root: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(root).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            xml_files(&p, out)
        } else if p.extension().and_then(|s| s.to_str()) == Some("xml") {
            out.push(p);
        }
    }
}
#[test]
fn every_frozen_xml_has_an_explicit_supported_or_unsupported_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut files = Vec::new();
    xml_files(&root, &mut files);
    files.sort();
    let mut supported = 0;
    let mut unavailable = 0;
    for path in files {
        let raw = fs::read(&path).unwrap();
        let x = parse_xml(&raw).unwrap();
        let synthetic = path.to_string_lossy().contains("synthetic/20261016123810");
        if matches!(x.product(), Product::Test | Product::RealTimeIntensity) {
            assert!(matches!(
                encode_xml_as_telegram(&raw),
                Err(EewError::UnsupportedXml(_))
            ));
            unavailable += 1;
        } else if x.is_canceled() {
            unsupported(std::str::from_utf8(&raw).unwrap(), "Earthquake");
            unavailable += 1;
        } else if synthetic && x.serial().is_some_and(|n| n >= 17) {
            unsupported(std::str::from_utf8(&raw).unwrap(), "ForecastInt");
            unavailable += 1;
        } else {
            roundtrip(&raw);
            supported += 1;
        }
    }
    assert!(supported >= 100);
    assert!(unavailable >= 30);
    eprintln!("Frozen XML conversion contracts: {supported} supported, {unavailable} unsupported");
}

#[test]
fn rk_unknown_and_every_defined_rank_keep_field_specific_meanings() {
    for (old, values, slot) in [
        ("<Epicenter rank=\"3\"", vec![1, 2, 3, 4, 5, 6, 7, 8], 0),
        ("<Depth rank=\"3\"", (0..=8).collect(), 1),
        (
            "<MagnitudeCalculation rank=\"5\"",
            vec![0, 2, 3, 4, 5, 6, 8],
            2,
        ),
        ("<NumberOfMagnitudeCalculation>1<", (0..=5).collect(), 3),
        ("rank2=\"3\"", vec![0, 1, 2, 3, 4, 9], 4),
    ] {
        for value in values {
            let new = if old.contains('>') {
                format!("<NumberOfMagnitudeCalculation>{value}<")
            } else if slot == 4 {
                format!("rank2=\"{value}\"")
            } else {
                format!("{}\"{value}\"", old.split('"').next().unwrap())
            };
            let mut raw = change(NOTO, old, &new);
            if slot == 0 && matches!(value, 1 | 2) {
                // Pattern 2 cannot preserve the original finite maximum bound.
                unsupported(&raw, "ForecastInt");
                raw = raw.replacen(
                    "<From>5+</From><To>5+</To>",
                    "<From>5+</From><To>over</To>",
                    1,
                );
            }
            let (_, m) = roundtrip(raw.as_bytes());
            assert_eq!(
                codes(&m).rk.as_ref().unwrap().as_bytes()[slot],
                if value == 0 { b'/' } else { b'0' + value }
            );
        }
    }
    unsupported(
        &change(NOTO, "<Epicenter rank=\"3\"", "<Epicenter rank=\"0\""),
        "Epicenter",
    );
    let zero = change(GAL, "rank2=\"1\"", "rank2=\"0\"");
    roundtrip(zero.as_bytes());
    // RC zero means no change, not unknown. It must remain a numeric zero.
    let (_, m) = roundtrip(PLUM.as_bytes());
    assert_eq!(codes(&m).rc.as_deref(), Some("00///"));
    for (old, new) in [
        ("<Epicenter rank=\"3\"", "<Epicenter rank=\"9\""),
        ("<Depth rank=\"3\"", "<Depth rank=\"9\""),
        ("rank2=\"3\"", "rank2=\"5\""),
        (
            "<MagnitudeCalculation rank=\"5\"",
            "<MagnitudeCalculation rank=\"1\"",
        ),
        (
            "<MagnitudeCalculation rank=\"5\"",
            "<MagnitudeCalculation rank=\"7\"",
        ),
        (
            "<NumberOfMagnitudeCalculation>1<",
            "<NumberOfMagnitudeCalculation>6<",
        ),
    ] {
        rejected(&change(NOTO, old, new));
    }
}
#[test]
fn assumed_hypocenter_accepts_unknown_count_without_fabricating_a_station() {
    for count in [0, 1] {
        for rank2 in [1, 9] {
            let raw = change(
                PLUM,
                "<NumberOfMagnitudeCalculation>0<",
                &format!("<NumberOfMagnitudeCalculation>{count}<"),
            );
            let raw = change(&raw, "rank2=\"1\"", &format!("rank2=\"{rank2}\""));
            let (_, m) = roundtrip(raw.as_bytes());
            assert_eq!(
                m.body.earthquake.unwrap().condition.as_deref(),
                Some("仮定震源要素")
            );
        }
    }
    for (old, new) in [
        ("-10000/", "-20000/"),
        (">1.0</jmx_eb:Magnitude>", ">2.0</jmx_eb:Magnitude>"),
        (
            "<NumberOfMagnitudeCalculation>0<",
            "<NumberOfMagnitudeCalculation>2<",
        ),
        ("<Depth rank=\"1\"", "<Depth rank=\"2\""),
        ("rank2=\"1\"", "rank2=\"2\""),
        ("<Code>09</Code>", "<Code>00</Code>"),
        ("</ReduceCode>", "</ReduceCode><LandOrSea>内陸</LandOrSea>"),
    ] {
        rejected(&change(PLUM, old, new));
    }
    rejected(&remove(PLUM, "OriginTime"));
    // A measured hypocenter can carry both conventional and PLUM areas.
    let measured = remove(PLUM, "Condition");
    let (_, m) = roundtrip(measured.as_bytes());
    assert!(m.body.earthquake.unwrap().condition.is_none());
    assert_eq!(
        codes_from_text(&encode_xml_as_telegram(measured.as_bytes()).unwrap())
            .rt
            .as_deref(),
        Some("/0///")
    );
}
fn codes_from_text(t: &str) -> TelegramCodes {
    parse_telegram(t.as_bytes())
        .unwrap()
        .source
        .unwrap()
        .telegram
        .unwrap()
}

#[test]
fn warning_updates_use_prc_unset_for_unrepresentable_addition_cause() {
    let (_, m) = roundtrip(WARNING.as_bytes());
    assert!(codes(&m).prc.as_ref().unwrap().contains('/'));
    for reason in [0, 1, 2, 3, 4, 9] {
        let raw = change(
            WARNING,
            "<MaxIntChangeReason>0<",
            &format!("<MaxIntChangeReason>{reason}<"),
        );
        let (_, m) = roundtrip(raw.as_bytes());
        assert_eq!(
            codes(&m).prc.as_ref().unwrap().as_bytes()[3],
            if matches!(reason, 1 | 2 | 9) {
                b'0' + reason
            } else {
                b'/'
            }
        );
    }
    let raw = remove(WARNING, "Appendix");
    roundtrip(raw.as_bytes());
}
#[test]
fn zero_unknown_nan_and_unrepresentable_ebi_bounds_are_distinguished() {
    let base = std::str::from_utf8(&fixture("production_20261106/20261106193937-11.xml"))
        .unwrap()
        .to_owned();
    let mut depth_base = base.clone();
    while depth_base.contains("<ForecastLgInt>") {
        depth_base = remove(&depth_base, "ForecastLgInt");
    }
    if depth_base.contains("<MaxLgIntChange>") {
        depth_base = remove(&depth_base, "MaxLgIntChange");
    }
    for coord in ["+35.0+142.3/", "+35.0+142.3+0/", "+35.0+142.3-700000/"] {
        roundtrip(change(&depth_base, "+35.0+142.3-70000/", coord).as_bytes());
    }
    // Magnitude is xs:float (not nullablefloat): empty content is invalid.
    let empty_magnitude = change(&base, ">6.1</jmx_eb:Magnitude>", "></jmx_eb:Magnitude>");
    assert!(parse_xml(empty_magnitude.as_bytes()).is_err());
    assert!(encode_xml_as_telegram(empty_magnitude.as_bytes()).is_err());
    for mag in ["NaN", "0.0", "9.9"] {
        roundtrip(
            change(
                &base,
                ">6.1</jmx_eb:Magnitude>",
                &format!(">{mag}</jmx_eb:Magnitude>"),
            )
            .as_bytes(),
        );
    }
    for value in ["0", "不明"] {
        let raw = change(
            &base,
            "<ForecastInt><From>3</From><To>3</To></ForecastInt>",
            &format!("<ForecastInt><From>{value}</From><To>{value}</To></ForecastInt>"),
        );
        let (_, m) = roundtrip(raw.as_bytes());
        assert_eq!(
            m.body
                .intensity
                .unwrap()
                .appendix
                .unwrap()
                .max_int_change
                .as_deref(),
            Some("0")
        );
    }
    unsupported(&change(NOTO, "+37.5+137.2-10000/", ""), "Coordinate");
    for from in ["0", "不明"] {
        unsupported(
            &change(
                PLUM,
                "<ForecastInt><From>4</From><To>over</To></ForecastInt><ArrivalTime>",
                &format!("<ForecastInt><From>{from}</From><To>4</To></ForecastInt><ArrivalTime>"),
            ),
            "ForecastInt",
        );
    }
    unsupported(
        &change(
            NOTO,
            "<From>5+</From><To>5+</To>",
            "<From>4</From><To>5+</To>",
        ),
        "ForecastInt",
    );
    unsupported(
        &change(
            NOTO,
            "<From>5+</From><To>5+</To>",
            "<From>5+</From><To>over</To>",
        ),
        "ForecastInt",
    );
}
#[test]
fn ebi_all_intensity_classes_ranges_categories_and_projection_threshold() {
    for v in ["1", "2", "3", "4", "5-", "5+", "6-", "6+", "7"] {
        let raw = change(
            PLUM,
            "<ForecastInt><From>4</From><To>over</To></ForecastInt><ArrivalTime>",
            &format!("<ForecastInt><From>{v}</From><To>7</To></ForecastInt><ArrivalTime>"),
        );
        roundtrip(raw.as_bytes());
    }
    let base = remove(PLUM, "Condition"); // retain ordinary hypocenter + PLUM area
    for code in ["00", "01", "09", "10", "11", "19"] {
        let mut raw = change(&base, "<Code>09</Code>", &format!("<Code>{code}</Code>"));
        if code.ends_with('1') {
            raw = change(
                &raw,
                "<ArrivalTime>2026-11-06T19:40:51+09:00</ArrivalTime>",
                "<Condition>既に主要動到達と推測</Condition>",
            );
        }
        roundtrip(raw.as_bytes());
    }
    let raw = change(
        &base,
        "<ForecastInt><From>4</From><To>over</To></ForecastInt><ArrivalTime>",
        "<ForecastInt><From>3</From><To>3</To></ForecastInt><ForecastLgInt><From>3</From><To>3</To></ForecastLgInt><ArrivalTime>",
    );
    let (_, m) = roundtrip(raw.as_bytes());
    assert_eq!(codes(&m).rt.as_deref(), Some("/0///"));
    let raw = change(&raw, "<Code>09</Code>", "<Code>19</Code>");
    let (_, m) = roundtrip(raw.as_bytes());
    assert_eq!(codes(&m).rt.as_deref(), Some("/1///"));
}
#[test]
fn product_lifecycle_serials_training_and_unsupported_states() {
    for serial in [1, 99, 100, 109, 110, 359] {
        let raw = change(
            NOTO,
            "<Serial>1</Serial>",
            &format!("<Serial>{serial}</Serial>"),
        );
        roundtrip(raw.as_bytes());
    }
    for serial in [360, 65536] {
        unsupported(
            &change(
                NOTO,
                "<Serial>1</Serial>",
                &format!("<Serial>{serial}</Serial>"),
            ),
            "Serial",
        );
    }
    rejected(&change(NOTO, "<Serial>1</Serial>", "<Serial>0</Serial>"));
    for src in [NOTO, PLUM, WARNING] {
        roundtrip(change(src, "<Status>通常</Status>", "<Status>訓練</Status>").as_bytes());
        unsupported(
            &change(src, "<Status>通常</Status>", "<Status>試験</Status>"),
            "Status",
        );
        unsupported(
            &change(
                src,
                "<InfoType>発表</InfoType>",
                "<InfoType>訂正</InfoType>",
            ),
            "InfoType",
        );
    }
    let cancelled = String::from_utf8(fixture("official_noto_cancellation.xml")).unwrap();
    rejected(&change(&cancelled, "</Body>", "<Intensity/></Body>"));
    rejected(&change(&cancelled, "</Body>", "<Earthquake/></Body>"));
    rejected(&change(
        NOTO,
        "</Body>",
        "<NextAdvisory>最終報ではない</NextAdvisory></Body>",
    ));
}
#[test]
fn structure_namespace_text_attributes_and_required_data_rejections() {
    let altered = change(NOTO, "xmlns:jmx_eb=", "xmlns:e=").replace("jmx_eb:", "e:");
    roundtrip(altered.as_bytes());
    roundtrip(format!("\u{feff}  \n{NOTO}").as_bytes());
    let title = NOTO.find("<Head ").unwrap();
    let raw = format!(
        "{}{}",
        &NOTO[..title],
        change(
            &NOTO[title..],
            "<Title>緊急地震速報（地震動予報）</Title>",
            "<Title>警報を伴う予報</Title>"
        )
    );
    roundtrip(raw.as_bytes());
    roundtrip(
        change(
            NOTO,
            "<Name>石川県能登地方</Name>",
            "<Name>石川県 &amp; 能登地方</Name>",
        )
        .as_bytes(),
    );
    for tag in [
        "EventID",
        "Serial",
        "OriginTime",
        "Hypocenter",
        "Coordinate",
        "Magnitude",
        "Accuracy",
        "ForecastInt",
        "Epicenter",
        "Depth",
        "MagnitudeCalculation",
    ] {
        let doc = roxmltree::Document::parse(NOTO).unwrap();
        let n = doc
            .descendants()
            .find(|n| n.is_element() && n.tag_name().name() == tag)
            .unwrap();
        let original = &NOTO[n.range()];
        rejected(&change(NOTO, original, &format!("{original}{original}")));
    }
    for tag in [
        "EventID",
        "Serial",
        "Earthquake",
        "Accuracy",
        "Epicenter",
        "Depth",
        "MagnitudeCalculation",
        "NumberOfMagnitudeCalculation",
        "ForecastInt",
        "Magnitude",
    ] {
        rejected(&remove(NOTO, tag));
    }
    for (old, new) in [
        (
            "<OriginTime>2024-01-01T16:10:08+09:00</OriginTime>",
            "<OriginTime/>",
        ),
        ("rank2=\"3\"", ""),
        ("+37.5+137.2-10000/", "+37.5+137.2-10000"),
        ("+37.5+137.2-10000/", "+91.0+137.2-10000/"),
        ("+37.5+137.2-10000/", "+37.5+181.0-10000/"),
        ("+37.5+137.2-10000/", "+37.5+137.2+10000/"),
        ("+37.5+137.2-10000/", "+37.5+137.2-710000/"),
        ("+37.5+137.2-10000/", "+37.5+137.2-10500/"),
        (
            "<Depth rank=\"3\">NaN</Depth>",
            "<Depth rank=\"3\">3</Depth>",
        ),
        (">5.5</jmx_eb:Magnitude>", ">inf</jmx_eb:Magnitude>"),
        (
            "</Forecast>",
            "<Appendix><MaxIntChange>3</MaxIntChange><MaxIntChangeReason>0</MaxIntChangeReason></Appendix></Forecast>",
        ),
        ("</Forecast>", "<Appendix/></Forecast>"),
        ("</Intensity>", "<Observation/></Intensity>"),
        ("<From>5+</From><To>5+</To>", "<From>7</From><To>4</To>"),
    ] {
        rejected(&change(NOTO, old, new));
    }
    for (old, new, field) in [
        ("+37.5+137.2-10000/", "+37.55+137.2-10000/", "latitude"),
        (
            ">5.5</jmx_eb:Magnitude>",
            ">10.0</jmx_eb:Magnitude>",
            "Magnitude",
        ),
        ("type=\"Mj\"", "type=\"Mw\"", "Magnitude"),
        ("気象庁本庁", "未知の官署", "EditorialOffice"),
    ] {
        unsupported(&change(NOTO, old, new), field);
    }
}
#[test]
fn timezones_calendar_edges_precision_and_two_digit_year_limits() {
    let base = std::str::from_utf8(&fixture("production_20261106/20261106193937-01.xml"))
        .unwrap()
        .to_owned();
    for (raw, expected) in [
        ("1999-12-31T15:00:00Z", "000101000000"),
        ("2099-12-31T14:59:59Z", "991231235959"),
        ("2024-02-29T23:59:59+09:00", "240229235959"),
        ("2026-11-06T20:39:42+10:00", "261106193942"),
    ] {
        let x = change(&base, "2026-11-06T10:39:42Z", raw);
        let (t, _) = roundtrip(x.as_bytes());
        assert_eq!(t.split_whitespace().nth(3), Some(expected));
    }
    for raw in ["1999-12-31T14:59:59Z", "2099-12-31T15:00:00Z"] {
        unsupported(&change(&base, "2026-11-06T10:39:42Z", raw), "DateTime");
    }
    for raw in ["2026-11-06T10:39:42.5Z", "2016-12-31T23:59:60Z"] {
        unsupported(&change(&base, "2026-11-06T10:39:42Z", raw), "DateTime");
    }
    rejected(&change(
        &base,
        "2026-11-06T10:39:42Z",
        "2026-02-29T10:00:00Z",
    ));
    let x = change(
        PLUM,
        "<ArrivalTime>2026-11-06T19:40:51+09:00</ArrivalTime>",
        "<ArrivalTime>2026-11-07T00:00:05+09:00</ArrivalTime>",
    );
    let (t, _) = roundtrip(x.as_bytes());
    assert!(t.contains("000005 09"));
}
#[test]
fn low_intensity_regions_are_validated_before_legacy_filtering() {
    let base = remove(PLUM, "Condition");
    let low = change(
        &base,
        "<ForecastInt><From>4</From><To>over</To></ForecastInt><ArrivalTime>",
        "<ForecastInt><From>3</From><To>3</To></ForecastInt><ArrivalTime>",
    );
    roundtrip(low.as_bytes());
    rejected(&change(&low, "<Code>09</Code>", "<Code>29</Code>"));
    rejected(&change(
        &low,
        "<ArrivalTime>2026-11-06T19:40:51+09:00</ArrivalTime>",
        "",
    ));
    rejected(&change(
        &low,
        "</Area></Pref>",
        "<Condition>既に主要動到達と推測</Condition></Area></Pref>",
    ));
    let doc = roxmltree::Document::parse(&low).unwrap();
    let pref = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "Pref")
        .unwrap();
    let text = &low[pref.range()];
    rejected(&change(&low, text, &format!("{text}{text}")));
}

#[test]
fn xml_simple_content_comments_cdata_and_entities_do_not_change_values() {
    for (old, new) in [
        (
            ">5.5</jmx_eb:Magnitude>",
            ">5<!-- comment -->.5</jmx_eb:Magnitude>",
        ),
        ("<From>5+</From>", "<From>5<![CDATA[+]]></From>"),
        (
            "<EventID>20240101161010</EventID>",
            "<EventID>20240101<!-- comment -->161010</EventID>",
        ),
        (
            "<Depth rank=\"3\">NaN</Depth>",
            "<Depth rank=\"3\">N<![CDATA[aN]]></Depth>",
        ),
        ("+37.5+137.2-10000/", "+37.5+137.2<!-- c -->-10000/"),
        ("<Code>0201</Code>", "<Code>02<!-- c -->01</Code>"),
    ] {
        let raw = change(NOTO, old, new);
        let (text, _) = roundtrip(raw.as_bytes());
        assert_eq!(text, encode_xml_as_telegram(NOTO.as_bytes()).unwrap());
        assert_eq!(
            parse_xml(raw.as_bytes()).unwrap().body,
            parse_xml(NOTO.as_bytes()).unwrap().body
        );
    }
    let last = String::from_utf8(fixture("official_noto_final.xml")).unwrap();
    roundtrip(
        change(
            &last,
            "緊急地震速報：最終報",
            "緊急地震速報<!-- comment -->：最終報",
        )
        .as_bytes(),
    );
}

#[test]
fn product_combinations_missing_warning_fields_and_headline_evidence() {
    rejected(&change(
        NOTO,
        "緊急地震速報（地震動予報）",
        "緊急地震速報（予報）",
    )); // VXSE44 cannot carry ForecastLgInt.
    let warning = String::from_utf8(fixture("official_plum_warning.xml")).unwrap();
    for tag in ["OriginTime", "ReduceCode", "Forecast", "Headline"] {
        rejected(&remove(&warning, tag));
    }
    let measured_warning = remove(&warning, "Condition");
    unsupported(
        &change(
            &measured_warning,
            "<Epicenter rank=\"1\"",
            "<Epicenter rank=\"0\"",
        ),
        "Epicenter",
    );
    // Headline warning evidence remains meaningful when all local intensity
    // forecasts are below 4 and disappear from EBI (e.g. LPGM-based warning).
    let doc = roxmltree::Document::parse(&warning).unwrap();
    let headline = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "Headline")
        .unwrap();
    let raw = change(
        PLUM,
        "<Headline><Text/></Headline>",
        &warning[headline.range()],
    );
    let (_, m) = roundtrip(raw.as_bytes());
    assert_eq!(codes(&m).rt.as_deref(), Some("/19//"));
    let unknown_headline = change(&warning, "<Code>31</Code>", "<Code>32</Code>");
    rejected(&unknown_headline);
    rejected(&change(
        &warning,
        "<LastKind><Name>なし</Name><Code>00</Code></LastKind>",
        "<LastKind><Name>なし</Name><Code>99</Code></LastKind>",
    ));
    rejected(&change(
        PLUM,
        "<Condition>仮定震源要素</Condition>",
        "<Condition>別の条件</Condition>",
    ));
    rejected(&change(
        PLUM,
        "</ReduceCode>",
        "</ReduceCode><LandOrSea>未知</LandOrSea>",
    ));
    for (old, new) in [
        ("<MaxIntChangeReason>0<", "<MaxIntChangeReason>5<"),
        ("<To>over</To>", "<To>8</To>"),
    ] {
        rejected(&change(PLUM, old, new));
    }
    for value in ["5", "over"] {
        rejected(&change(
            NOTO,
            "<ForecastLgInt><From>1</From>",
            &format!("<ForecastLgInt><From>{value}</From>"),
        ));
    }
}

#[test]
fn oversized_projection_requires_fragmentation_instead_of_forging_c11() {
    let base = remove(PLUM, "Condition");
    let doc = roxmltree::Document::parse(&base).unwrap();
    let pref = doc
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "Pref")
        .unwrap();
    let region = &base[pref.range()];
    let mut regions = String::new();
    // Only use real region codes already carried by the official final XML.
    let mut names = std::collections::BTreeSet::new();
    // The current JMA regional code table has enough distinct entries for a
    // large, explicitly synthetic projection. Names are deliberately unchanged.
    let table = include_str!("../src/codes.rs");
    let part = table
        .split("pub(crate) fn region(")
        .nth(1)
        .unwrap()
        .split("pub(crate) fn")
        .next()
        .unwrap();
    for line in part.lines() {
        if let Some(code) = line
            .trim()
            .strip_prefix('"')
            .and_then(|s| s.split('"').next())
            && code.len() == 3
            && code.bytes().all(|b| b.is_ascii_digit())
        {
            names.insert(code.to_string());
        }
    }
    for code in names {
        regions.push_str(&region.replace("<Code>250</Code>", &format!("<Code>{code}</Code>")));
    }
    let raw = change(&base, region, &regions);
    unsupported(&raw, "Cnf");
}

#[test]
fn warning_nested_final_marker_does_not_end_the_outer_warning_series() {
    let base = String::from_utf8(fixture("official_plum_warning.xml")).unwrap();
    let raw = change(
        &base,
        "</Body>",
        "<NextAdvisory>この情報をもって、緊急地震速報：最終報とします。</NextAdvisory></Body>",
    );
    let x = parse_xml(raw.as_bytes()).unwrap();
    assert!(x.is_final());
    assert!(!x.is_last_info());
    let text = encode_xml_as_telegram(raw.as_bytes()).unwrap();
    check_format(&text);
    let t = parse_telegram(text.as_bytes()).unwrap();
    assert_eq!(codes(&t).ncn.as_deref(), Some("9//"));
    assert!(!t.is_final());
    assert!(!t.is_last_info());
    assert_eq!(x.event_id(), t.event_id());
    assert_eq!(x.serial(), t.serial());
}
#[test]
fn versions_and_numeric_attribute_whitespace_are_explicit() {
    for version in ["1.0_0", "1.1_0", "1.2_0"] {
        roundtrip(
            change(
                GAL,
                "<InfoKindVersion>1.2_0</InfoKindVersion>",
                &format!("<InfoKindVersion>{version}</InfoKindVersion>"),
            )
            .as_bytes(),
        );
    }
    rejected(&change(
        GAL,
        "<InfoKindVersion>1.2_0</InfoKindVersion>",
        "<InfoKindVersion>1.3_0</InfoKindVersion>",
    ));
    roundtrip(change(NOTO, "<Epicenter rank=\"3\"", "<Epicenter rank=\" 3 \"").as_bytes());
    roundtrip(
        change(
            NOTO,
            "<OriginTime>2024-01-01T16:10:08+09:00</OriginTime>",
            "<OriginTime> 2024-01-01T16:10:08+09:00 </OriginTime>",
        )
        .as_bytes(),
    );
}

#[test]
fn unknown_quantity_conditions_and_lpgm_presence_cannot_conflict() {
    rejected(&change(
        NOTO,
        "type=\"Mj\"",
        "type=\"Mj\" condition=\"不明\"",
    ));
    let unknown = change(NOTO, ">5.5</jmx_eb:Magnitude>", ">NaN</jmx_eb:Magnitude>");
    roundtrip(change(&unknown, "type=\"Mj\"", "type=\"Mj\" condition=\"不明\"").as_bytes());
    rejected(&change(
        &unknown,
        "type=\"Mj\"",
        "type=\"Mj\" condition=\"未知の条件\"",
    ));
    let last = String::from_utf8(fixture("official_noto_final.xml")).unwrap();
    rejected(&remove(&last, "MaxLgIntChange"));
    rejected(&remove(&last, "ForecastLgInt"));
    let boundary = change(NOTO, "+37.5+137.2-10000/", "+37.5+137.2-150000/");
    roundtrip(boundary.as_bytes());
    rejected(&change(NOTO, "+37.5+137.2-10000/", "+37.5+137.2-160000/"));
}

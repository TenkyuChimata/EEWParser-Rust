//! Independent XML regressions grounded in JMA XML common chapters and II.21.
//! Mutations of frozen raw messages below are deliberately synthetic.
use eew_parser::{encode_xml_as_telegram, parse_xml};

const NOTO: &str = include_str!("fixtures/official_noto_first.xml");
const PLUM: &str = include_str!("fixtures/official_plum_warning.xml");
const RTI: &str = include_str!("fixtures/official/rti-first.xml");
const TEST: &str = include_str!("fixtures/official/54_01_01_100514_VXSE42.xml");

fn node_range(xml: &str, name: &str) -> std::ops::Range<usize> {
    roxmltree::Document::parse(xml)
        .unwrap()
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == name)
        .unwrap_or_else(|| panic!("missing test anchor {name}"))
        .range()
}

fn replace_node(xml: &str, name: &str, replacement: &str) -> String {
    let mut result = xml.to_owned();
    result.replace_range(node_range(xml, name), replacement);
    result
}

#[test]
fn conflicting_singletons_never_select_the_first_value() {
    for name in [
        "Title",
        "DateTime",
        "Status",
        "EditorialOffice",
        "PublishingOffice",
        "ReportDateTime",
        "TargetDateTime",
        "EventID",
        "InfoType",
        "Serial",
        "InfoKind",
        "InfoKindVersion",
        "Headline",
        "OriginTime",
        "ArrivalTime",
        "Hypocenter",
        "Accuracy",
        "Epicenter",
        "Depth",
        "MagnitudeCalculation",
        "NumberOfMagnitudeCalculation",
        "ReduceCode",
        "ReduceName",
        "LandOrSea",
        "Intensity",
        "Forecast",
        "ForecastInt",
        "ForecastLgInt",
        "From",
        "To",
        "Category",
        "Comments",
        "WarningComment",
    ] {
        let original = &NOTO[node_range(NOTO, name)];
        let bad = replace_node(NOTO, name, &format!("{original}{original}"));
        assert!(parse_xml(bad.as_bytes()).is_err(), "duplicate {name}");
    }
    let bad = NOTO.replace(
        "<InfoType>発表</InfoType>",
        "<InfoType>発表</InfoType><InfoType>取消</InfoType>",
    );
    assert!(parse_xml(bad.as_bytes()).is_err());
    let final_text =
        "<NextAdvisory>この情報をもって、緊急地震速報：最終報とします。</NextAdvisory>";
    let bad = NOTO.replace("</Body>", &format!("{final_text}{final_text}</Body>"));
    assert!(parse_xml(bad.as_bytes()).is_err());
}

#[test]
fn simple_content_cannot_hide_nested_data() {
    for (old, new) in [
        (
            "<InfoType>発表</InfoType>",
            "<InfoType>発表<Other>取消</Other></InfoType>",
        ),
        ("<Serial>1</Serial>", "<Serial>1<Other>9</Other></Serial>"),
        (
            ">5.5</jmx_eb:Magnitude>",
            ">5.5<Other>9.9</Other></jmx_eb:Magnitude>",
        ),
        (
            "+37.5+137.2-10000/",
            "+37.5+137.2-10000/<Other>+38+140-20000/</Other>",
        ),
        ("<From>5+</From>", "<From>5+<Other>7</Other></From>"),
        (
            "<ReduceName>石川県</ReduceName>",
            "<ReduceName>石川県<Other>東京都</Other></ReduceName>",
        ),
    ] {
        assert!(NOTO.contains(old));
        let bad = NOTO.replace(old, new);
        assert!(parse_xml(bad.as_bytes()).is_err(), "{new}");
        assert!(encode_xml_as_telegram(bad.as_bytes()).is_err(), "{new}");
    }
}

#[test]
fn absent_required_data_and_empty_present_times_are_not_unknown_values() {
    for name in [
        "TargetDateTime",
        "Headline",
        "Serial",
        "Accuracy",
        "Epicenter",
        "Depth",
        "MagnitudeCalculation",
        "NumberOfMagnitudeCalculation",
        "ReduceName",
        "ReduceCode",
        "ForecastInt",
    ] {
        let bad = replace_node(NOTO, name, "");
        assert!(parse_xml(bad.as_bytes()).is_err(), "missing {name}");
    }
    for (name, replacement) in [
        ("Serial", "<Serial/>"),
        ("Serial", "<Serial>+1</Serial>"),
        ("OriginTime", "<OriginTime/>"),
        ("Intensity", "<Intensity/>"),
        ("Forecast", "<Forecast/>"),
    ] {
        assert!(parse_xml(replace_node(NOTO, name, replacement).as_bytes()).is_err());
    }
    let bad = NOTO.replace("</Body>", "<NextAdvisory/></Body>");
    assert!(parse_xml(bad.as_bytes()).is_err());
    // The official VXSE42 sample has an empty Headline/Text, which is valid.
    let parsed = parse_xml(TEST.as_bytes()).unwrap();
    assert_eq!(parsed.header.event_id.as_deref(), Some("20090811050000"));
    assert_eq!(parsed.header.serial_no.as_deref(), Some("1"));
    assert!(parsed.header.headline.is_none());
}

#[test]
fn unknown_magnitude_requires_nan_and_cannot_conflict_with_a_numeric_value() {
    for value in ["", " ", "INF", "-INF", "NaN<Other/>"] {
        let bad = NOTO.replace(
            ">5.5</jmx_eb:Magnitude>",
            &format!(">{value}</jmx_eb:Magnitude>"),
        );
        assert!(parse_xml(bad.as_bytes()).is_err(), "{value:?}");
        assert!(encode_xml_as_telegram(bad.as_bytes()).is_err(), "{value:?}");
    }
    let unknown = NOTO.replace(
        "description=\"Ｍ５．５\">5.5",
        "condition=\"不明\" description=\"Ｍ不明\">NaN",
    );
    assert!(
        parse_xml(unknown.as_bytes())
            .unwrap()
            .body
            .earthquake
            .unwrap()
            .magnitude
            .unwrap()
            .value
            .is_none()
    );
    let contradictory = NOTO.replace("type=\"Mj\"", "type=\"Mj\" condition=\"不明\"");
    assert!(parse_xml(contradictory.as_bytes()).is_err());
    assert!(encode_xml_as_telegram(contradictory.as_bytes()).is_err());
}

#[test]
fn coordinate_notation_is_iso_decimal_and_precision_is_not_silently_rounded() {
    for coordinate in [
        "+3.75e1+137.2-10000/",
        "+37.5+1.372e2-10000/",
        "+37.5+137.2-1e4/",
        "+37.5+137.2-10000./",
        "+37.5+137.2-NaN/",
        "+90.000000000000000001+137.2-10000/",
        "+37.5+180.000000000000000001-10000/",
        "+37.5+137.2-700000.0000000000001/",
        "+37.5+137.2+0.0000000000000000001/",
    ] {
        let bad = NOTO.replace("+37.5+137.2-10000/", coordinate);
        assert!(parse_xml(bad.as_bytes()).is_err(), "{coordinate}");
        assert!(
            encode_xml_as_telegram(bad.as_bytes()).is_err(),
            "{coordinate}"
        );
    }
    let precise = NOTO.replace("+37.5+137.2-10000/", "-37.500001+137.200009-10000/");
    let eq = parse_xml(precise.as_bytes())
        .unwrap()
        .body
        .earthquake
        .unwrap();
    assert_eq!(
        eq.hypocenter.coordinate.latitude.unwrap().value,
        "-37.500001"
    );
    assert_eq!(
        eq.hypocenter.coordinate.longitude.unwrap().value,
        "137.200009"
    );
    assert!(encode_xml_as_telegram(precise.as_bytes()).is_err());
    let fractional = NOTO.replace("+37.5+137.2-10000/", "+37.5+137.2-0.000000000000000001/");
    let depth = parse_xml(fractional.as_bytes())
        .unwrap()
        .body
        .earthquake
        .unwrap()
        .hypocenter
        .depth
        .unwrap();
    assert_eq!(depth.value.as_deref(), Some("0.000000000000000000001"));
    assert!(depth.condition.is_none());
    for coordinate in ["+37.500000001+137.2-10000/", "+37.5+137.200000001-10000/"] {
        let xml = NOTO.replace("+37.5+137.2-10000/", coordinate);
        assert!(parse_xml(xml.as_bytes()).is_ok());
        assert!(encode_xml_as_telegram(xml.as_bytes()).is_err());
    }
    for magnitude in ["5.500000001", "5.5e-99"] {
        let xml = NOTO.replace(
            ">5.5</jmx_eb:Magnitude>",
            &format!(">{magnitude}</jmx_eb:Magnitude>"),
        );
        assert!(parse_xml(xml.as_bytes()).is_ok());
        assert!(encode_xml_as_telegram(xml.as_bytes()).is_err());
    }
    for magnitude in ["5.5e0", "550e-2"] {
        let xml = NOTO.replace(
            ">5.5</jmx_eb:Magnitude>",
            &format!(">{magnitude}</jmx_eb:Magnitude>"),
        );
        assert_eq!(
            encode_xml_as_telegram(xml.as_bytes()).unwrap(),
            encode_xml_as_telegram(NOTO.as_bytes()).unwrap()
        );
    }
    let negative_zero = NOTO.replace("+37.5+137.2-10000/", "-0.0-0.0-10000/");
    assert!(
        encode_xml_as_telegram(negative_zero.as_bytes())
            .unwrap()
            .contains(" S000 W0000 ")
    );
    let eq = parse_xml(negative_zero.as_bytes())
        .unwrap()
        .body
        .earthquake
        .unwrap();
    assert_eq!(eq.hypocenter.coordinate.latitude.unwrap().value, "-0.0000");
    assert_eq!(eq.hypocenter.coordinate.longitude.unwrap().value, "-0.0000");
    for coordinate in ["", "+37.5+137.2/"] {
        assert!(
            parse_xml(NOTO.replace("+37.5+137.2-10000/", coordinate).as_bytes())
                .unwrap()
                .body
                .earthquake
                .unwrap()
                .hypocenter
                .depth
                .unwrap()
                .value
                .is_none()
        );
    }
}

#[test]
fn forecast_categories_and_duplicate_regions_are_validated_before_flags() {
    let area_doc = roxmltree::Document::parse(NOTO).unwrap();
    let area = area_doc
        .descendants()
        .find(|n| n.has_tag_name("Area") && n.children().any(|c| c.has_tag_name("Category")))
        .unwrap();
    let raw_area = &NOTO[area.range()];
    let duplicate = NOTO.replacen(raw_area, &format!("{raw_area}{raw_area}"), 1);
    assert!(parse_xml(duplicate.as_bytes()).is_err());
    for replacement in [
        "<Condition/>",
        "",
        "<ArrivalTime>2024-01-01T16:10:20+09:00</ArrivalTime>",
    ] {
        let bad = NOTO.replace("<Condition>既に主要動到達と推測</Condition>", replacement);
        assert!(parse_xml(bad.as_bytes()).is_err(), "{replacement}");
    }
    let plum = roxmltree::Document::parse(PLUM).unwrap();
    let arrival = plum
        .descendants()
        .find(|n| {
            n.has_tag_name("ArrivalTime") && n.parent().is_some_and(|p| p.has_tag_name("Area"))
        })
        .unwrap();
    let mut bad = PLUM.to_owned();
    bad.replace_range(arrival.range(), "");
    assert!(parse_xml(bad.as_bytes()).is_err());
    // Preserve XML's original field: for PLUM it is first prediction time.
    let parsed = parse_xml(PLUM.as_bytes()).unwrap();
    assert!(
        parsed
            .body
            .intensity
            .unwrap()
            .regions
            .unwrap()
            .iter()
            .all(|r| r.is_plum == Some(true)
                && r.arrival_time.is_some()
                && r.arrival_time_of_day.is_none()
                && r.plum_prediction_time_of_day.is_none())
    );
}

#[test]
fn long_period_fields_cannot_migrate_to_the_wrong_product_or_deep_hypocenter() {
    let wrong_product = NOTO.replace("緊急地震速報（地震動予報）", "緊急地震速報（予報）");
    assert!(parse_xml(wrong_product.as_bytes()).is_err());
    assert!(encode_xml_as_telegram(wrong_product.as_bytes()).is_err());
    for depth in ["-160000/", "-150000.000000000000001/"] {
        let too_deep = NOTO.replace("-10000/", depth);
        assert!(parse_xml(too_deep.as_bytes()).is_err());
        assert!(encode_xml_as_telegram(too_deep.as_bytes()).is_err());
    }
    assert!(parse_xml(NOTO.replace("-10000/", "-150000/").as_bytes()).is_ok());
}

#[test]
fn realtime_observation_must_contain_unambiguous_stations() {
    assert!(parse_xml(replace_node(RTI, "Observation", "<Observation/>").as_bytes()).is_err());
    let station = &RTI[node_range(RTI, "IntensityStation")];
    let bad = replace_node(RTI, "IntensityStation", &format!("{station}{station}"));
    assert!(parse_xml(bad.as_bytes()).is_err());
    let bad = replace_node(RTI, "K", "<K>4.4</K><K>6.0</K>");
    assert!(parse_xml(bad.as_bytes()).is_err());
    // JMA's example explicitly permits negative instrumental intensity.
    assert!(parse_xml(replace_node(RTI, "K", "<K>-1.2</K>").as_bytes()).is_ok());
}

#[test]
fn realtime_k_may_be_absent_without_losing_the_station_or_inventing_intensity() {
    // JMA PLUM supplement 2017-07, p2 explicitly allows absent K and unknown Int.
    // The current (2024-10-31) jmx_seis.xsd still declares K minOccurs="0".
    for (intensity, expected) in [
        ("4", eew_parser::IntensityValue::Four),
        ("不明", eew_parser::IntensityValue::Unknown),
    ] {
        let xml = replace_node(
            &replace_node(RTI, "K", ""),
            "Int",
            &format!("<Int>{intensity}</Int>"),
        );
        let station = parse_xml(xml.as_bytes())
            .unwrap()
            .body
            .intensity
            .unwrap()
            .stations
            .unwrap()
            .remove(0);
        assert_eq!(station.int, expected);
        assert!(station.k.is_none());
        assert_eq!(station.station.code, "1720500");
    }
    assert!(parse_xml(replace_node(RTI, "K", "<K/>").as_bytes()).is_err());
}

#[test]
fn namespaced_attributes_cannot_shadow_or_supply_unqualified_jma_attributes() {
    let base = NOTO.replace("<Report ", "<Report xmlns:foreign=\"urn:test:extension\" ");
    let shadowed = base
        .replace(
            "type=\"Mj\"",
            "foreign:type=\"Mw\" foreign:condition=\"不明\" type=\"Mj\"",
        )
        .replace(
            "datum=\"日本測地系\"",
            "foreign:datum=\"世界測地系\" foreign:condition=\"不明\" datum=\"日本測地系\"",
        )
        .replace(
            "<Epicenter rank=\"3\"",
            "<Epicenter foreign:rank=\"1\" rank=\"3\"",
        )
        .replace(
            "<Information type=",
            "<Information foreign:type=\"unknown\" type=",
        );
    assert_eq!(
        parse_xml(shadowed.as_bytes()).unwrap().body,
        parse_xml(NOTO.as_bytes()).unwrap().body
    );
    assert_eq!(
        encode_xml_as_telegram(shadowed.as_bytes()).unwrap(),
        encode_xml_as_telegram(NOTO.as_bytes()).unwrap()
    );
    for (old, new) in [
        ("type=\"Mj\"", "foreign:type=\"Mj\""),
        ("<Epicenter rank=\"3\"", "<Epicenter foreign:rank=\"3\""),
    ] {
        let bad = base.replace(old, new);
        assert!(parse_xml(bad.as_bytes()).is_err());
        assert!(encode_xml_as_telegram(bad.as_bytes()).is_err());
    }
    let foreign_datum = base.replace("datum=\"日本測地系\"", "foreign:datum=\"日本測地系\"");
    assert!(
        parse_xml(foreign_datum.as_bytes())
            .unwrap()
            .body
            .earthquake
            .unwrap()
            .hypocenter
            .coordinate
            .geodetic_system
            .is_none()
    );
}

#[test]
fn explicit_foreign_code_systems_are_not_interpreted_using_jma_tables() {
    for (source, path, table) in [
        (NOTO, "Pref/Area/Category/Kind/Code", "緊急地震速報"),
        (NOTO, "Pref/Area/Code", "地震情報／細分区域"),
        (
            RTI,
            "Pref/Area/City/IntensityStation/Code",
            "リアルタイム震度観測点",
        ),
    ] {
        let old = format!("<Type xpath=\"{path}\">{table}</Type>");
        assert!(source.contains(&old));
        let bad = source.replace(
            &old,
            &format!("<Type xpath=\"{path}\">別のコード体系</Type>"),
        );
        assert!(matches!(
            parse_xml(bad.as_bytes()),
            Err(eew_parser::EewError::UnsupportedXml(_))
        ));
        assert!(encode_xml_as_telegram(bad.as_bytes()).is_err());
    }
    // Extensions for paths not interpreted here and omitted definitions remain supported.
    let extension = NOTO.replace(
        "</CodeDefine>",
        "<Type xpath=\"Extension/Code\">future table</Type></CodeDefine>",
    );
    assert!(parse_xml(extension.as_bytes()).is_ok());
    assert!(parse_xml(replace_node(NOTO, "CodeDefine", "").as_bytes()).is_ok());
}

#[test]
fn rank_and_appendix_fields_require_integer_syntax_without_discarding_future_codes() {
    for value in ["bogus", "NaN", "1.5", "2147483648"] {
        let bad = NOTO.replace("rank=\"3\"", &format!("rank=\"{value}\""));
        assert!(parse_xml(bad.as_bytes()).is_err());
        let bad = replace_node(
            NOTO,
            "NumberOfMagnitudeCalculation",
            &format!("<NumberOfMagnitudeCalculation>{value}</NumberOfMagnitudeCalculation>"),
        );
        assert!(parse_xml(bad.as_bytes()).is_err());
    }
    let future = NOTO.replace("<Epicenter rank=\"3\"", "<Epicenter rank=\"9\"");
    assert_eq!(
        parse_xml(future.as_bytes())
            .unwrap()
            .body
            .earthquake
            .unwrap()
            .hypocenter
            .accuracy
            .unwrap()
            .epicenters[0]
            .as_deref(),
        Some("9")
    );
    for coordinate in [
        "+37.5+137.2-700001/",
        "+37.5+137.2-999999999999999999999999/",
    ] {
        assert!(parse_xml(NOTO.replace("+37.5+137.2-10000/", coordinate).as_bytes()).is_err());
    }
}

#[test]
fn pattern_two_encoder_never_changes_a_finite_maximum_into_an_open_upper_bound() {
    for rank in [1, 2] {
        let xml = NOTO.replace(
            "<Epicenter rank=\"3\"",
            &format!("<Epicenter rank=\"{rank}\""),
        );
        assert!(parse_xml(xml.as_bytes()).is_ok());
        assert!(matches!(
            encode_xml_as_telegram(xml.as_bytes()),
            Err(eew_parser::EewError::Unsupported { .. })
        ));
    }
    let range = roxmltree::Document::parse(PLUM).unwrap();
    let from = range
        .descendants()
        .find(|n| n.has_tag_name("From"))
        .unwrap()
        .text()
        .unwrap();
    let finite = replace_node(
        PLUM,
        "ForecastInt",
        &format!("<ForecastInt><From>{from}</From><To>{from}</To></ForecastInt>"),
    );
    assert!(parse_xml(finite.as_bytes()).is_ok());
    assert!(matches!(
        encode_xml_as_telegram(finite.as_bytes()),
        Err(eew_parser::EewError::Unsupported { .. })
    ));
}

#[test]
fn names_comments_and_source_bytes_remain_verbatim() {
    let raw = NOTO
        .replace(
            "<ReduceName>石川県</ReduceName>",
            "<ReduceName>石川県\n</ReduceName>",
        )
        .replace(
            "<Name>石川県能登地方</Name>",
            "<Name> 石川県能登地方 </Name>",
        )
        .replace(
            "<Text>強い揺れに警戒してください。</Text>",
            "<Text>強い揺れに警戒してください。\n</Text>",
        );
    let parsed = parse_xml(raw.as_bytes()).unwrap();
    let eq = parsed.body.earthquake.unwrap();
    assert_eq!(eq.hypocenter.area.name.as_deref(), Some(" 石川県能登地方 "));
    assert_eq!(
        eq.hypocenter.reduce.unwrap().name.as_deref(),
        Some("石川県\n")
    );
    assert_eq!(
        parsed.body.comments.unwrap().warning.unwrap().text,
        "強い揺れに警戒してください。\n"
    );
    assert_eq!(parsed.source.unwrap().raw, raw);
}

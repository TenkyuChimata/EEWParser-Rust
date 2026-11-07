//! Direct XML semantic decoding. The DOM is constructed exactly once; the
//! optional legacy Telegram encoder is not part of this path.
use self::{NS_ELEMENT as EB, NS_INFO as IB, NS_REPORT as JMX, NS_SEIS as SEIS};
use crate::{EewError, model::*};
pub(crate) const NS_REPORT: &str = "http://xml.kishou.go.jp/jmaxml1/";
pub(crate) const NS_INFO: &str = "http://xml.kishou.go.jp/jmaxml1/informationBasis1/";
pub(crate) const NS_SEIS: &str = "http://xml.kishou.go.jp/jmaxml1/body/seismology1/";
pub(crate) const NS_ELEMENT: &str = "http://xml.kishou.go.jp/jmaxml1/elementBasis1/";

use chrono::DateTime;
use roxmltree::{Document, Node, ParsingOptions};
use std::{borrow::Cow, collections::HashSet};

pub fn parse(raw: &str) -> Result<EewInformation, EewError> {
    let xml = raw.strip_prefix('\u{feff}').unwrap_or(raw).trim_start();
    crate::input::check_xml_depth(xml)?;
    let doc = Document::parse_with_options(
        xml,
        ParsingOptions {
            nodes_limit: 250_000,
            ..ParsingOptions::default()
        },
    )
    .map_err(|e| EewError::InvalidXml(e.to_string()))?;
    let root = doc.root_element();
    if !tag(root, JMX, "Report") {
        return Err(EewError::UnsupportedXml(
            "expected JMA Report namespace".into(),
        ));
    }
    let control = required(root, JMX, "Control")?;
    let head = required(root, IB, "Head")?;
    let body_node = required(root, SEIS, "Body")?;
    let control_title = text(control, JMX, "Title")?;
    let product = Product::from_control_title(&control_title);
    if product == Product::Unknown {
        return Err(EewError::UnsupportedXml(format!(
            "unknown Control/Title {control_title:?}"
        )));
    }
    let info_kind = text(head, IB, "InfoKind")?;
    if info_kind != "緊急地震速報" {
        return Err(EewError::UnsupportedXml(format!(
            "unknown InfoKind {info_kind:?}"
        )));
    }
    let version = text(head, IB, "InfoKindVersion")?;
    if !matches!(version.as_ref(), "1.0_0" | "1.1_0" | "1.2_0") {
        return Err(EewError::UnsupportedXml(format!(
            "unsupported InfoKindVersion {version:?}"
        )));
    }
    let status = Status::from_text(&text(control, JMX, "Status")?);
    let info_type = InfoType::from_text(&text(head, IB, "InfoType")?);
    if let InfoType::Other(value) = &info_type {
        return Err(EewError::UnsupportedXml(format!(
            "unknown InfoType {value:?}: lifecycle cannot be determined"
        )));
    }
    let canceled = info_type == InfoType::Cancellation;
    // JMA XML common appendix 03 expressly excludes operational test cancellations.
    if product == Product::Test && canceled {
        return Err(EewError::UnsupportedXml(
            "VXSE42 has no cancellation operation".into(),
        ));
    }
    required(head, IB, "EventID")?;
    let event_id = optional_text(head, IB, "EventID")?.map(|s| s.trim().to_owned());
    if event_id
        .as_ref()
        .is_some_and(|s| s.len() != 14 || !s.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid("Head/EventID must be 14 digits"));
    }
    if product != Product::Test && event_id.is_none() {
        return Err(invalid("Head/EventID"));
    }
    required(head, IB, "Serial")?;
    let serial = optional_text(head, IB, "Serial")?.map(|s| s.trim().to_owned());
    if let Some(s) = &serial {
        if !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("Head/Serial must contain ASCII digits"));
        }
        let number = s
            .parse::<u32>()
            .map_err(|_| invalid("Head/Serial must be an unsigned integer"))?;
        if number == 0 {
            return Err(invalid("Head/Serial must be positive"));
        }
    }
    if product != Product::Test && serial.is_none() {
        return Err(invalid("Head/Serial must be a positive integer"));
    }
    let headline = required(head, IB, "Headline")?;
    required(headline, IB, "Text")?;
    let header = Header {
        information_type: Some(control_title.into()),
        title: Some(text(head, IB, "Title")?.into()),
        status: Some(status),
        info_type: Some(info_type),
        editorial_office: Some(text(control, JMX, "EditorialOffice")?.into()),
        publishing_office: Some(
            text(control, JMX, "PublishingOffice")?
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        ),
        press_date_time: Some(time(&text(control, JMX, "DateTime")?, "Control/DateTime")?),
        report_date_time: Some(time(
            &text(head, IB, "ReportDateTime")?,
            "Head/ReportDateTime",
        )?),
        target_date_time: Some(time(
            &text(head, IB, "TargetDateTime")?,
            "Head/TargetDateTime",
        )?),
        event_id,
        serial_no: serial,
        info_kind: Some(info_kind.into()),
        info_kind_version: Some(version.into()),
        headline: optional_text(headline, IB, "Text")?,
        target_date_time_dubious: optional_text(head, IB, "TargetDTDubious")?,
        target_duration: optional_text(head, IB, "TargetDuration")?,
        valid_date_time: optional_time(head, IB, "ValidDateTime")?,
    };
    let next_advisory = child(body_node, SEIS, "NextAdvisory")?
        .map(|_| text(body_node, SEIS, "NextAdvisory").map(Cow::into_owned))
        .transpose()?;
    let final_report =
        next_advisory.as_deref() == Some("この情報をもって、緊急地震速報：最終報とします。");
    if next_advisory.is_some() && !final_report {
        return Err(EewError::UnsupportedXml(
            "unrecognized EEW NextAdvisory: final marker cannot be determined".into(),
        ));
    }
    if children(body_node, SEIS, "Earthquake").count() > 1 {
        return Err(EewError::UnsupportedXml(
            "EEW contains multiple earthquakes".into(),
        ));
    }
    let eq_node = child(body_node, SEIS, "Earthquake")?;
    let int_node = child(body_node, SEIS, "Intensity")?;
    if canceled && (eq_node.is_some() || int_node.is_some()) {
        return Err(EewError::InvalidXml(
            "cancellation contains Earthquake/Intensity".into(),
        ));
    }
    if product == Product::Test && (eq_node.is_some() || int_node.is_some()) {
        return Err(EewError::InvalidXml(
            "VXSE42 contains earthquake/intensity data".into(),
        ));
    }
    if !canceled && product != Product::Test && eq_node.is_none() {
        if header.info_type == Some(InfoType::Correction) {
            return Err(EewError::UnsupportedXml("text-only correction has no independent lifecycle evidence; an original report is required".into()));
        }
        return Err(invalid("Body/Earthquake"));
    }
    let mut body = Body {
        is_last_info: canceled
            || (product != Product::Test && product != Product::Warning && final_report),
        is_canceled: canceled,
        earthquake: eq_node.map(earthquake).transpose()?,
        intensity: int_node.map(|n| intensity(n, product)).transpose()?,
        text: optional_text(body_node, SEIS, "Text")?,
        comments: child(body_node, SEIS, "Comments")?
            .map(comments)
            .transpose()?,
        next_advisory,
        ..Body::default()
    };
    if canceled || product == Product::Test {
        if body.text.is_none() {
            return Err(invalid("Body/Text"));
        }
        if body.comments.is_some() || body.next_advisory.is_some() {
            return Err(invalid("cancellation/test Body may only contain Text"));
        }
    } else {
        if body
            .earthquake
            .as_ref()
            .and_then(|e| e.hypocenter.depth.as_ref())
            .and_then(|d| d.value.as_deref())
            .is_some_and(|d| !coordinate_within_bound(d, 150))
            && body.intensity.as_ref().is_some_and(has_long_period)
        {
            return Err(invalid(
                "ForecastLgInt is not defined for hypocenters deeper than 150 km",
            ));
        }
        if matches!(product, Product::Warning | Product::GroundMotionForecast) {
            warning_areas(Some(headline), &mut body)?;
        }
        if product != Product::RealTimeIntensity {
            // Kind codes explicitly convey warning issuance; intensity values do not.
            let regions = body.intensity.as_ref().and_then(|i| i.regions.as_ref());
            let region_warning =
                regions.is_some_and(|rs| rs.iter().any(|r| r.is_warning == Some(true)));
            let unknown_region =
                regions.is_some_and(|rs| rs.iter().any(|r| r.is_warning.is_none()));
            let warning_areas = [&body.zones, &body.prefectures, &body.regions]
                .into_iter()
                .flatten()
                .flatten();
            let mut area_warning = false;
            let mut unknown_area = false;
            for area in warning_areas {
                area_warning |= area.kind.current.code == "31";
                unknown_area |= area.kind.current.code != "31";
            }
            body.is_warning = if product == Product::Warning || region_warning || area_warning {
                Some(true)
            } else if unknown_region || unknown_area {
                None
            } else {
                Some(false)
            };
        }
    }
    Ok(EewInformation {
        header,
        product,
        body,
        source: Some(Source {
            format: SourceFormat::Xml,
            raw: raw.to_owned(),
            final_report,
            sent_at: None,
            code_table_version: None,
            telegram: None,
        }),
    })
}

fn earthquake(node: Node<'_, '_>) -> Result<Earthquake, EewError> {
    let hyp = required(node, SEIS, "Hypocenter")?;
    let area = required(hyp, SEIS, "Area")?;
    let (coordinate, depth) = coordinate(required(area, EB, "Coordinate")?)?;
    let magnitude = Some(required(node, EB, "Magnitude")?)
        .map(|m| {
            let content = checked_scalar_text(m)?;
            let raw = content.trim();
            let value = if raw == "NaN" {
                None
            } else {
                number(raw, "Magnitude")?;
                Some(raw.to_owned())
            };
            if value.is_some() && attribute(m, "condition").is_some() {
                return Err(invalid("numeric Magnitude cannot have a condition"));
            }
            Ok::<_, EewError>(Quantity {
                kind: "マグニチュード".into(),
                unit: attribute(m, "type")
                    .ok_or_else(|| invalid("Earthquake/Magnitude/@type"))?
                    .into(),
                bound: None,
                condition: if value.is_none() {
                    Some(attribute(m, "description").unwrap_or("Ｍ不明").into())
                } else {
                    None
                },
                value,
            })
        })
        .transpose()?;
    let a = required(hyp, SEIS, "Accuracy")?;
    let rank = |name, attr| -> Result<Option<String>, EewError> {
        let node = required(a, SEIS, name)?;
        if checked_scalar_text(node)?.trim() != "NaN" {
            return Err(invalid(format!("Accuracy/{name} must contain NaN")));
        }
        let value =
            attribute(node, attr).ok_or_else(|| invalid(format!("Accuracy/{name}/@{attr}")))?;
        integer(value.trim(), &format!("Accuracy/{name}/@{attr}"))?;
        Ok(Some(value.trim().to_owned()))
    };
    let accuracy = Some(Accuracy {
        epicenters: [rank("Epicenter", "rank")?, rank("Epicenter", "rank2")?],
        depth: rank("Depth", "rank")?,
        magnitude_calculation: rank("MagnitudeCalculation", "rank")?,
        number_of_magnitude_calculation: Some(integer_text(a, "NumberOfMagnitudeCalculation")?),
    });
    let reduce = Some(CodeName {
        code: text(area, SEIS, "ReduceCode")?.into(),
        name: Some(preserved_text(area, SEIS, "ReduceName")?),
    });
    Ok(Earthquake {
        origin_time: optional_time(node, SEIS, "OriginTime")?,
        arrival_time: Some(time(
            &text(node, SEIS, "ArrivalTime")?,
            "Earthquake/ArrivalTime",
        )?),
        condition: optional_text(node, SEIS, "Condition")?,
        hypocenter: Hypocenter {
            area: code_name(area, SEIS)?,
            coordinate,
            depth,
            reduce,
            land_or_sea: optional_text(area, SEIS, "LandOrSea")?,
            accuracy,
        },
        magnitude,
    })
}

fn coordinate(node: Node<'_, '_>) -> Result<(Coordinate, Option<Quantity>), EewError> {
    let content = checked_scalar_text(node)?;
    let raw = content.trim();
    let mut result = Coordinate {
        geodetic_system: attribute(node, "datum").map(str::to_owned),
        condition: attribute(node, "condition").map(str::to_owned),
        ..Coordinate::default()
    };
    if raw.is_empty() || raw == "/" {
        result.condition = Some("不明".into());
        return Ok((result, Some(depth_quantity(None))));
    }
    let raw = raw
        .strip_suffix('/')
        .ok_or_else(|| invalid("Coordinate requires ISO6709 terminal /"))?;
    if !raw.is_ascii() || !raw.starts_with(['+', '-']) {
        return Err(invalid(
            "Coordinate requires signed ASCII ISO6709 components",
        ));
    }
    let starts: Vec<usize> = raw.match_indices(['+', '-']).map(|(i, _)| i).collect();
    if starts.len() < 2 || starts.len() > 3 {
        return Err(invalid("Coordinate requires latitude and longitude"));
    }
    let lat_raw = &raw[..starts[1]];
    let lon_raw = &raw[starts[1]..starts.get(2).copied().unwrap_or(raw.len())];
    let lat = coordinate_number(lat_raw, "latitude")?;
    let lon = coordinate_number(lon_raw, "longitude")?;
    if !coordinate_within_bound(lat_raw, 90) || !coordinate_within_bound(lon_raw, 180) {
        return Err(invalid("Coordinate latitude/longitude out of range"));
    }
    result.latitude = Some(axis(lat, lat_raw, true));
    result.longitude = Some(axis(lon, lon_raw, false));
    let depth = if starts.len() == 3 {
        let height_raw = &raw[starts[2]..];
        let height = coordinate_number(height_raw, "height")?;
        if height > 0.0
            || (height_raw.starts_with('+') && decimal_nonzero(height_raw))
            || !coordinate_within_bound(height_raw, 700_000)
        {
            return Err(invalid(
                "hypocenter height must be -700000..0 metres; 700 km is the lower-bound special value",
            ));
        }
        let value = height_raw
            .strip_prefix('+')
            .unwrap_or(height_raw)
            .to_owned();
        result.height = Some(Quantity {
            kind: "高さ".into(),
            unit: "m".into(),
            bound: None,
            value: Some(value),
            condition: None,
        });
        Some(depth_quantity(Some(depth_kilometres(height_raw))))
    } else {
        Some(depth_quantity(None))
    };
    Ok((result, depth))
}
pub(crate) fn axis(value: f64, raw: &str, latitude: bool) -> CoordinateAxis {
    let suffix = if latitude {
        if value.is_sign_negative() { 'S' } else { 'N' }
    } else if value.is_sign_negative() {
        'W'
    } else {
        'E'
    };
    // Four places is the usual display convention, not a precision limit.
    // Preserve every supplied decimal digit instead of rounding to four places.
    let unsigned = raw.trim_start_matches(['+', '-']);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    let value = format!(
        "{}{whole}.{fraction}{}",
        if value.is_sign_negative() { "-" } else { "" },
        "0".repeat(4usize.saturating_sub(fraction.len()))
    );
    CoordinateAxis {
        text: format!("{unsigned}˚{suffix}"),
        value,
    }
}
pub(crate) fn format_number(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else {
        value.to_string()
    }
}
pub(crate) fn depth_quantity(value: Option<String>) -> Quantity {
    let condition = match value.as_deref() {
        None => Some("不明"),
        Some("0") => Some("ごく浅い"),
        Some("700") => Some("７００ｋｍ以上"),
        _ => None,
    }
    .map(str::to_owned);
    Quantity {
        kind: "深さ".into(),
        unit: "km".into(),
        bound: None,
        value,
        condition,
    }
}

fn intensity(node: Node<'_, '_>, product: Product) -> Result<Intensity, EewError> {
    let forecast = child(node, SEIS, "Forecast")?;
    let observation = child(node, SEIS, "Observation")?;
    if product == Product::RealTimeIntensity && forecast.is_some() {
        return Err(invalid("VXSE47 cannot contain Forecast"));
    }
    if product != Product::RealTimeIntensity && observation.is_some() {
        return Err(invalid("Observation requires VXSE47"));
    }
    if forecast.is_none() && observation.is_none() {
        return Err(invalid("Intensity requires Forecast or Observation"));
    }
    let mut result = Intensity::default();
    if let Some(f) = forecast {
        validate_code_definitions(f, false)?;
        result.forecast_max_int = Some(int_range(required(f, SEIS, "ForecastInt")?)?);
        result.forecast_max_lg_int = child(f, SEIS, "ForecastLgInt")?.map(lg_range).transpose()?;
        result.appendix = child(f, SEIS, "Appendix")?
            .map(|a| {
                Ok::<_, EewError>(Appendix {
                    max_int_change: Some(integer_text(a, "MaxIntChange")?),
                    max_lg_int_change: child(a, SEIS, "MaxLgIntChange")?
                        .map(|_| integer_text(a, "MaxLgIntChange"))
                        .transpose()?,
                    max_int_change_reason: Some(integer_text(a, "MaxIntChangeReason")?),
                })
            })
            .transpose()?;
        let mut regions = Vec::new();
        let mut area_codes = HashSet::new();
        for pref in children(f, SEIS, "Pref") {
            for a in children(pref, SEIS, "Area") {
                let kind = code_name(
                    required(required(a, SEIS, "Category")?, SEIS, "Kind")?,
                    SEIS,
                )?;
                let (is_warning, is_plum) = kind_flags(&kind.code);
                let area = code_name(a, SEIS)?;
                if !area_codes.insert(area.code.clone()) {
                    return Err(invalid("Forecast contains duplicate area codes"));
                }
                let condition = optional_text(a, SEIS, "Condition")?;
                let arrival_time = optional_time(a, SEIS, "ArrivalTime")?;
                match kind.code.as_str() {
                    "01" | "11"
                        if arrival_time.is_some()
                            || condition.as_deref() != Some("既に主要動到達と推測") =>
                    {
                        return Err(invalid(
                            "arrived area requires Condition and no ArrivalTime",
                        ));
                    }
                    "00" | "10" | "09" | "19" if arrival_time.is_none() || condition.is_some() => {
                        return Err(invalid(
                            "unarrived/PLUM area requires ArrivalTime and no Condition",
                        ));
                    }
                    _ => {}
                }
                regions.push(ForecastRegion {
                    area,
                    kind,
                    is_warning,
                    is_plum,
                    forecast_max_int: Some(int_range(required(a, SEIS, "ForecastInt")?)?),
                    forecast_max_lg_int: child(a, SEIS, "ForecastLgInt")?
                        .map(lg_range)
                        .transpose()?,
                    arrival_time_of_day: None,
                    plum_prediction_time_of_day: None,
                    condition,
                    arrival_time,
                });
            }
        }
        result.regions = Some(regions);
        if product == Product::Forecast
            && (has_long_period(&result)
                || result
                    .appendix
                    .as_ref()
                    .is_some_and(|a| a.max_lg_int_change.is_some()))
        {
            return Err(invalid(
                "ForecastLgInt/MaxLgIntChange are only defined for VXSE43/VXSE45",
            ));
        }
    }
    if let Some(o) = observation {
        validate_code_definitions(o, true)?;
        let mut stations = Vec::new();
        let mut station_codes = HashSet::new();
        for p in children(o, SEIS, "Pref") {
            for a in children(p, SEIS, "Area") {
                for c in children(a, SEIS, "City") {
                    for s in children(c, SEIS, "IntensityStation") {
                        // JMA's 2017 real-time-intensity supplement and the
                        // current XSD permit K to be absent. Int remains independent.
                        let k = child(s, SEIS, "K")?
                            .map(|_| {
                                let value = text(s, SEIS, "K")?;
                                number(&value, "IntensityStation/K")?;
                                Ok::<_, EewError>(value.into_owned())
                            })
                            .transpose()?;
                        let int = IntensityValue::from_text(&text(s, SEIS, "Int")?);
                        if matches!(int, IntensityValue::Over) {
                            return Err(invalid("station intensity cannot be over"));
                        }
                        let station = code_name(s, SEIS)?;
                        if !station_codes.insert(station.code.clone()) {
                            return Err(invalid("Observation contains duplicate station codes"));
                        }
                        stations.push(Station { station, int, k });
                    }
                }
            }
        }
        if stations.is_empty() {
            return Err(invalid(
                "Observation requires at least one IntensityStation",
            ));
        }
        result.stations = Some(stations);
    }
    Ok(result)
}
// The code-system declaration determines the meaning of a Code. Check
// explicit declarations for paths whose values we actually interpret. Missing
// declarations and additional extension paths are not synthesized or renamed.
pub(crate) fn validate_code_definitions(
    node: Node<'_, '_>,
    observation: bool,
) -> Result<(), EewError> {
    let Some(definitions) = child(node, SEIS, "CodeDefine")? else {
        return Ok(());
    };
    let expected: &[(&str, &str)] = if observation {
        &[
            ("Pref/Code", "地震情報／都道府県等"),
            ("Pref/Area/Code", "地震情報／細分区域"),
            ("Pref/Area/City/Code", "気象・地震・火山情報／市町村等"),
            (
                "Pref/Area/City/IntensityStation/Code",
                "リアルタイム震度観測点",
            ),
        ]
    } else {
        &[
            ("Pref/Code", "緊急地震速報／府県予報区"),
            ("Pref/Area/Code", "地震情報／細分区域"),
            ("Pref/Area/Category/Kind/Code", "緊急地震速報"),
        ]
    };
    let used: Vec<_> = expected
        .iter()
        .filter(|(path, _)| path_exists(node, path))
        .collect();
    for definition in children(definitions, SEIS, "Type") {
        let Some(path) = attribute(definition, "xpath") else {
            continue;
        };
        if let Some((_, expected)) = used.iter().find(|(known, _)| *known == path.trim()) {
            let actual = checked_scalar_text(definition)?;
            if actual.trim() != *expected {
                return Err(EewError::UnsupportedXml(format!(
                    "CodeDefine {path:?} uses unsupported code system {actual:?}; expected {expected:?}"
                )));
            }
        }
    }
    Ok(())
}
fn path_exists(node: Node<'_, '_>, path: &str) -> bool {
    let (name, rest) = path
        .split_once('/')
        .map_or((path, None), |(name, rest)| (name, Some(rest)));
    children(node, SEIS, name).any(|child| rest.is_none_or(|rest| path_exists(child, rest)))
}
fn has_long_period(intensity: &Intensity) -> bool {
    intensity.forecast_max_lg_int.is_some()
        || intensity
            .regions
            .as_ref()
            .is_some_and(|rs| rs.iter().any(|r| r.forecast_max_lg_int.is_some()))
}
fn int_range(n: Node<'_, '_>) -> Result<ForecastRange<IntensityValue>, EewError> {
    let from = IntensityValue::from_text(&text(n, SEIS, "From")?);
    let to = IntensityValue::from_text(&text(n, SEIS, "To")?);
    if from == IntensityValue::Over {
        return Err(invalid("ForecastInt/From cannot be over"));
    }
    if matches!((from.rank(), to.rank()), (Some(a), Some(b)) if a > b) {
        return Err(invalid("forecast lower bound exceeds upper bound"));
    }
    Ok(ForecastRange { from, to })
}
fn lg_range(n: Node<'_, '_>) -> Result<ForecastRange<LgIntensityValue>, EewError> {
    let from = LgIntensityValue::from_text(&text(n, SEIS, "From")?);
    let to = LgIntensityValue::from_text(&text(n, SEIS, "To")?);
    if from == LgIntensityValue::Over {
        return Err(invalid("ForecastLgInt/From cannot be over"));
    }
    if matches!((from.rank(), to.rank()), (Some(a), Some(b)) if a > b) {
        return Err(invalid("forecast lower bound exceeds upper bound"));
    }
    Ok(ForecastRange { from, to })
}
pub(crate) fn kind_flags(code: &str) -> (Option<bool>, Option<bool>) {
    match code {
        "00" | "01" => (Some(false), Some(false)),
        "09" => (Some(false), Some(true)),
        "10" | "11" => (Some(true), Some(false)),
        "19" => (Some(true), Some(true)),
        _ => (None, None),
    }
}
fn warning_areas(headline: Option<Node<'_, '_>>, body: &mut Body) -> Result<(), EewError> {
    let Some(h) = headline else {
        return Ok(());
    };
    let mut seen_codes: [HashSet<String>; 3] = Default::default();
    for info in children(h, IB, "Information") {
        let (target, seen) = match attribute(info, "type").unwrap_or("") {
            "緊急地震速報（地方予報区）" => (&mut body.zones, &mut seen_codes[0]),
            "緊急地震速報（府県予報区）" => {
                (&mut body.prefectures, &mut seen_codes[1])
            }
            "緊急地震速報（細分区域）" => (&mut body.regions, &mut seen_codes[2]),
            _ => continue,
        };
        for item in children(info, IB, "Item") {
            let kind = code_name(required(item, IB, "Kind")?, IB)?;
            if kind.code == "00" {
                continue;
            }
            let last_kind = child(item, IB, "LastKind")?
                .map(|n| code_name(n, IB))
                .transpose()?;
            for a in children(required(item, IB, "Areas")?, IB, "Area") {
                let area = code_name(a, IB)?;
                let target = target.get_or_insert_with(Vec::new);
                if !seen.insert(area.code.clone()) {
                    return Err(invalid("Headline contains duplicate warning area codes"));
                }
                target.push(WarningArea {
                    area,
                    kind: WarningKind {
                        current: kind.clone(),
                        last_kind: last_kind.clone(),
                    },
                });
            }
        }
    }
    Ok(())
}
fn comments(n: Node<'_, '_>) -> Result<Comments, EewError> {
    let warning = child(n, SEIS, "WarningComment")?
        .map(|w| {
            Ok::<_, EewError>(WarningComment {
                text: preserved_text(w, SEIS, "Text")?,
                codes: text(w, SEIS, "Code")?
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect(),
            })
        })
        .transpose()?;
    Ok(Comments {
        free: optional_text(n, SEIS, "FreeFormComment")?,
        warning,
    })
}
fn code_name(n: Node<'_, '_>, ns: &str) -> Result<CodeName, EewError> {
    Ok(CodeName {
        code: text(n, ns, "Code")?.into(),
        name: Some(preserved_text(n, ns, "Name")?),
    })
}
fn time(raw: &str, field: &str) -> Result<String, EewError> {
    let raw = raw.trim();
    DateTime::parse_from_rfc3339(raw).map_err(|_| invalid(format!("{field} requires RFC3339")))?;
    Ok(raw.to_owned())
}
fn optional_time(n: Node<'_, '_>, ns: &str, name: &str) -> Result<Option<String>, EewError> {
    child(n, ns, name)?
        .map(|node| time(&checked_scalar_text(node)?, name))
        .transpose()
}
// Check decimal bounds before f64 rounding can hide out-of-range trailing digits.
pub(crate) fn coordinate_within_bound(raw: &str, bound: u32) -> bool {
    let unsigned = raw.trim_start_matches(['+', '-']);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let whole = whole.trim_start_matches('0');
    let bound = bound.to_string();
    match whole
        .len()
        .cmp(&bound.len())
        .then_with(|| whole.cmp(bound.as_str()))
    {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Equal => !fraction.bytes().any(|b| b != b'0'),
        std::cmp::Ordering::Greater => false,
    }
}
fn decimal_nonzero(raw: &str) -> bool {
    raw.bytes().any(|b| b.is_ascii_digit() && b != b'0')
}
fn depth_kilometres(height: &str) -> String {
    let unsigned = height.trim_start_matches(['+', '-']);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let padded = format!("{whole:0>4}");
    let (integer, remainder) = padded.split_at(padded.len() - 3);
    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    let fraction = format!("{remainder}{fraction}");
    let fraction = fraction.trim_end_matches('0');
    if fraction.is_empty() {
        integer.to_owned()
    } else {
        format!("{integer}.{fraction}")
    }
}
pub(crate) fn coordinate_number(raw: &str, field: &str) -> Result<f64, EewError> {
    let unsigned = raw
        .strip_prefix(['+', '-'])
        .ok_or_else(|| invalid(format!("{field} requires a sign")))?;
    let (whole, fraction) = unsigned
        .split_once('.')
        .map_or((unsigned, None), |(a, b)| (a, Some(b)));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || fraction.is_some_and(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid(format!(
            "{field} requires ISO6709 decimal notation"
        )));
    }
    number(raw, field)
}
fn integer(raw: &str, field: &str) -> Result<(), EewError> {
    raw.parse::<i32>()
        .map(|_| ())
        .map_err(|_| invalid(format!("{field} requires an XML xs:int")))
}
fn integer_text(node: Node<'_, '_>, name: &str) -> Result<String, EewError> {
    let value = text(node, SEIS, name)?;
    integer(&value, name)?;
    Ok(value.into())
}
fn number(raw: &str, field: &str) -> Result<f64, EewError> {
    raw.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| invalid(format!("{field} requires a finite number")))
}
// Attributes that drive JMA semantics are unqualified. roxmltree 0.21's
// string lookup matches local names in every namespace; never let a foreign
// attribute shadow a protocol attribute (or supply a missing one).
pub(crate) fn attribute<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|a| a.namespace().is_none() && a.name() == name)
        .map(|a| a.value())
}
fn tag(n: Node<'_, '_>, ns: &str, name: &str) -> bool {
    n.is_element() && n.tag_name().namespace() == Some(ns) && n.tag_name().name() == name
}
fn child<'a, 'i>(n: Node<'a, 'i>, ns: &str, name: &str) -> Result<Option<Node<'a, 'i>>, EewError> {
    let mut nodes = n.children().filter(|n| tag(*n, ns, name));
    let first = nodes.next();
    if nodes.next().is_some() {
        return Err(EewError::InvalidXml(format!("duplicate {name} element")));
    }
    Ok(first)
}
fn children<'a, 'i>(
    n: Node<'a, 'i>,
    ns: &'a str,
    name: &'a str,
) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(move |n| tag(*n, ns, name))
}
fn required<'a, 'i>(n: Node<'a, 'i>, ns: &str, name: &str) -> Result<Node<'a, 'i>, EewError> {
    child(n, ns, name)?.ok_or_else(|| invalid(name))
}
fn optional_text(n: Node<'_, '_>, ns: &str, name: &str) -> Result<Option<String>, EewError> {
    child(n, ns, name)?
        .map(|n| checked_scalar_text(n).map(Cow::into_owned))
        .transpose()
        .map(|s| s.filter(|s| !s.trim().is_empty()))
}
fn preserved_text(n: Node<'_, '_>, ns: &str, name: &str) -> Result<String, EewError> {
    optional_text(n, ns, name)?.ok_or_else(|| invalid(name))
}
fn checked_scalar_text<'a>(node: Node<'a, '_>) -> Result<Cow<'a, str>, EewError> {
    if node.children().any(|n| n.is_element()) {
        return Err(invalid(format!(
            "{} must contain simple text",
            node.tag_name().name()
        )));
    }
    Ok(scalar_text(node))
}
fn text<'a>(n: Node<'a, '_>, ns: &str, name: &str) -> Result<Cow<'a, str>, EewError> {
    let value = checked_scalar_text(required(n, ns, name)?)?;
    let value = match value {
        Cow::Borrowed(s) => Cow::Borrowed(s.trim()),
        Cow::Owned(s) => Cow::Owned(s.trim().to_owned()),
    };
    if value.is_empty() {
        return Err(invalid(name));
    }
    Ok(value)
}
/// XML comments and CDATA boundaries do not split a simple-content value.
/// The usual single text node is borrowed directly from the existing DOM.
pub(crate) fn scalar_text<'a>(node: Node<'a, '_>) -> Cow<'a, str> {
    let mut texts = node.children().filter(|n| n.is_text());
    let first = texts.next().and_then(|n| n.text()).unwrap_or_default();
    if texts.next().is_none() {
        return Cow::Borrowed(first);
    }
    Cow::Owned(
        node.children()
            .filter(|n| n.is_text())
            .filter_map(|n| n.text())
            .collect(),
    )
}
fn invalid(field: impl Into<String>) -> EewError {
    EewError::MissingXmlField(field.into())
}

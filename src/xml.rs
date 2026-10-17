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
    let product = Product::from_control_title(control_title);
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
    if !matches!(version, "1.0_0" | "1.1_0" | "1.2_0") {
        return Err(EewError::UnsupportedXml(format!(
            "unsupported InfoKindVersion {version:?}"
        )));
    }
    let status = Status::from_text(text(control, JMX, "Status")?);
    let info_type = InfoType::from_text(text(head, IB, "InfoType")?);
    if let InfoType::Other(value) = &info_type {
        return Err(EewError::UnsupportedXml(format!(
            "unknown InfoType {value:?}: lifecycle cannot be determined"
        )));
    }
    let canceled = info_type == InfoType::Cancellation;
    if product == Product::Test && canceled {
        return Err(EewError::UnsupportedXml(
            "VXSE42 has no cancellation operation".into(),
        ));
    }
    let event_id = optional_text(head, IB, "EventID").map(|s| s.trim().to_owned());
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
    let serial = optional_text(head, IB, "Serial").map(|s| s.trim().to_owned());
    if let Some(s) = &serial {
        let number = s
            .parse::<u32>()
            .map_err(|_| invalid("Head/Serial must be an unsigned integer"))?;
        if number == 0 {
            return Err(invalid("Head/Serial must be positive"));
        }
    }
    let headline = child(head, IB, "Headline");
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
        press_date_time: Some(time(text(control, JMX, "DateTime")?, "Control/DateTime")?),
        report_date_time: Some(time(
            text(head, IB, "ReportDateTime")?,
            "Head/ReportDateTime",
        )?),
        target_date_time: optional_time(head, IB, "TargetDateTime")?,
        event_id,
        serial_no: serial,
        info_kind: Some(info_kind.into()),
        info_kind_version: Some(version.into()),
        headline: headline.and_then(|n| optional_text(n, IB, "Text")),
        target_date_time_dubious: optional_text(head, IB, "TargetDTDubious"),
        target_duration: optional_text(head, IB, "TargetDuration"),
        valid_date_time: optional_time(head, IB, "ValidDateTime")?,
    };
    let next_advisory = optional_text(body_node, SEIS, "NextAdvisory");
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
    let eq_node = child(body_node, SEIS, "Earthquake");
    let int_node = child(body_node, SEIS, "Intensity");
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
        text: optional_text(body_node, SEIS, "Text"),
        comments: child(body_node, SEIS, "Comments")
            .map(comments)
            .transpose()?,
        next_advisory,
        ..Body::default()
    };
    if canceled || product == Product::Test {
        if body.text.is_none() {
            return Err(invalid("Body/Text"));
        }
        if canceled {
            body.comments = None;
        }
    } else {
        if matches!(product, Product::Warning | Product::GroundMotionForecast) {
            warning_areas(headline, &mut body)?;
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
            let raw = m.text().unwrap_or("").trim();
            let value = if raw.is_empty() || raw == "NaN" {
                None
            } else {
                number(raw, "Magnitude")?;
                Some(raw.to_owned())
            };
            Ok::<_, EewError>(Quantity {
                kind: "マグニチュード".into(),
                unit: m
                    .attribute("type")
                    .ok_or_else(|| invalid("Earthquake/Magnitude/@type"))?
                    .into(),
                bound: None,
                condition: if value.is_none() {
                    Some(m.attribute("description").unwrap_or("Ｍ不明").into())
                } else {
                    None
                },
                value,
            })
        })
        .transpose()?;
    let accuracy = child(hyp, SEIS, "Accuracy").map(|a| {
        let rank = |name, attr| {
            child(a, SEIS, name)
                .and_then(|n| n.attribute(attr))
                .map(str::to_owned)
        };
        Accuracy {
            epicenters: [rank("Epicenter", "rank"), rank("Epicenter", "rank2")],
            depth: rank("Depth", "rank"),
            magnitude_calculation: rank("MagnitudeCalculation", "rank"),
            number_of_magnitude_calculation: optional_text(a, SEIS, "NumberOfMagnitudeCalculation"),
        }
    });
    let reduce = optional_text(area, SEIS, "ReduceCode").map(|code| CodeName {
        code,
        name: optional_text(area, SEIS, "ReduceName"),
    });
    Ok(Earthquake {
        origin_time: optional_time(node, SEIS, "OriginTime")?,
        arrival_time: Some(time(
            text(node, SEIS, "ArrivalTime")?,
            "Earthquake/ArrivalTime",
        )?),
        condition: optional_text(node, SEIS, "Condition"),
        hypocenter: Hypocenter {
            area: code_name(area, SEIS)?,
            coordinate,
            depth,
            reduce,
            land_or_sea: optional_text(area, SEIS, "LandOrSea"),
            accuracy,
        },
        magnitude,
    })
}

fn coordinate(node: Node<'_, '_>) -> Result<(Coordinate, Option<Quantity>), EewError> {
    let raw = node.text().unwrap_or("").trim();
    let mut result = Coordinate {
        geodetic_system: node.attribute("datum").map(str::to_owned),
        condition: node.attribute("condition").map(str::to_owned),
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
    let lat = number(lat_raw, "latitude")?;
    let lon = number(lon_raw, "longitude")?;
    if lat.abs() > 90.0 || lon.abs() > 180.0 {
        return Err(invalid("Coordinate latitude/longitude out of range"));
    }
    result.latitude = Some(axis(lat, lat_raw, true));
    result.longitude = Some(axis(lon, lon_raw, false));
    let depth = if starts.len() == 3 {
        let height_raw = &raw[starts[2]..];
        let height = number(height_raw, "height")?;
        if height > 0.0 {
            return Err(invalid("hypocenter height cannot be positive"));
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
        Some(depth_quantity(Some(format_number(-height / 1000.0))))
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
    CoordinateAxis {
        text: format!("{}˚{suffix}", raw.trim_start_matches(['+', '-'])),
        value: format!("{value:.4}"),
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
    let forecast = child(node, SEIS, "Forecast");
    let observation = child(node, SEIS, "Observation");
    if product == Product::RealTimeIntensity && forecast.is_some() {
        return Err(invalid("VXSE47 cannot contain Forecast"));
    }
    if product != Product::RealTimeIntensity && observation.is_some() {
        return Err(invalid("Observation requires VXSE47"));
    }
    let mut result = Intensity::default();
    if let Some(f) = forecast {
        result.forecast_max_int = child(f, SEIS, "ForecastInt").map(int_range).transpose()?;
        result.forecast_max_lg_int = child(f, SEIS, "ForecastLgInt").map(lg_range).transpose()?;
        result.appendix = child(f, SEIS, "Appendix").map(|a| Appendix {
            max_int_change: optional_text(a, SEIS, "MaxIntChange"),
            max_lg_int_change: optional_text(a, SEIS, "MaxLgIntChange"),
            max_int_change_reason: optional_text(a, SEIS, "MaxIntChangeReason"),
        });
        let mut regions = Vec::new();
        for pref in children(f, SEIS, "Pref") {
            for a in children(pref, SEIS, "Area") {
                let kind = code_name(
                    required(required(a, SEIS, "Category")?, SEIS, "Kind")?,
                    SEIS,
                )?;
                let (is_warning, is_plum) = kind_flags(&kind.code);
                regions.push(ForecastRegion {
                    area: code_name(a, SEIS)?,
                    kind,
                    is_warning,
                    is_plum,
                    forecast_max_int: child(a, SEIS, "ForecastInt").map(int_range).transpose()?,
                    forecast_max_lg_int: child(a, SEIS, "ForecastLgInt")
                        .map(lg_range)
                        .transpose()?,
                    arrival_time_of_day: None,
                    condition: optional_text(a, SEIS, "Condition"),
                    arrival_time: optional_time(a, SEIS, "ArrivalTime")?,
                });
            }
        }
        result.regions = Some(regions);
    }
    if let Some(o) = observation {
        let mut stations = Vec::new();
        for p in children(o, SEIS, "Pref") {
            for a in children(p, SEIS, "Area") {
                for c in children(a, SEIS, "City") {
                    for s in children(c, SEIS, "IntensityStation") {
                        let k = text(s, SEIS, "K")?.to_owned();
                        number(&k, "IntensityStation/K")?;
                        let int = IntensityValue::from_text(text(s, SEIS, "Int")?);
                        if matches!(int, IntensityValue::Over) {
                            return Err(invalid("station intensity cannot be over"));
                        }
                        stations.push(Station {
                            station: code_name(s, SEIS)?,
                            int,
                            k: Some(k),
                        });
                    }
                }
            }
        }
        result.stations = Some(stations);
    }
    Ok(result)
}
fn int_range(n: Node<'_, '_>) -> Result<ForecastRange<IntensityValue>, EewError> {
    let from = IntensityValue::from_text(text(n, SEIS, "From")?);
    let to = IntensityValue::from_text(text(n, SEIS, "To")?);
    if from == IntensityValue::Over {
        return Err(invalid("ForecastInt/From cannot be over"));
    }
    if matches!((from.rank(), to.rank()), (Some(a), Some(b)) if a > b) {
        return Err(invalid("forecast lower bound exceeds upper bound"));
    }
    Ok(ForecastRange { from, to })
}
fn lg_range(n: Node<'_, '_>) -> Result<ForecastRange<LgIntensityValue>, EewError> {
    let from = LgIntensityValue::from_text(text(n, SEIS, "From")?);
    let to = LgIntensityValue::from_text(text(n, SEIS, "To")?);
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
    for info in children(h, IB, "Information") {
        let target = match info.attribute("type").unwrap_or("") {
            "緊急地震速報（地方予報区）" => &mut body.zones,
            "緊急地震速報（府県予報区）" => &mut body.prefectures,
            "緊急地震速報（細分区域）" => &mut body.regions,
            _ => continue,
        };
        for item in children(info, IB, "Item") {
            let kind = code_name(required(item, IB, "Kind")?, IB)?;
            if kind.code == "00" {
                continue;
            }
            let last_kind = child(item, IB, "LastKind")
                .map(|n| code_name(n, IB))
                .transpose()?;
            for a in children(required(item, IB, "Areas")?, IB, "Area") {
                target.get_or_insert_with(Vec::new).push(WarningArea {
                    area: code_name(a, IB)?,
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
    let warning = child(n, SEIS, "WarningComment").map(|w| WarningComment {
        text: optional_text(w, SEIS, "Text").unwrap_or_default(),
        codes: children(w, SEIS, "Code")
            .filter_map(|n| n.text())
            .flat_map(str::split_whitespace)
            .map(str::to_owned)
            .collect(),
    });
    Ok(Comments {
        free: optional_text(n, SEIS, "FreeFormComment"),
        warning,
    })
}
fn code_name(n: Node<'_, '_>, ns: &str) -> Result<CodeName, EewError> {
    Ok(CodeName {
        code: text(n, ns, "Code")?.into(),
        name: optional_text(n, ns, "Name"),
    })
}
fn time(raw: &str, field: &str) -> Result<String, EewError> {
    DateTime::parse_from_rfc3339(raw).map_err(|_| invalid(format!("{field} requires RFC3339")))?;
    Ok(raw.to_owned())
}
fn optional_time(n: Node<'_, '_>, ns: &str, name: &str) -> Result<Option<String>, EewError> {
    optional_text(n, ns, name)
        .map(|s| time(&s, name))
        .transpose()
}
fn number(raw: &str, field: &str) -> Result<f64, EewError> {
    raw.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| invalid(format!("{field} requires a finite number")))
}
fn tag(n: Node<'_, '_>, ns: &str, name: &str) -> bool {
    n.is_element() && n.tag_name().namespace() == Some(ns) && n.tag_name().name() == name
}
fn child<'a, 'i>(n: Node<'a, 'i>, ns: &str, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|n| tag(*n, ns, name))
}
fn children<'a, 'i>(
    n: Node<'a, 'i>,
    ns: &'a str,
    name: &'a str,
) -> impl Iterator<Item = Node<'a, 'i>> {
    n.children().filter(move |n| tag(*n, ns, name))
}
fn required<'a, 'i>(n: Node<'a, 'i>, ns: &str, name: &str) -> Result<Node<'a, 'i>, EewError> {
    let mut nodes = n.children().filter(|n| tag(*n, ns, name));
    let first = nodes.next().ok_or_else(|| invalid(name))?;
    if nodes.next().is_some() {
        return Err(EewError::UnsupportedXml(format!(
            "multiple {name} elements require an explicit selection; raw input remains with caller"
        )));
    }
    Ok(first)
}
fn optional_text(n: Node<'_, '_>, ns: &str, name: &str) -> Option<String> {
    child(n, ns, name)
        .and_then(|n| n.text())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
}
fn text<'a>(n: Node<'a, '_>, ns: &str, name: &str) -> Result<&'a str, EewError> {
    child(n, ns, name)
        .and_then(|n| n.text())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(name))
}
fn invalid(field: impl Into<String>) -> EewError {
    EewError::MissingXmlField(field.into())
}

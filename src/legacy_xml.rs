//! Legacy encoder adapter; never used by the semantic parsing API.
use chrono::{DateTime, Datelike, FixedOffset};
use roxmltree::{Document, Node};

use crate::error::AppError;

use crate::xml::{NS_ELEMENT, NS_INFO, NS_REPORT, NS_SEIS};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XmlProduct {
    Forecast,
    Warning,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InfoType {
    Publication,
    Correction,
    Cancellation,
}

#[derive(Debug)]
pub struct EewXmlDocument {
    pub product: XmlProduct,
    pub xml_type: String,
    pub control_time: DateTime<FixedOffset>,
    pub status: String,
    pub editorial_office: String,
    pub event_id: String,
    pub serial: u16,
    pub info_type: InfoType,
    pub final_report: bool,
    pub earthquake: Option<Earthquake>,
    pub forecast: Option<Forecast>,
    pub warning_areas: WarningAreas,
}

#[derive(Debug)]
pub struct Earthquake {
    pub origin_time: Option<DateTime<FixedOffset>>,
    pub arrival_time: DateTime<FixedOffset>,
    pub assumed_hypocenter: bool,
    pub epicenter_code: String,
    pub reduced_epicenter_code: Option<String>,
    pub coordinate: Coordinate,
    pub land_or_sea: Option<String>,
    pub magnitude: Option<f64>,
    pub accuracy: Accuracy,
}

#[derive(Debug)]
pub struct Coordinate {
    pub latitude: f64,
    pub longitude: f64,
    pub depth_km: Option<i64>,
}

#[derive(Debug)]
pub struct Accuracy {
    pub epicenter_rank: u8,
    pub epicenter_rank2: u8,
    pub depth_rank: u8,
    pub magnitude_rank: u8,
    pub magnitude_stations: u8,
}

#[derive(Debug, Default)]
pub struct Forecast {
    pub maximum_intensity: Option<IntensityRange>,
    pub _maximum_long_period: Option<IntensityRange>,
    pub max_int_change: Option<u8>,
    pub _max_long_period_change: Option<u8>,
    pub max_int_change_reason: Option<u8>,
    pub areas: Vec<ForecastArea>,
}

#[derive(Clone, Debug)]
pub struct IntensityRange {
    pub from: String,
    pub to: String,
}

#[derive(Debug)]
pub struct ForecastArea {
    pub code: String,
    pub kind_code: String,
    pub intensity: IntensityRange,
    pub _long_period: Option<IntensityRange>,
    pub arrival_time: Option<DateTime<FixedOffset>>,
    pub condition: Option<String>,
}

#[derive(Debug, Default)]
pub struct WarningAreas {
    pub current_districts: Vec<String>,
    pub current_prefectures: Vec<String>,
    pub current_regions: Vec<String>,
    pub added_districts: Vec<String>,
    pub added_prefectures: Vec<String>,
    pub added_regions: Vec<String>,
}

pub fn parse(xml: &str) -> Result<EewXmlDocument, AppError> {
    let xml = xml.strip_prefix('\u{feff}').unwrap_or(xml);
    let document = Document::parse(xml).map_err(|error| AppError::InvalidXml(error.to_string()))?;
    let root = document.root_element();
    if root.tag_name().name() != "Report" || root.tag_name().namespace() != Some(NS_REPORT) {
        return Err(AppError::UnsupportedXml(
            "root must be jmaxml1:Report in the JMA namespace".to_string(),
        ));
    }

    let control = required_child(root, NS_REPORT, "Control")?;
    let head = required_child(root, NS_INFO, "Head")?;
    let body = required_child(root, NS_SEIS, "Body")?;

    let control_title = required_text(control, NS_REPORT, "Title")?;
    let head_title = required_text(head, NS_INFO, "Title")?;
    if control_title != head_title {
        return Err(AppError::UnsupportedXml(format!(
            "Control/Title {control_title:?} differs from Head/Title {head_title:?}"
        )));
    }
    let info_kind = required_text(head, NS_INFO, "InfoKind")?;
    if info_kind != "緊急地震速報" {
        return Err(AppError::UnsupportedXml(format!(
            "Head/InfoKind is {info_kind:?}, not 緊急地震速報"
        )));
    }
    let info_kind_version = required_text(head, NS_INFO, "InfoKindVersion")?.to_string();
    if !matches!(info_kind_version.as_str(), "1.0_0" | "1.1_0" | "1.2_0") {
        return Err(AppError::UnsupportedXml(format!(
            "unsupported Head/InfoKindVersion {info_kind_version:?}"
        )));
    }

    let info_type = match required_text(head, NS_INFO, "InfoType")? {
        "発表" => InfoType::Publication,
        "訂正" => InfoType::Correction,
        "取消" => InfoType::Cancellation,
        value => {
            return Err(AppError::UnsupportedXml(format!(
                "unsupported Head/InfoType {value:?}"
            )));
        }
    };
    let (product, xml_type) = match head_title {
        "緊急地震速報（警報）" => (XmlProduct::Warning, "VXSE43"),
        "緊急地震速報（予報）" => (XmlProduct::Forecast, "VXSE44"),
        "緊急地震速報（地震動予報）" => (XmlProduct::Forecast, "VXSE45"),
        _ if info_type == InfoType::Cancellation => (XmlProduct::Unknown, "unknown"),
        _ => {
            return Err(AppError::UnsupportedXml(format!(
                "unsupported Head/Title {head_title:?}"
            )));
        }
    };
    let serial = required_text(head, NS_INFO, "Serial")?
        .parse::<u16>()
        .map_err(|_| missing("Head/Serial must be an integer"))?;
    if serial == 0 || serial > 359 {
        return Err(missing("Head/Serial must be in 1..=359"));
    }

    let event_id = required_text(head, NS_INFO, "EventID")?.to_string();
    if event_id.len() != 14 || !event_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(missing("Head/EventID must contain exactly 14 digits"));
    }

    let control_time = parse_time(
        required_text(control, NS_REPORT, "DateTime")?,
        "Control/DateTime",
    )?;
    let report_datetime = required_text(head, NS_INFO, "ReportDateTime")?.to_string();
    parse_time(&report_datetime, "Head/ReportDateTime")?;
    let target_datetime = required_text(head, NS_INFO, "TargetDateTime")?.to_string();
    parse_time(&target_datetime, "Head/TargetDateTime")?;
    let status = required_text(control, NS_REPORT, "Status")?.to_string();
    if !matches!(status.as_str(), "通常" | "訓練" | "試験") {
        return Err(AppError::Unsupported {
            event_id: Some(event_id.clone()),
            xml_type: Some(xml_type.to_string()),
            info_type: Some(info_type.as_xml_value().to_string()),
            field: Some("Control/Status".to_string()),
            reason: format!("unsupported value {status:?}"),
        });
    }
    if status == "試験" {
        return Err(AppError::Unsupported {
            event_id: Some(event_id.clone()),
            xml_type: Some(xml_type.to_string()),
            info_type: Some(info_type.as_xml_value().to_string()),
            field: Some("Control/Status".to_string()),
            reason: "status 試験 cannot be mapped uniquely to Telegram message type 20 or 30"
                .to_string(),
        });
    }
    if info_type == InfoType::Correction {
        return Err(AppError::Unsupported {
            event_id: Some(event_id.clone()),
            xml_type: Some(xml_type.to_string()),
            info_type: Some(info_type.as_xml_value().to_string()),
            field: Some("Head/InfoType".to_string()),
            reason: "訂正 has no corresponding state in the current code Telegram fields"
                .to_string(),
        });
    }

    let earthquake_node = child(body, NS_SEIS, "Earthquake");
    let forecast_node =
        child(body, NS_SEIS, "Intensity").and_then(|node| child(node, NS_SEIS, "Forecast"));
    if info_type == InfoType::Cancellation {
        if earthquake_node.is_some() || forecast_node.is_some() {
            return Err(AppError::InvalidXml(
                "a cancellation EEW must not contain Earthquake or Intensity/Forecast".to_string(),
            ));
        }
        required_text(body, NS_SEIS, "Text")?;
    } else if earthquake_node.is_none() {
        return Err(missing("Body/Earthquake"));
    }

    let earthquake = earthquake_node.map(parse_earthquake).transpose()?;
    let forecast = forecast_node.map(parse_forecast).transpose()?;
    let warning_areas = parse_warning_areas(head)?;
    let final_report = child(body, NS_SEIS, "NextAdvisory")
        .and_then(|node| node.text())
        .is_some_and(|text| text.contains("最終報"));

    Ok(EewXmlDocument {
        product,
        xml_type: xml_type.to_string(),
        control_time,
        status,
        editorial_office: required_text(control, NS_REPORT, "EditorialOffice")?.to_string(),
        event_id,
        serial,
        info_type,
        final_report,
        earthquake,
        forecast,
        warning_areas,
    })
}

impl InfoType {
    pub fn as_xml_value(self) -> &'static str {
        match self {
            Self::Publication => "発表",
            Self::Correction => "訂正",
            Self::Cancellation => "取消",
        }
    }
}

impl EewXmlDocument {
    pub fn unsupported(&self, field: impl Into<String>, reason: impl Into<String>) -> AppError {
        AppError::Unsupported {
            event_id: Some(self.event_id.clone()),
            xml_type: Some(self.xml_type.clone()),
            info_type: Some(self.info_type.as_xml_value().to_string()),
            field: Some(field.into()),
            reason: reason.into(),
        }
    }
}

fn parse_earthquake(node: Node<'_, '_>) -> Result<Earthquake, AppError> {
    let origin_time = child(node, NS_SEIS, "OriginTime")
        .and_then(|node| node.text())
        .map(|value| parse_time(value, "Body/Earthquake/OriginTime"))
        .transpose()?;
    let arrival_time = parse_time(
        required_text(node, NS_SEIS, "ArrivalTime")?,
        "Body/Earthquake/ArrivalTime",
    )?;
    let assumed_hypocenter = child(node, NS_SEIS, "Condition")
        .and_then(|node| node.text())
        .is_some_and(|condition| condition == "仮定震源要素");

    let hypocenter = required_child(node, NS_SEIS, "Hypocenter")?;
    let area = required_child(hypocenter, NS_SEIS, "Area")?;
    let epicenter_code = required_text(area, NS_SEIS, "Code")?.to_string();
    if epicenter_code.len() != 3 || !epicenter_code.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(missing(
            "Body/Earthquake/Hypocenter/Area/Code must be 3 digits",
        ));
    }
    let reduced_epicenter_code = child(area, NS_SEIS, "ReduceCode")
        .and_then(|node| node.text())
        .map(str::to_string);
    if reduced_epicenter_code
        .as_deref()
        .is_some_and(|code| code.len() != 4 || !code.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(missing(
            "Body/Earthquake/Hypocenter/Area/ReduceCode must be 4 digits",
        ));
    }
    let coordinate = child(area, NS_ELEMENT, "Coordinate")
        .ok_or_else(|| missing("Body/Earthquake/Hypocenter/Area/Coordinate"))?;
    let coordinate = parse_coordinate(coordinate.text().unwrap_or_default())?;
    let land_or_sea = child(area, NS_SEIS, "LandOrSea")
        .and_then(|node| node.text())
        .map(str::to_string);
    if land_or_sea
        .as_deref()
        .is_some_and(|value| !matches!(value, "内陸" | "海域"))
    {
        return Err(missing("Body/Earthquake/Hypocenter/Area/LandOrSea"));
    }

    let accuracy = required_child(hypocenter, NS_SEIS, "Accuracy")?;
    let epicenter = required_child(accuracy, NS_SEIS, "Epicenter")?;
    let depth = required_child(accuracy, NS_SEIS, "Depth")?;
    let magnitude_calculation = required_child(accuracy, NS_SEIS, "MagnitudeCalculation")?;
    let accuracy = Accuracy {
        epicenter_rank: parse_attribute_u8(epicenter, "rank", "Accuracy/Epicenter/@rank")?,
        epicenter_rank2: parse_attribute_u8(epicenter, "rank2", "Accuracy/Epicenter/@rank2")?,
        depth_rank: parse_attribute_u8(depth, "rank", "Accuracy/Depth/@rank")?,
        magnitude_rank: parse_attribute_u8(
            magnitude_calculation,
            "rank",
            "Accuracy/MagnitudeCalculation/@rank",
        )?,
        magnitude_stations: required_text(accuracy, NS_SEIS, "NumberOfMagnitudeCalculation")?
            .parse()
            .map_err(|_| missing("Accuracy/NumberOfMagnitudeCalculation"))?,
    };
    if !matches!(accuracy.epicenter_rank, 0..=8)
        || !matches!(accuracy.epicenter_rank2, 0..=4 | 9)
        || !matches!(accuracy.depth_rank, 0..=8)
        || !matches!(accuracy.magnitude_rank, 0 | 2..=6 | 8)
        || !matches!(accuracy.magnitude_stations, 0..=5)
    {
        return Err(missing(
            "Earthquake/Hypocenter/Accuracy contains an undefined rank",
        ));
    }

    let magnitude =
        child(node, NS_ELEMENT, "Magnitude").ok_or_else(|| missing("Body/Earthquake/Magnitude"))?;
    let magnitude = match magnitude.text().unwrap_or_default().trim() {
        "NaN" | "" => None,
        value => Some(
            value
                .parse::<f64>()
                .map_err(|_| missing("Body/Earthquake/Magnitude"))?,
        ),
    };
    if assumed_hypocenter
        && (coordinate.depth_km != Some(10)
            || land_or_sea.is_some()
            || magnitude != Some(1.0)
            || accuracy.epicenter_rank != 1
            || !matches!(accuracy.epicenter_rank2, 1 | 9)
            || accuracy.depth_rank != 1
            || accuracy.magnitude_rank != 8
            || accuracy.magnitude_stations != 1)
    {
        return Err(missing(
            "an assumed hypocenter must use the official depth, magnitude, land/sea, and accuracy values",
        ));
    }

    Ok(Earthquake {
        origin_time,
        arrival_time,
        assumed_hypocenter,
        epicenter_code,
        reduced_epicenter_code,
        coordinate,
        land_or_sea,
        magnitude,
        accuracy,
    })
}

fn parse_forecast(node: Node<'_, '_>) -> Result<Forecast, AppError> {
    let mut result = Forecast {
        maximum_intensity: child(node, NS_SEIS, "ForecastInt")
            .map(parse_intensity)
            .transpose()?,
        _maximum_long_period: child(node, NS_SEIS, "ForecastLgInt")
            .map(parse_long_period)
            .transpose()?,
        ..Forecast::default()
    };

    if let Some(appendix) = child(node, NS_SEIS, "Appendix") {
        result.max_int_change = optional_u8(appendix, "MaxIntChange")?;
        result._max_long_period_change = optional_u8(appendix, "MaxLgIntChange")?;
        result.max_int_change_reason = optional_u8(appendix, "MaxIntChangeReason")?;
        if result.max_int_change.is_some_and(|value| value > 2)
            || result
                ._max_long_period_change
                .is_some_and(|value| value > 2)
            || result
                .max_int_change_reason
                .is_some_and(|value| !matches!(value, 0..=4 | 9))
        {
            return Err(missing("Forecast/Appendix contains an undefined code"));
        }
    }

    for pref in children(node, NS_SEIS, "Pref") {
        for area in children(pref, NS_SEIS, "Area") {
            let category = required_child(area, NS_SEIS, "Category")?;
            let kind = required_child(category, NS_SEIS, "Kind")?;
            let kind_code = required_text(kind, NS_SEIS, "Code")?.to_string();
            if kind_code.len() != 2 || !kind_code.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(missing(
                    "Forecast/Pref/Area/Category/Kind/Code must be 2 digits",
                ));
            }
            let code = required_text(area, NS_SEIS, "Code")?.to_string();
            if code.len() != 3 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(missing("Forecast/Pref/Area/Code must be 3 digits"));
            }
            result.areas.push(ForecastArea {
                code,
                kind_code,
                intensity: parse_intensity(required_child(area, NS_SEIS, "ForecastInt")?)?,
                _long_period: child(area, NS_SEIS, "ForecastLgInt")
                    .map(parse_long_period)
                    .transpose()?,
                arrival_time: child(area, NS_SEIS, "ArrivalTime")
                    .and_then(|node| node.text())
                    .map(|value| parse_time(value, "Forecast/Pref/Area/ArrivalTime"))
                    .transpose()?,
                condition: child(area, NS_SEIS, "Condition")
                    .and_then(|node| node.text())
                    .map(str::to_string),
            });
        }
    }
    Ok(result)
}

fn parse_intensity(node: Node<'_, '_>) -> Result<IntensityRange, AppError> {
    let range = IntensityRange {
        from: required_text(node, NS_SEIS, "From")?.to_string(),
        to: required_text(node, NS_SEIS, "To")?.to_string(),
    };
    const FROM: &[&str] = &["0", "1", "2", "3", "4", "5-", "5+", "6-", "6+", "7", "不明"];
    const TO: &[&str] = &[
        "0", "1", "2", "3", "4", "5-", "5+", "6-", "6+", "7", "over", "不明",
    ];
    if !FROM.contains(&range.from.as_str()) || !TO.contains(&range.to.as_str()) {
        return Err(missing("ForecastInt contains an unsupported intensity"));
    }
    if range.to != "over"
        && range.from != "不明"
        && range.to != "不明"
        && intensity_rank(&range.from) > intensity_rank(&range.to)
    {
        return Err(missing("ForecastInt lower bound exceeds upper bound"));
    }
    Ok(range)
}

fn parse_long_period(node: Node<'_, '_>) -> Result<IntensityRange, AppError> {
    let range = IntensityRange {
        from: required_text(node, NS_SEIS, "From")?.to_string(),
        to: required_text(node, NS_SEIS, "To")?.to_string(),
    };
    const FROM: &[&str] = &["0", "1", "2", "3", "4", "不明"];
    const TO: &[&str] = &["0", "1", "2", "3", "4", "over", "不明"];
    if !FROM.contains(&range.from.as_str()) || !TO.contains(&range.to.as_str()) {
        return Err(missing("ForecastLgInt contains an unsupported class"));
    }
    if range.to != "over"
        && range.from != "不明"
        && range.to != "不明"
        && intensity_rank(&range.from) > intensity_rank(&range.to)
    {
        return Err(missing("ForecastLgInt lower bound exceeds upper bound"));
    }
    Ok(range)
}

fn intensity_rank(value: &str) -> u8 {
    match value {
        "0" => 0,
        "1" => 1,
        "2" => 2,
        "3" => 3,
        "4" => 4,
        "5-" => 5,
        "5+" => 6,
        "6-" => 7,
        "6+" => 8,
        "7" => 9,
        _ => 0,
    }
}

fn parse_warning_areas(head: Node<'_, '_>) -> Result<WarningAreas, AppError> {
    let mut result = WarningAreas::default();
    let Some(headline) = child(head, NS_INFO, "Headline") else {
        return Ok(result);
    };
    for information in children(headline, NS_INFO, "Information") {
        let area_type = information.attribute("type").unwrap_or_default();
        for item in children(information, NS_INFO, "Item") {
            let kind = required_child(item, NS_INFO, "Kind")?;
            if required_text(kind, NS_INFO, "Code")? != "31" {
                continue;
            }
            let last_kind = required_child(item, NS_INFO, "LastKind")?;
            let is_added = required_text(last_kind, NS_INFO, "Code")? == "00";
            let areas = required_child(item, NS_INFO, "Areas")?;
            for area in children(areas, NS_INFO, "Area") {
                let code = required_text(area, NS_INFO, "Code")?.to_string();
                let (current, added, width) = if area_type.contains("地方予報区") {
                    (
                        &mut result.current_districts,
                        &mut result.added_districts,
                        4,
                    )
                } else if area_type.contains("府県予報区") {
                    (
                        &mut result.current_prefectures,
                        &mut result.added_prefectures,
                        4,
                    )
                } else if area_type.contains("細分区域") {
                    (&mut result.current_regions, &mut result.added_regions, 3)
                } else {
                    continue;
                };
                if code.len() != width || !code.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(missing(
                        "Headline/Information/Areas/Area/Code has invalid width",
                    ));
                }
                current.push(code.clone());
                if is_added {
                    added.push(code);
                }
            }
        }
    }
    Ok(result)
}

fn parse_coordinate(value: &str) -> Result<Coordinate, AppError> {
    let value = value.trim().strip_suffix('/').unwrap_or(value.trim());
    if value.is_empty() {
        return Err(missing(
            "Coordinate is empty (all hypocenter elements are unknown)",
        ));
    }
    if !value.is_ascii() || !matches!(value.as_bytes().first(), Some(b'+' | b'-')) {
        return Err(missing(
            "Coordinate must use the ISO 6709 signed ASCII form",
        ));
    }
    let second = value[1..]
        .find(['+', '-'])
        .map(|index| index + 1)
        .ok_or_else(|| missing("Coordinate longitude"))?;
    let third = value[second + 1..]
        .find(['+', '-'])
        .map(|index| second + 1 + index);
    let latitude = value[..second]
        .parse::<f64>()
        .map_err(|_| missing("Coordinate latitude"))?;
    let longitude_end = third.unwrap_or(value.len());
    let longitude = value[second..longitude_end]
        .parse::<f64>()
        .map_err(|_| missing("Coordinate longitude"))?;
    if !latitude.is_finite()
        || !longitude.is_finite()
        || latitude.abs() > 90.0
        || longitude.abs() > 180.0
    {
        return Err(missing("Coordinate latitude or longitude is out of range"));
    }
    let depth_km = third
        .map(|index| {
            let signed_meters = value[index..]
                .parse::<i64>()
                .map_err(|_| missing("Coordinate depth"))?;
            if signed_meters > 0 {
                return Err(missing(
                    "Coordinate depth must use a negative value or the +0 shallow-depth special value",
                ));
            }
            let meters = signed_meters
                .checked_abs()
                .ok_or_else(|| missing("Coordinate depth is out of range"))?;
            if meters > 700_000 {
                return Err(missing("Coordinate depth exceeds the 700 km special value"));
            }
            if meters < 700_000 && meters % 10_000 != 0 {
                return Err(missing(
                    "Coordinate depth below 700 km must use the official 10 km precision",
                ));
            }
            Ok(meters / 1000)
        })
        .transpose()?;
    Ok(Coordinate {
        latitude,
        longitude,
        depth_km,
    })
}

fn parse_time(value: &str, field: &str) -> Result<DateTime<FixedOffset>, AppError> {
    let time = DateTime::parse_from_rfc3339(value)
        .map_err(|_| missing(format!("{field} must be RFC 3339")))?;
    if !(2000..=2099).contains(&time.year()) {
        return Err(missing(format!(
            "{field} year cannot be represented by Telegram's two-digit year"
        )));
    }
    Ok(time)
}

fn parse_attribute_u8(node: Node<'_, '_>, attribute: &str, field: &str) -> Result<u8, AppError> {
    node.attribute(attribute)
        .ok_or_else(|| missing(field))?
        .parse()
        .map_err(|_| missing(field))
}

fn optional_u8(node: Node<'_, '_>, name: &str) -> Result<Option<u8>, AppError> {
    child(node, NS_SEIS, name)
        .and_then(|node| node.text())
        .map(|value| {
            let value = value
                .parse()
                .map_err(|_| missing(format!("Appendix/{name}")))?;
            if value > 9 {
                return Err(missing(format!("Appendix/{name} must be one digit")));
            }
            Ok(value)
        })
        .transpose()
}

fn child<'a, 'input>(
    node: Node<'a, 'input>,
    namespace: &str,
    name: &str,
) -> Option<Node<'a, 'input>> {
    node.children().find(|child| {
        child.is_element()
            && child.tag_name().namespace() == Some(namespace)
            && child.tag_name().name() == name
    })
}

fn children<'a, 'input>(
    node: Node<'a, 'input>,
    namespace: &'a str,
    name: &'a str,
) -> impl Iterator<Item = Node<'a, 'input>> {
    node.children().filter(move |child| {
        child.is_element()
            && child.tag_name().namespace() == Some(namespace)
            && child.tag_name().name() == name
    })
}

fn required_child<'a, 'input>(
    node: Node<'a, 'input>,
    namespace: &str,
    name: &str,
) -> Result<Node<'a, 'input>, AppError> {
    child(node, namespace, name).ok_or_else(|| missing(name))
}

fn required_text<'a>(node: Node<'a, '_>, namespace: &str, name: &str) -> Result<&'a str, AppError> {
    child(node, namespace, name)
        .and_then(|node| node.text())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| missing(name))
}

fn missing(field: impl Into<String>) -> AppError {
    AppError::MissingXmlField(field.into())
}

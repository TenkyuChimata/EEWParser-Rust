//! Legacy encoder adapter; never used by the semantic parsing API.
use chrono::{DateTime, FixedOffset, Timelike};
use roxmltree::{Document, Node, ParsingOptions};
use std::{borrow::Cow, collections::HashSet};

use crate::error::AppError;

use crate::xml::{NS_ELEMENT, NS_INFO, NS_REPORT, NS_SEIS};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XmlProduct {
    Forecast,
    Warning,
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
    let xml = xml.strip_prefix('\u{feff}').unwrap_or(xml).trim_start();
    crate::input::check_xml_depth(xml)?;
    let document = Document::parse_with_options(
        xml,
        ParsingOptions {
            nodes_limit: 250_000,
            ..ParsingOptions::default()
        },
    )
    .map_err(|error| AppError::InvalidXml(error.to_string()))?;
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
    // Control identifies the product; Head/Title is its presentation title.
    required_text(head, NS_INFO, "Title")?;
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

    let info_type = match required_text(head, NS_INFO, "InfoType")?.as_ref() {
        "発表" => InfoType::Publication,
        "訂正" => InfoType::Correction,
        "取消" => InfoType::Cancellation,
        value => {
            return Err(AppError::UnsupportedXml(format!(
                "unsupported Head/InfoType {value:?}"
            )));
        }
    };
    let (product, xml_type) = match control_title.as_ref() {
        "緊急地震速報（警報）" => (XmlProduct::Warning, "VXSE43"),
        "緊急地震速報（予報）" => (XmlProduct::Forecast, "VXSE44"),
        "緊急地震速報（地震動予報）" => (XmlProduct::Forecast, "VXSE45"),
        _ => {
            return Err(AppError::UnsupportedXml(format!(
                "unsupported Control/Title {control_title:?}"
            )));
        }
    };
    let serial = required_text(head, NS_INFO, "Serial")?
        .parse::<u32>()
        .map_err(|_| missing("Head/Serial must be an integer"))?;
    if serial == 0 {
        return Err(missing("Head/Serial must be positive"));
    }
    if serial > 359 {
        return Err(unavailable(
            "Head/Serial",
            "serial cannot be represented in Telegram's 01..Z9 range",
        ));
    }
    let serial = serial as u16;

    let event_id = required_text(head, NS_INFO, "EventID")?.to_string();
    if event_id.len() != 14 || !event_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(missing("Head/EventID must contain exactly 14 digits"));
    }

    let control_time = parse_time(
        &required_text(control, NS_REPORT, "DateTime")?,
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

    let earthquake_node = child(body, NS_SEIS, "Earthquake")?;
    let intensity_node = child(body, NS_SEIS, "Intensity")?;
    if let Some(intensity) = intensity_node
        && child(intensity, NS_SEIS, "Observation")?.is_some()
    {
        return Err(missing("Observation is only defined for VXSE47"));
    }
    let forecast_node = intensity_node
        .map(|node| child(node, NS_SEIS, "Forecast"))
        .transpose()?
        .flatten();
    if info_type == InfoType::Cancellation {
        if earthquake_node.is_some() || intensity_node.is_some() {
            return Err(AppError::InvalidXml(
                "a cancellation EEW must not contain Earthquake or Intensity/Forecast".to_string(),
            ));
        }
        required_text(body, NS_SEIS, "Text")?;
    } else if earthquake_node.is_none() {
        return Err(missing("Body/Earthquake"));
    }

    let context = |error| match error {
        AppError::Unsupported { field, reason, .. } => AppError::Unsupported {
            event_id: Some(event_id.clone()),
            xml_type: Some(xml_type.to_string()),
            info_type: Some(info_type.as_xml_value().to_string()),
            field,
            reason,
        },
        other => other,
    };
    let earthquake = earthquake_node
        .map(parse_earthquake)
        .transpose()
        .map_err(context)?;
    let forecast = forecast_node
        .map(parse_forecast)
        .transpose()
        .map_err(context)?;
    if xml_type == "VXSE44"
        && forecast.as_ref().is_some_and(|f| {
            f._maximum_long_period.is_some()
                || f._max_long_period_change.is_some()
                || f.areas.iter().any(|a| a._long_period.is_some())
        })
    {
        return Err(missing(
            "ForecastLgInt/MaxLgIntChange are only defined for VXSE43/VXSE45",
        ));
    }
    if earthquake
        .as_ref()
        .is_some_and(|eq| eq.coordinate.depth_km.is_some_and(|d| d > 150))
        && forecast.as_ref().is_some_and(|f| {
            f._maximum_long_period.is_some() || f.areas.iter().any(|a| a._long_period.is_some())
        })
    {
        return Err(missing(
            "ForecastLgInt is not defined for hypocenters deeper than 150 km",
        ));
    }
    let warning_areas = parse_warning_areas(head).map_err(context)?;
    let final_report = match optional_text(body, NS_SEIS, "NextAdvisory")?.as_deref() {
        None => false,
        Some("この情報をもって、緊急地震速報：最終報とします。") => true,
        Some(_) => {
            return Err(missing(
                "Body/NextAdvisory must use the official final-report text",
            ));
        }
    };

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
    let origin_time = optional_text(node, NS_SEIS, "OriginTime")?
        .map(|value| parse_time(&value, "Body/Earthquake/OriginTime"))
        .transpose()?;
    let arrival_time = parse_time(
        &required_text(node, NS_SEIS, "ArrivalTime")?,
        "Body/Earthquake/ArrivalTime",
    )?;
    let assumed_hypocenter = match optional_text(node, NS_SEIS, "Condition")?.as_deref() {
        None => false,
        Some("仮定震源要素") => true,
        Some(_) => return Err(missing("Earthquake/Condition must be 仮定震源要素")),
    };
    if assumed_hypocenter && origin_time.is_none() {
        return Err(missing("an assumed hypocenter requires OriginTime"));
    }

    let hypocenter = required_child(node, NS_SEIS, "Hypocenter")?;
    let area = required_child(hypocenter, NS_SEIS, "Area")?;
    let epicenter_code = required_text(area, NS_SEIS, "Code")?.to_string();
    if epicenter_code.len() != 3 || !epicenter_code.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(missing(
            "Body/Earthquake/Hypocenter/Area/Code must be 3 digits",
        ));
    }
    let reduced_epicenter_code = optional_text(area, NS_SEIS, "ReduceCode")?.map(Cow::into_owned);
    if reduced_epicenter_code
        .as_deref()
        .is_some_and(|code| code.len() != 4 || !code.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(missing(
            "Body/Earthquake/Hypocenter/Area/ReduceCode must be 4 digits",
        ));
    }
    let coordinate = child(area, NS_ELEMENT, "Coordinate")?
        .ok_or_else(|| missing("Body/Earthquake/Hypocenter/Area/Coordinate"))?;
    let coordinate = parse_coordinate(&scalar_text(coordinate, "Coordinate")?)?;
    let land_or_sea = optional_text(area, NS_SEIS, "LandOrSea")?.map(Cow::into_owned);
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
    for rank in [epicenter, depth, magnitude_calculation] {
        if scalar_text(rank, "Accuracy")? != "NaN" {
            return Err(missing(
                "Accuracy quantities must contain NaN; the rank is an attribute",
            ));
        }
    }
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

    let magnitude = child(node, NS_ELEMENT, "Magnitude")?
        .ok_or_else(|| missing("Body/Earthquake/Magnitude"))?;
    if magnitude
        .attribute("type")
        .ok_or_else(|| missing("Magnitude/@type"))?
        != "Mj"
    {
        return Err(unavailable(
            "Earthquake/Magnitude/@type",
            "only Mj has a verified mapping to the Telegram magnitude field",
        ));
    }
    let magnitude_condition = magnitude.attribute("condition");
    let magnitude = match scalar_text(magnitude, "Magnitude")?.as_ref() {
        "NaN" | "" => None,
        value => Some(
            value
                .parse::<f64>()
                .map_err(|_| missing("Body/Earthquake/Magnitude"))?,
        ),
    };
    if magnitude.is_some_and(|value| !value.is_finite()) {
        return Err(missing(
            "Magnitude must be finite or the explicit unknown NaN",
        ));
    }
    if magnitude_condition.is_some_and(|condition| condition != "不明" || magnitude.is_some()) {
        return Err(missing(
            "Magnitude/@condition must indicate 不明 only for an unknown magnitude",
        ));
    }
    if assumed_hypocenter
        && (coordinate.depth_km != Some(10)
            || land_or_sea.is_some()
            || magnitude != Some(1.0)
            || accuracy.epicenter_rank != 1
            || !matches!(accuracy.epicenter_rank2, 1 | 9)
            || accuracy.depth_rank != 1
            || accuracy.magnitude_rank != 8
            // 0 is unknown, not zero stations. This internal-use field can
            // change without notice (JMA XML II.21, 1-4-2-4).
            || !matches!(accuracy.magnitude_stations, 0 | 1))
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
        maximum_intensity: Some(parse_intensity(required_child(
            node,
            NS_SEIS,
            "ForecastInt",
        )?)?),
        _maximum_long_period: child(node, NS_SEIS, "ForecastLgInt")?
            .map(parse_long_period)
            .transpose()?,
        ..Forecast::default()
    };

    if let Some(appendix) = child(node, NS_SEIS, "Appendix")? {
        result.max_int_change = Some(
            required_text(appendix, NS_SEIS, "MaxIntChange")?
                .parse()
                .map_err(|_| missing("Appendix/MaxIntChange"))?,
        );
        result._max_long_period_change = optional_u8(appendix, "MaxLgIntChange")?;
        if result._maximum_long_period.is_some() != result._max_long_period_change.is_some() {
            return Err(missing(
                "Appendix/MaxLgIntChange must accompany ForecastLgInt",
            ));
        }
        result.max_int_change_reason = Some(
            required_text(appendix, NS_SEIS, "MaxIntChangeReason")?
                .parse()
                .map_err(|_| missing("Appendix/MaxIntChangeReason"))?,
        );
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

    let mut area_codes = HashSet::new();
    for pref in children(node, NS_SEIS, "Pref") {
        for area in children(pref, NS_SEIS, "Area") {
            let category = required_child(area, NS_SEIS, "Category")?;
            let kind = required_child(category, NS_SEIS, "Kind")?;
            let kind_code = required_text(kind, NS_SEIS, "Code")?.to_string();
            if !matches!(kind_code.as_str(), "00" | "01" | "09" | "10" | "11" | "19") {
                return Err(missing(
                    "Forecast/Pref/Area/Category/Kind/Code must be 00, 01, 09, 10, 11, or 19",
                ));
            }
            let code = required_text(area, NS_SEIS, "Code")?.to_string();
            if code.len() != 3 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(missing("Forecast/Pref/Area/Code must be 3 digits"));
            }
            if !area_codes.insert(required_text(area, NS_SEIS, "Code")?) {
                return Err(missing("Forecast contains duplicate area codes"));
            }
            let forecast_area = ForecastArea {
                code,
                kind_code,
                intensity: parse_intensity(required_child(area, NS_SEIS, "ForecastInt")?)?,
                _long_period: child(area, NS_SEIS, "ForecastLgInt")?
                    .map(parse_long_period)
                    .transpose()?,
                arrival_time: optional_text(area, NS_SEIS, "ArrivalTime")?
                    .map(|value| parse_time(&value, "Forecast/Pref/Area/ArrivalTime"))
                    .transpose()?,
                condition: optional_text(area, NS_SEIS, "Condition")?.map(Cow::into_owned),
            };
            // Validate every area, including low-intensity LPGM areas that
            // will be omitted from the legacy EBI projection.
            let arrived = forecast_area.kind_code.ends_with('1');
            if arrived {
                if forecast_area.arrival_time.is_some()
                    || forecast_area.condition.as_deref() != Some("既に主要動到達と推測")
                {
                    return Err(missing(
                        "arrived area requires Condition and no ArrivalTime",
                    ));
                }
            } else if forecast_area.arrival_time.is_none() || forecast_area.condition.is_some() {
                return Err(missing(
                    "unarrived/PLUM area requires ArrivalTime and no Condition",
                ));
            }
            result.areas.push(forecast_area);
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
    let Some(headline) = child(head, NS_INFO, "Headline")? else {
        return Ok(result);
    };
    for information in children(headline, NS_INFO, "Information") {
        let area_type = information.attribute("type").unwrap_or_default();
        if !matches!(
            area_type,
            "緊急地震速報（地方予報区）"
                | "緊急地震速報（府県予報区）"
                | "緊急地震速報（細分区域）"
        ) {
            continue;
        }
        for item in children(information, NS_INFO, "Item") {
            let kind = required_child(item, NS_INFO, "Kind")?;
            match required_text(kind, NS_INFO, "Code")?.as_ref() {
                "00" => continue,
                "31" => {}
                _ => return Err(missing("Headline/Kind/Code must be 00 or 31")),
            }
            let last_kind = required_child(item, NS_INFO, "LastKind")?;
            let is_added = match required_text(last_kind, NS_INFO, "Code")?.as_ref() {
                "00" => true,
                "31" => false,
                _ => return Err(missing("Headline/LastKind/Code must be 00 or 31")),
            };
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
                if current.contains(&code) {
                    return Err(missing("Headline contains duplicate warning area codes"));
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
    let value = value.trim();
    if value.is_empty() || value == "/" {
        return Err(unavailable(
            "Earthquake/Hypocenter/Area/Coordinate",
            "all coordinates are unknown; the code specification defines unset latitude/longitude for cancellation only",
        ));
    }
    let value = value
        .strip_suffix('/')
        .ok_or_else(|| missing("Coordinate requires ISO 6709 terminal /"))?;
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
    if time.nanosecond() != 0 {
        return Err(unavailable(
            field,
            "subsecond precision cannot be represented in Telegram",
        ));
    }
    Ok(time)
}

fn parse_attribute_u8(node: Node<'_, '_>, attribute: &str, field: &str) -> Result<u8, AppError> {
    node.attribute(attribute)
        .ok_or_else(|| missing(field))?
        .trim()
        .parse()
        .map_err(|_| missing(field))
}

fn optional_u8(node: Node<'_, '_>, name: &str) -> Result<Option<u8>, AppError> {
    optional_text(node, NS_SEIS, name)?
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
) -> Result<Option<Node<'a, 'input>>, AppError> {
    let mut nodes = node.children().filter(|child| {
        child.is_element()
            && child.tag_name().namespace() == Some(namespace)
            && child.tag_name().name() == name
    });
    let first = nodes.next();
    if nodes.next().is_some() {
        return Err(AppError::InvalidXml(format!("duplicate {name} element")));
    }
    Ok(first)
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
    child(node, namespace, name)?.ok_or_else(|| missing(name))
}

fn required_text<'a>(
    node: Node<'a, '_>,
    namespace: &str,
    name: &str,
) -> Result<Cow<'a, str>, AppError> {
    optional_text(node, namespace, name)?.ok_or_else(|| missing(name))
}

fn optional_text<'a>(
    node: Node<'a, '_>,
    namespace: &str,
    name: &str,
) -> Result<Option<Cow<'a, str>>, AppError> {
    child(node, namespace, name)?
        .map(|node| leaf_text(node, name))
        .transpose()
}

fn leaf_text<'a>(node: Node<'a, '_>, name: &str) -> Result<Cow<'a, str>, AppError> {
    let text = scalar_text(node, name)?;
    if text.is_empty() {
        return Err(missing(name));
    }
    Ok(text)
}

fn scalar_text<'a>(node: Node<'a, '_>, name: &str) -> Result<Cow<'a, str>, AppError> {
    if node.children().any(|n| n.is_element()) {
        return Err(missing(format!("{name} must contain simple text")));
    }
    Ok(match crate::xml::scalar_text(node) {
        Cow::Borrowed(s) => Cow::Borrowed(s.trim()),
        Cow::Owned(s) => Cow::Owned(s.trim().to_owned()),
    })
}

pub(crate) fn unavailable(field: impl Into<String>, reason: impl Into<String>) -> AppError {
    AppError::Unsupported {
        event_id: None,
        xml_type: None,
        info_type: None,
        field: Some(field.into()),
        reason: reason.into(),
    }
}

fn missing(field: impl Into<String>) -> AppError {
    AppError::MissingXmlField(field.into())
}

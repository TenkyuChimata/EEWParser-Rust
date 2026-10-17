use chrono::{DateTime, FixedOffset};

use crate::error::AppError;
use crate::legacy_xml::{
    Earthquake, EewXmlDocument, Forecast, ForecastArea, InfoType, IntensityRange, WarningAreas,
    XmlProduct,
};

pub fn encode(document: &EewXmlDocument) -> Result<String, AppError> {
    if document.info_type == InfoType::Correction {
        return Err(document.unsupported(
            "Head/InfoType",
            "訂正 has no corresponding state in the current code Telegram fields",
        ));
    }
    let code_type = select_code_type(document)?;
    let section = section_code(&document.editorial_office)?;
    let message_type = message_type(document)?;
    let sent_at = telegram_time(&document.control_time);
    let mut lines = vec![format!(
        "{code_type} {section} {message_type} {sent_at} C11"
    )];

    match code_type {
        "35" | "36" | "37" | "39" => {
            lines.extend(forecast_lines(document, code_type)?);
        }
        "47" => lines.extend(warning_lines(document)?),
        "48" => {
            return Err(document.unsupported(
                "Body/Earthquake",
                "a standalone warning cancellation XML omits the origin time and hypocenter required by code 48",
            ));
        }
        _ => unreachable!(),
    }
    lines.push("9999=".to_string());
    Ok(lines.join("\n"))
}

pub fn select_code_type(document: &EewXmlDocument) -> Result<&'static str, AppError> {
    match (document.product, document.info_type) {
        (XmlProduct::Warning, InfoType::Cancellation) => Ok("48"),
        (XmlProduct::Warning, _) => Ok("47"),
        (XmlProduct::Forecast, InfoType::Cancellation) => Ok("39"),
        (XmlProduct::Forecast, _) => {
            let earthquake = document
                .earthquake
                .as_ref()
                .ok_or_else(|| missing("Body/Earthquake"))?;
            if earthquake.origin_time.is_none() {
                return Ok("35");
            }
            if earthquake.assumed_hypocenter {
                return Ok("36");
            }
            match earthquake.accuracy.epicenter_rank {
                1 | 2 => Ok("36"),
                3..=8 => Ok("37"),
                _ => Err(document.unsupported(
                    "Body/Earthquake/Hypocenter/Accuracy/Epicenter/@rank",
                    "rank=0 cannot determine Telegram type 36 or 37",
                )),
            }
        }
        (XmlProduct::Unknown, _) => Err(document.unsupported(
            "Head/Title",
            "the XML product type cannot be mapped to a Telegram code type",
        )),
    }
}

fn forecast_lines(document: &EewXmlDocument, code_type: &str) -> Result<Vec<String>, AppError> {
    if document.info_type == InfoType::Cancellation {
        return Err(document.unsupported(
            "Body/Earthquake/OriginTime|ArrivalTime",
            "a cancellation XML omits the occurrence/detection time required by code 39; that field has no official unset value",
        ));
    }

    let earthquake = document
        .earthquake
        .as_ref()
        .ok_or_else(|| missing("Body/Earthquake"))?;
    let forecast = document.forecast.as_ref();
    let event_time = earthquake
        .origin_time
        .as_ref()
        .unwrap_or(&earthquake.arrival_time);
    let mut lines = vec![
        telegram_time(event_time),
        forecast_identification(document)?,
    ];
    lines.push(forecast_core(code_type, earthquake, forecast)?);
    if code_type != "35"
        && let Some(forecast) = forecast
    {
        let groups = forecast
            .areas
            .iter()
            .filter(|area| intensity_at_least_four(&area.intensity))
            .map(ebi_group)
            .collect::<Result<Vec<_>, _>>()?;
        lines.extend(grouped_lines("EBI", &groups, 3));
    }
    Ok(lines)
}

fn forecast_identification(document: &EewXmlDocument) -> Result<String, AppError> {
    let status = if document.final_report { '9' } else { '0' };
    Ok(format!(
        "ND{} NCN{status}{} JD////////////// JN///",
        document.event_id,
        serial_code(document.serial)?
    ))
}

fn forecast_core(
    code_type: &str,
    earthquake: &Earthquake,
    forecast: Option<&Forecast>,
) -> Result<String, AppError> {
    let (latitude, longitude) = coordinate_codes(
        earthquake.coordinate.latitude,
        earthquake.coordinate.longitude,
    )?;
    let depth = depth_code(earthquake.coordinate.depth_km)?;
    let magnitude = if code_type == "35" {
        "//".to_string()
    } else {
        magnitude_code(earthquake.magnitude)?
    };
    let intensity = if code_type == "35" {
        "5-".to_string()
    } else {
        forecast
            .and_then(|forecast| forecast.maximum_intensity.as_ref())
            .map(|range| intensity_code(&range.from))
            .transpose()?
            .unwrap_or_else(|| "//".to_string())
    };
    let rank = |value: u8| {
        if value == 0 {
            '/'
        } else {
            char::from(b'0' + value)
        }
    };
    let accuracy = &earthquake.accuracy;
    if accuracy.epicenter_rank > 9
        || accuracy.epicenter_rank2 > 9
        || accuracy.depth_rank > 9
        || accuracy.magnitude_rank > 9
        || accuracy.magnitude_stations > 9
    {
        return Err(missing(
            "Earthquake/Hypocenter/Accuracy rank exceeds one digit",
        ));
    }
    let rk = format!(
        "RK{}{}{}{}{}",
        rank(accuracy.epicenter_rank),
        rank(accuracy.depth_rank),
        rank(accuracy.magnitude_rank),
        char::from(b'0' + accuracy.magnitude_stations),
        rank(accuracy.epicenter_rank2)
    );
    let land = match (
        earthquake.assumed_hypocenter,
        earthquake.land_or_sea.as_deref(),
    ) {
        (true, _) | (false, None) => '/',
        (false, Some("内陸")) => '0',
        (false, Some("海域")) => '1',
        (false, Some(_)) => return Err(missing("Earthquake/Hypocenter/Area/LandOrSea")),
    };
    let warning = forecast.is_some_and(|forecast| {
        forecast
            .areas
            .iter()
            .any(|area| area.kind_code.starts_with('1'))
    });
    let method = if earthquake.assumed_hypocenter {
        '9'
    } else {
        '/'
    };
    let rt = format!("RT{land}{}{method}//", if warning { '1' } else { '0' });
    let rc = forecast
        .map(|forecast| {
            format!(
                "RC{}{}///",
                optional_digit(forecast.max_int_change),
                optional_digit(forecast.max_int_change_reason)
            )
        })
        .unwrap_or_else(|| "RC/////".to_string());

    Ok(format!(
        "{} {latitude} {longitude} {depth} {magnitude} {intensity} {rk} {rt} {rc}",
        earthquake.epicenter_code
    ))
}

fn ebi_group(area: &ForecastArea) -> Result<String, AppError> {
    let intensity = ebi_intensity(&area.intensity)?;
    let flags = area.kind_code.as_str();
    if !matches!(flags, "00" | "01" | "09" | "10" | "11" | "19") {
        return Err(missing(format!(
            "unsupported Forecast/Pref/Area/Category/Kind/Code {flags:?}"
        )));
    }
    let expected_arrival = flags.ends_with('0') || flags.ends_with('9');
    let time = if expected_arrival {
        let arrival = area.arrival_time.as_ref().ok_or_else(|| {
            missing(format!(
                "Forecast area {} requires ArrivalTime for Kind/Code {flags}",
                area.code
            ))
        })?;
        arrival.with_timezone(&jst()).format("%H%M%S").to_string()
    } else {
        if area.condition.as_deref() != Some("既に主要動到達と推測") {
            return Err(missing(format!(
                "Forecast area {} requires Condition=既に主要動到達と推測",
                area.code
            )));
        }
        "//////".to_string()
    };
    Ok(format!("{} S{intensity} {time} {flags}", area.code))
}

fn warning_lines(document: &EewXmlDocument) -> Result<Vec<String>, AppError> {
    let earthquake = document
        .earthquake
        .as_ref()
        .ok_or_else(|| missing("Body/Earthquake"))?;
    let origin = earthquake
        .origin_time
        .as_ref()
        .ok_or_else(|| missing("warning Earthquake/OriginTime"))?;
    let reduced = earthquake
        .reduced_epicenter_code
        .as_deref()
        .ok_or_else(|| missing("warning Earthquake/Hypocenter/Area/ReduceCode"))?;
    let (latitude, longitude) = coordinate_codes(
        earthquake.coordinate.latitude,
        earthquake.coordinate.longitude,
    )?;
    let mut lines = vec![
        telegram_time(origin),
        format!(
            "ND{} NCPN{}",
            document.event_id,
            warning_serial_code(document.serial)?
        ),
        format!(
            "{reduced} {latitude} {longitude} {}",
            depth_code(earthquake.coordinate.depth_km)?
        ),
    ];
    let forecast = document
        .forecast
        .as_ref()
        .ok_or_else(|| missing("warning Intensity/Forecast"))?;
    let areas = &document.warning_areas;
    validate_warning_current_areas(areas)?;

    let first = document.serial == 1;
    let has_additions = !areas.added_districts.is_empty()
        || !areas.added_prefectures.is_empty()
        || !areas.added_regions.is_empty();
    let prc = if first || !has_additions {
        "PRC0000/".to_string()
    } else {
        let reason = forecast
            .max_int_change_reason
            .filter(|value| matches!(value, 1 | 2 | 9))
            .ok_or_else(|| {
                missing("warning additions require Appendix/MaxIntChangeReason 1, 2, or 9")
            })?;
        format!(
            "PRC{}{}{}{reason}{}",
            u8::from(!areas.added_districts.is_empty()),
            u8::from(!areas.added_prefectures.is_empty()),
            u8::from(!areas.added_regions.is_empty()),
            if earthquake.assumed_hypocenter {
                '9'
            } else {
                '0'
            }
        )
    };
    lines.push(prc);

    let no_district_additions = vec!["0000".to_string()];
    let no_prefecture_additions = vec!["0000".to_string()];
    let no_region_additions = vec!["000".to_string()];
    lines.extend(code_list_lines(
        "CAI",
        if first || areas.added_districts.is_empty() {
            &no_district_additions
        } else {
            &areas.added_districts
        },
    ));
    lines.extend(code_list_lines(
        "CPI",
        if first || areas.added_prefectures.is_empty() {
            &no_prefecture_additions
        } else {
            &areas.added_prefectures
        },
    ));
    lines.extend(code_list_lines(
        "CBI",
        if first || areas.added_regions.is_empty() {
            &no_region_additions
        } else {
            &areas.added_regions
        },
    ));
    lines.extend(code_list_lines("PAI", &areas.current_districts));
    lines.extend(code_list_lines("PPI", &areas.current_prefectures));
    lines.extend(code_list_lines("PBI", &areas.current_regions));
    lines.push("NCP".to_string());
    let mut nested = forecast_lines(document, select_forecast_code(document, earthquake)?)?;
    // VXSE43 carries the warning serial, but not the serial of the VXSE45
    // forecast embedded in a code-47 telegram. Preserve the known normal/final
    // state and use NCN's explicit unset value for the unavailable serial.
    let identification = nested.get_mut(1).ok_or_else(|| {
        AppError::InvalidTelegram("encoder omitted the nested forecast identification".to_string())
    })?;
    *identification = format!(
        "ND{} NCN{}// JD////////////// JN///",
        document.event_id,
        if document.final_report { '9' } else { '0' }
    );
    // Official NCP starts at ND; the event time occurs only in the outer block.
    nested.remove(0);
    lines.extend(nested);
    Ok(lines)
}

fn select_forecast_code(
    document: &EewXmlDocument,
    earthquake: &Earthquake,
) -> Result<&'static str, AppError> {
    if earthquake.origin_time.is_none() {
        Ok("35")
    } else if earthquake.assumed_hypocenter {
        Ok("36")
    } else {
        match earthquake.accuracy.epicenter_rank {
            1 | 2 => Ok("36"),
            3..=8 => Ok("37"),
            _ => Err(document.unsupported(
                "Body/Earthquake/Hypocenter/Accuracy/Epicenter/@rank",
                "rank=0 cannot determine nested Telegram type 36 or 37",
            )),
        }
    }
}

fn validate_warning_current_areas(areas: &WarningAreas) -> Result<(), AppError> {
    if areas.current_districts.is_empty()
        || areas.current_prefectures.is_empty()
        || areas.current_regions.is_empty()
    {
        return Err(missing(
            "warning Headline must contain current district, prefecture, and region areas",
        ));
    }
    Ok(())
}

fn grouped_lines(marker: &str, groups: &[String], per_line: usize) -> Vec<String> {
    groups
        .chunks(per_line)
        .enumerate()
        .map(|(index, chunk)| {
            let joined = chunk.join(" ");
            if index == 0 {
                format!("{marker} {joined}")
            } else {
                joined
            }
        })
        .collect()
}

fn code_list_lines(marker: &str, codes: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    let mut line = marker.to_string();
    for code in codes {
        if line.len() + code.len() + 1 > 69 {
            result.push(line);
            line = code.clone();
        } else {
            line.push(' ');
            line.push_str(code);
        }
    }
    result.push(line);
    result
}

fn ebi_intensity(range: &IntensityRange) -> Result<String, AppError> {
    let from = intensity_code(&range.from)?;
    if range.to == "over" {
        Ok(format!("{from}//"))
    } else {
        let to = intensity_code(&range.to)?;
        Ok(format!("{to}{from}"))
    }
}

fn intensity_code(value: &str) -> Result<String, AppError> {
    match value {
        "0" | "不明" => Ok("//".to_string()),
        "1" | "2" | "3" | "4" | "7" => Ok(format!("0{value}")),
        "5-" | "5+" | "6-" | "6+" => Ok(value.to_string()),
        _ => Err(missing(format!("unsupported intensity {value:?}"))),
    }
}

fn intensity_at_least_four(range: &IntensityRange) -> bool {
    matches!(
        range.to.as_str(),
        "4" | "5-" | "5+" | "6-" | "6+" | "7" | "over"
    ) || matches!(range.from.as_str(), "4" | "5-" | "5+" | "6-" | "6+" | "7")
}

fn coordinate_codes(latitude: f64, longitude: f64) -> Result<(String, String), AppError> {
    let latitude_tenths = exact_tenths(latitude, "latitude")?;
    let longitude_tenths = exact_tenths(longitude, "longitude")?;
    if latitude_tenths.unsigned_abs() > 999 || longitude_tenths.unsigned_abs() > 9999 {
        return Err(missing("coordinate does not fit Telegram field width"));
    }
    Ok((
        format!(
            "{}{:03}",
            if latitude_tenths < 0 { 'S' } else { 'N' },
            latitude_tenths.unsigned_abs()
        ),
        format!(
            "{}{:04}",
            if longitude_tenths < 0 { 'W' } else { 'E' },
            longitude_tenths.unsigned_abs()
        ),
    ))
}

fn exact_tenths(value: f64, field: &str) -> Result<i64, AppError> {
    if !value.is_finite() {
        return Err(missing(format!("{field} must be finite")));
    }
    let scaled = value * 10.0;
    let rounded = scaled.round();
    if (scaled - rounded).abs() > 1e-7 {
        return Err(missing(format!(
            "{field} has more precision than Telegram's 0.1 degree field"
        )));
    }
    Ok(rounded as i64)
}

fn depth_code(depth: Option<i64>) -> Result<String, AppError> {
    match depth {
        Some(value @ 0..=999) => Ok(format!("{value:03}")),
        Some(_) => Err(missing(
            "depth does not fit Telegram's 3-digit kilometre field",
        )),
        None => Ok("///".to_string()),
    }
}

fn magnitude_code(magnitude: Option<f64>) -> Result<String, AppError> {
    let Some(magnitude) = magnitude else {
        return Ok("//".to_string());
    };
    let tenths = exact_tenths(magnitude, "magnitude")?;
    if !(0..=99).contains(&tenths) {
        return Err(missing("magnitude does not fit Telegram's 2-digit field"));
    }
    Ok(format!("{tenths:02}"))
}

fn optional_digit(value: Option<u8>) -> char {
    value
        .filter(|value| *value <= 9)
        .map(|value| char::from(b'0' + value))
        .unwrap_or('/')
}

fn serial_code(serial: u16) -> Result<String, AppError> {
    match serial {
        1..=99 => Ok(format!("{serial:02}")),
        100..=359 => {
            let offset = serial - 100;
            let letter = char::from(b'A' + (offset / 10) as u8);
            Ok(format!("{letter}{}", offset % 10))
        }
        _ => Err(missing("Serial must fit the Telegram 01..Z9 range")),
    }
}

fn warning_serial_code(serial: u16) -> Result<String, AppError> {
    serial_code(serial)
}

fn section_code(office: &str) -> Result<&'static str, AppError> {
    match office {
        "札幌管区気象台" => Ok("01"),
        "仙台管区気象台" => Ok("02"),
        "気象庁本庁" | "気象庁" => Ok("03"),
        "大阪管区気象台" => Ok("04"),
        "福岡管区気象台" => Ok("05"),
        "沖縄気象台" => Ok("06"),
        _ => Err(missing(format!(
            "Control/EditorialOffice {office:?} has no Telegram section code"
        ))),
    }
}

fn message_type(document: &EewXmlDocument) -> Result<&'static str, AppError> {
    match (document.status.as_str(), document.info_type) {
        ("通常", InfoType::Cancellation) => Ok("10"),
        ("訓練", InfoType::Cancellation) => Ok("11"),
        ("通常", _) => Ok("00"),
        ("訓練", _) => Ok("01"),
        _ => Err(document.unsupported(
            "Control/Status",
            format!(
                "status {:?} cannot be mapped uniquely to Telegram message type 20 or 30",
                document.status
            ),
        )),
    }
}

fn telegram_time(time: &DateTime<FixedOffset>) -> String {
    time.with_timezone(&jst())
        .format("%y%m%d%H%M%S")
        .to_string()
}

fn jst() -> FixedOffset {
    FixedOffset::east_opt(9 * 60 * 60).expect("+09:00 is a valid fixed offset")
}

fn missing(field: impl Into<String>) -> AppError {
    AppError::MissingXmlField(field.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::telegram_for_json;
    use crate::parser::decoder;

    const GAL_100: &str = include_str!("../tests/fixtures/official_100gal_first.xml");
    const PLUM_WARNING: &str = include_str!("../tests/fixtures/official_plum_warning.xml");
    const PLUM_CANCEL: &str = include_str!("../tests/fixtures/official_plum_cancellation.xml");
    const PLUM_FORECAST: &str = include_str!("../tests/fixtures/official_plum_forecast.xml");
    const NOTO_FIRST: &str = include_str!("../tests/fixtures/official_noto_first.xml");
    const NOTO_FINAL: &str = include_str!("../tests/fixtures/official_noto_final.xml");
    const NOTO_CANCEL: &str = include_str!("../tests/fixtures/official_noto_cancellation.xml");

    fn round_trip(xml_source: &str) -> (String, crate::parser::Telegram) {
        let document = crate::legacy_xml::parse(xml_source).unwrap();
        let telegram = encode(&document).unwrap();
        let decoded = decoder(&telegram, telegram_for_json(&telegram)).unwrap();
        (telegram, decoded)
    }

    #[test]
    fn official_100_gal_first_report_uses_35() {
        let (telegram, decoded) = round_trip(GAL_100);
        assert!(telegram.starts_with("35 04 00 230513161032 C11\n"));
        assert_eq!(decoded.EqID, "20230513161031");
        assert_eq!(decoded.EqTime, "20230513161031");
        assert_eq!(decoded.ShinouLat, Some(29.8));
        assert_eq!(decoded.ShinouLng, Some(129.9));
        assert_eq!(decoded.Shindo, "5弱");
        assert!(decoded.Ebis.is_empty());
        assert!(decoded.Telegram.ends_with("9999="));
    }

    #[test]
    fn official_plum_only_warning_uses_47_and_nested_decoder() {
        let (telegram, decoded) = round_trip(PLUM_WARNING);
        assert!(telegram.starts_with("47 03 00"));
        assert!(telegram.contains("PRC0000/"));
        assert!(telegram.contains("NCP\n"));
        assert!(telegram.contains("NCN0//"));
        assert!(!telegram.contains("NCN001"));
        assert_eq!(decoded.EqID, "20230505215718");
        assert_eq!(decoded.WarnType, "一般向け");
        assert_eq!(decoded.WarnNum, "01");
        assert_eq!(decoded.WarnCode, "通常");
        assert_eq!(decoded.Ebis.len(), 1);
        assert_eq!(
            decoded.Ebis[0].Arrive,
            "主要動到達時刻の予測なし（PLUM 法による予測）"
        );

        let final_xml = PLUM_WARNING.replace(
            "</Body>",
            "<NextAdvisory>この情報をもって、緊急地震速報：最終報とします。</NextAdvisory></Body>",
        );
        let (final_telegram, final_decoded) = round_trip(&final_xml);
        assert!(final_telegram.contains("NCN9//"));
        assert_eq!(final_decoded.WarnCode, "最終");
    }

    #[test]
    fn official_forecast_cancellation_reports_irrecoverable_required_time() {
        let document = crate::legacy_xml::parse(PLUM_CANCEL).unwrap();
        assert_eq!(select_code_type(&document).unwrap(), "39");
        assert!(
            matches!(encode(&document), Err(AppError::Unsupported { reason, .. }) if reason.contains("occurrence/detection time"))
        );
    }

    #[test]
    fn official_one_point_plum_forecast_round_trips() {
        let (forecast, decoded) = round_trip(PLUM_FORECAST);
        assert!(forecast.starts_with("36 "));
        assert_eq!(decoded.EqID, "20230906063640");
        assert_eq!(decoded.WarnNum, "01");
        assert!(decoded.Ebis.is_empty());
        assert!(forecast.contains("RT/09//"));
    }

    #[test]
    fn official_noto_first_and_final_reports_share_event_and_use_37() {
        let (telegram, first) = round_trip(NOTO_FIRST);
        assert!(telegram.starts_with("37 "));
        assert_eq!(first.EqID, "20240101161010");
        assert_eq!(first.WarnNum, "01");
        assert_eq!(first.ShinouLat, Some(37.5));
        assert_eq!(first.ShinouLng, Some(137.2));
        assert_eq!(first.ShinouDpth, Some(10));
        assert_eq!(first.Magnitude, Some(5.5));

        let (final_telegram, final_report) = round_trip(NOTO_FINAL);
        assert!(final_telegram.starts_with("37 "));
        assert_eq!(final_report.EqID, first.EqID);
        assert_eq!(final_report.WarnNum, "46");
        assert_eq!(final_report.WarnCode, "最終");
        assert_eq!(final_report.Ebis.len(), 45);
    }

    #[test]
    fn zero_magnitude_calculation_count_is_preserved_in_rk() {
        let xml = NOTO_FINAL.replace(
            "<NumberOfMagnitudeCalculation>5</NumberOfMagnitudeCalculation>",
            "<NumberOfMagnitudeCalculation>0</NumberOfMagnitudeCalculation>",
        );

        let (telegram, _) = round_trip(&xml);

        assert!(telegram.contains("RK44509"));
        assert!(!telegram.contains("RK445/9"));

        // RC encoding must remain unchanged by this RK-only fix.
        assert!(telegram.contains("RC00///"));
    }

    #[test]
    fn official_noto_cancellation_reports_irrecoverable_required_time() {
        let document = crate::legacy_xml::parse(NOTO_CANCEL).unwrap();
        assert_eq!(document.event_id, "20240101161133");
        assert_eq!(document.info_type, InfoType::Cancellation);
        assert_eq!(select_code_type(&document).unwrap(), "39");
        assert!(matches!(
            encode(&document),
            Err(AppError::Unsupported { .. })
        ));
    }

    #[test]
    fn coordinate_hemispheres_are_preserved() {
        let xml = GAL_100
            .replace("+29.8+129.9-10000/", "-29.8-129.9-10000/")
            .replace(
                "北緯２９．８度　東経１２９．９度",
                "南緯２９．８度　西経１２９．９度",
            );
        let (telegram, decoded) = round_trip(&xml);
        assert!(telegram.contains(" S298 W1299 010 "));
        assert_eq!(decoded.ShinouLat, Some(-29.8));
        assert_eq!(decoded.ShinouLng, Some(-129.9));
    }

    #[test]
    fn element_namespace_prefix_is_not_significant() {
        let xml = GAL_100
            .replace("xmlns:jmx_eb=", "xmlns:changed=")
            .replace("jmx_eb:", "changed:");
        let (telegram, _) = round_trip(&xml);
        assert!(telegram.starts_with("35 "));
    }

    #[test]
    fn utf8_bom_xml_completes_the_full_round_trip() {
        let xml = format!("\u{feff}{GAL_100}");
        let (telegram, decoded) = round_trip(&xml);
        assert!(telegram.starts_with("35 "));
        assert_eq!(decoded.EqID, "20230513161031");
    }

    #[test]
    fn utc_control_time_is_converted_to_japan_time() {
        let (telegram, _) = round_trip(GAL_100);
        assert!(telegram.starts_with("35 04 00 230513161032"));

        let utc_origin = NOTO_FIRST.replace(
            "<OriginTime>2024-01-01T16:10:08+09:00</OriginTime>",
            "<OriginTime>2024-01-01T07:10:08Z</OriginTime>",
        );
        let (telegram, _) = round_trip(&utc_origin);
        assert!(telegram.contains("\n240101161008\n"));
    }

    #[test]
    fn malformed_and_non_eew_xml_are_rejected() {
        assert!(matches!(
            crate::legacy_xml::parse("<Report>"),
            Err(AppError::InvalidXml(_))
        ));
        let non_eew = GAL_100.replace("緊急地震速報（地震動予報）", "震源・震度に関する情報");
        assert!(matches!(
            crate::legacy_xml::parse(&non_eew),
            Err(AppError::UnsupportedXml(_))
        ));
    }

    #[test]
    fn missing_required_event_id_is_rejected() {
        let xml = GAL_100.replace("<EventID>20230513161031</EventID>", "");
        assert!(matches!(
            crate::legacy_xml::parse(&xml),
            Err(AppError::MissingXmlField(_))
        ));
    }

    #[test]
    fn unrepresentable_depth_and_unknown_selection_rank_are_rejected() {
        let fractional_depth = GAL_100.replace("+29.8+129.9-10000/", "+29.8+129.9-10500/");
        assert!(matches!(
            crate::legacy_xml::parse(&fractional_depth),
            Err(AppError::MissingXmlField(_))
        ));

        let unknown_rank = NOTO_FIRST.replacen("<Epicenter rank=\"3\"", "<Epicenter rank=\"0\"", 1);
        let document = crate::legacy_xml::parse(&unknown_rank).unwrap();
        assert!(matches!(
            encode(&document),
            Err(AppError::Unsupported { reason, .. }) if reason.contains("cannot determine")
        ));
    }

    #[test]
    fn final_and_training_flags_are_encoded_and_correction_is_rejected() {
        let final_xml = GAL_100.replace(
            "</Body>",
            "<NextAdvisory>この情報をもって、緊急地震速報：最終報とします。</NextAdvisory></Body>",
        );
        let (final_telegram, _) = round_trip(&final_xml);
        assert!(final_telegram.contains("NCN901"));

        let correction = GAL_100.replace("<InfoType>発表</InfoType>", "<InfoType>訂正</InfoType>");
        assert!(matches!(
            crate::legacy_xml::parse(&correction),
            Err(AppError::Unsupported { .. })
        ));

        let training = GAL_100.replace("<Status>通常</Status>", "<Status>訓練</Status>");
        let (training_telegram, _) = round_trip(&training);
        assert!(training_telegram.starts_with("35 04 01"));
    }

    #[test]
    fn ebi_projection_excludes_long_period_only_low_intensity_areas() {
        assert!(!intensity_at_least_four(&IntensityRange {
            from: "3".to_string(),
            to: "3".to_string(),
        }));
        assert!(intensity_at_least_four(&IntensityRange {
            from: "4".to_string(),
            to: "5-".to_string(),
        }));
    }
}

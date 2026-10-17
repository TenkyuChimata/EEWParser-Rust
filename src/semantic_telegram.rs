//! Code Telegram -> semantic information, without a second parse or localized
//! display-string interpretation.
use crate::xml::{axis, depth_quantity, format_number, kind_flags};
use crate::{EewError, codes, model::*, parser};
use chrono::{FixedOffset, NaiveDateTime, NaiveTime};

pub fn parse(raw: &str) -> Result<EewInformation, EewError> {
    parse_with_convention(raw, NcnConvention::Current)
}
pub fn parse_with_convention(
    raw: &str,
    convention: NcnConvention,
) -> Result<EewInformation, EewError> {
    let mut msg = parser::reader(raw)?;
    msg.raw_codes.ncn_convention = convention;
    let legacy_correction = matches!(msg.WarnCode.as_str(), "6" | "7" | "8");
    if legacy_correction && convention != NcnConvention::Legacy2013 {
        return Err(invalid(
            "NCN 6/7/8 require explicit NcnConvention::Legacy2013; the retirement date is not inferred",
        ));
    }
    let product = match msg.CodeType.as_str() {
        "35" | "36" | "37" | "39" => Product::TelegramForecast,
        "47" | "48" => Product::Warning,
        "38" => Product::Test,
        "61" => Product::RealTimeIntensity,
        _ => unreachable!(),
    };
    let sent_at = timestamp(&msg.WarnTime)?;
    let canceled =
        matches!(msg.CodeType.as_str(), "39" | "48") || matches!(msg.MsgType.as_str(), "10" | "11");
    if product == Product::Test && canceled {
        return Err(invalid(
            "a distribution test cannot cancel an earthquake series",
        ));
    }
    let final_report = product != Product::Warning
        && product != Product::Test
        && (msg.WarnCode == "9" || (legacy_correction && msg.WarnCode == "8"));
    let status = match msg.MsgType.as_str() {
        "00" | "10" => Status::Normal,
        "01" | "11" => Status::Training,
        "20" | "30" => Status::Test,
        _ => unreachable!(),
    };
    let mut header = Header {
        event_id: (!msg.EqID.is_empty()).then(|| msg.EqID.clone()),
        status: if msg.MsgType == "20" && product != Product::Test {
            None
        } else {
            Some(status)
        },
        info_type: Some(if canceled {
            InfoType::Cancellation
        } else if legacy_correction {
            InfoType::Correction
        } else {
            InfoType::Publication
        }),
        ..Header::default()
    };
    if let Some(id) = &header.event_id
        && (id.len() != 14 || !id.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(invalid("ND requires 14 digits"));
    }
    header.serial_no = decode_serial(&msg.WarnNum)?.map(|n| n.to_string());
    if convention == NcnConvention::Legacy2013
        && header
            .serial_no
            .as_ref()
            .is_some_and(|n| n.parse::<u32>().is_ok_and(|n| n > 99))
    {
        return Err(invalid("2013 NCN convention supports serials 01..99 only"));
    }
    if !msg.EqTime.is_empty() {
        timestamp(&msg.EqTime)?;
        msg.raw_codes.event_time = Some(msg.EqTime.clone());
    }
    let lower_bound_forecast = msg.CodeType == "36"
        || (product == Product::Warning && matches!(msg.Rk1.as_str(), "1" | "2"));
    let mut body = Body {
        is_last_info: canceled || final_report,
        is_canceled: canceled,
        text: msg.text.take(),
        ..Body::default()
    };
    if !canceled && product != Product::Test {
        body.is_warning = match product {
            Product::Warning => Some(true),
            Product::RealTimeIntensity => None,
            _ => match msg.Rt2.as_str() {
                "0" => Some(false),
                "1" => Some(true),
                _ => None,
            },
        };
        let mut coordinate = Coordinate {
            latitude: msg
                .ShinouLat
                .map(|v| axis(v, &format_number(v.abs()), true)),
            longitude: msg
                .ShinouLng
                .map(|v| axis(v, &format_number(v.abs()), false)),
            ..Coordinate::default()
        };
        if let Some(depth) = msg.ShinouDpth {
            if !(0..=700).contains(&depth) {
                return Err(invalid("depth must be 0..700 km or ///"));
            }
            coordinate.height = Some(Quantity {
                kind: "高さ".into(),
                unit: "m".into(),
                bound: None,
                value: Some((-depth * 1000).to_string()),
                condition: None,
            });
        }
        if coordinate.latitude.is_none() && coordinate.longitude.is_none() {
            coordinate.condition = Some("不明".into());
        }
        let hypo = Hypocenter {
            area: named(&msg.Shinou, codes::epicenter),
            coordinate,
            depth: Some(depth_quantity(msg.ShinouDpth.map(|n| n.to_string()))),
            reduce: msg
                .reduced_epicenter
                .as_ref()
                .map(|c| named(c, codes::reduced)),
            land_or_sea: match msg.Rt1.as_str() {
                "0" => Some("内陸".into()),
                "1" => Some("海域".into()),
                _ => None,
            },
            accuracy: msg.raw_codes.rk.as_ref().map(|_| Accuracy {
                epicenters: [known(&msg.Rk1), known(&msg.Rk5)],
                depth: known(&msg.Rk2),
                magnitude_calculation: known(&msg.Rk3),
                number_of_magnitude_calculation: known(&msg.Rk4),
            }),
        };
        let magnitude = msg.raw_codes.rk.as_ref().map(|_| Quantity {
            kind: "マグニチュード".into(),
            unit: "M".into(),
            bound: (lower_bound_forecast && msg.Rt3 != "9" && msg.Magnitude.is_some())
                .then_some(QuantityBound::Lower),
            value: msg.Magnitude.map(|v| format!("{v:.1}")),
            condition: msg.Magnitude.is_none().then(|| "Ｍ不明".into()),
        });
        let event_time = (!msg.EqTime.is_empty())
            .then(|| timestamp(&msg.EqTime))
            .transpose()?;
        body.earthquake = Some(Earthquake {
            // The first event-time field is detection time only for code 35.
            origin_time: if matches!(msg.CodeType.as_str(), "35" | "61") {
                None
            } else {
                event_time.clone()
            },
            arrival_time: if msg.CodeType == "35" {
                event_time
            } else {
                None
            },
            condition: (msg.Rt3 == "9").then(|| "仮定震源要素".into()),
            hypocenter: hypo,
            magnitude,
        });
        if product == Product::Warning {
            body.zones = warning_areas(
                &msg.raw_codes.pai,
                &msg.raw_codes.cai,
                msg.WarnNum == "01",
                codes::zone,
            );
            body.prefectures = warning_areas(
                &msg.raw_codes.ppi,
                &msg.raw_codes.cpi,
                msg.WarnNum == "01",
                codes::prefecture,
            );
            body.regions = warning_areas(
                &msg.raw_codes.pbi,
                &msg.raw_codes.cbi,
                msg.WarnNum == "01",
                codes::region,
            );
        }
        if product == Product::RealTimeIntensity {
            if !msg.eip.is_empty() {
                let stations = msg
                    .eip
                    .iter()
                    .map(|(c, raw)| {
                        let k = measured_intensity(raw);
                        Station {
                            station: named(c, codes::station),
                            int: k.map(intensity_class).unwrap_or(IntensityValue::Unknown),
                            k: k.map(|v| format!("{v:.1}")),
                        }
                    })
                    .collect();
                body.intensity = Some(Intensity {
                    stations: Some(stations),
                    ..Intensity::default()
                });
            }
        } else if msg.Shindo != "//" || !msg.Ebis.is_empty() {
            let mut regions = Vec::with_capacity(msg.Ebis.len());
            for e in &msg.Ebis {
                let kind_code = format!("{}{}", e.Type, e.Arrive);
                let (is_warning, is_plum) = kind_flags(&kind_code);
                let upper = intensity_value(&e.Shindo1)?;
                let (from, to) = if e.Shindo2 == "//" {
                    (upper, IntensityValue::Over)
                } else {
                    (intensity_value(&e.Shindo2)?, upper)
                };
                if matches!((from.rank(), to.rank()), (Some(a), Some(b)) if a > b) {
                    return Err(invalid("EBI lower bound exceeds upper bound"));
                }
                if e.Time != "//////" {
                    NaiveTime::parse_from_str(&e.Time, "%H%M%S")
                        .map_err(|_| invalid("EBI invalid HHMMSS"))?;
                }
                regions.push(ForecastRegion {
                    area: named(&e.Chiiki, codes::region),
                    kind: CodeName {
                        code: kind_code,
                        name: is_warning.map(|b| {
                            if b {
                                "緊急地震速報（警報）".into()
                            } else {
                                "緊急地震速報（予報）".into()
                            }
                        }),
                    },
                    is_warning,
                    is_plum,
                    forecast_max_int: Some(ForecastRange { from, to }),
                    forecast_max_lg_int: None,
                    condition: (e.Arrive == "1").then(|| "既に主要動到達と推測".into()),
                    arrival_time: None,
                    arrival_time_of_day: (e.Arrive != "1" && e.Time != "//////")
                        .then(|| e.Time.clone()),
                });
            }
            let appendix = if known(&msg.Rc1).is_some() || known(&msg.Rc2).is_some() {
                Some(Appendix {
                    max_int_change: known(&msg.Rc1),
                    max_int_change_reason: known(&msg.Rc2),
                    max_lg_int_change: None,
                })
            } else {
                None
            };
            let maximum = if msg.Shindo == "//" {
                None
            } else {
                let value = intensity_value(&msg.Shindo)?;
                Some(ForecastRange {
                    from: value.clone(),
                    to: if msg.CodeType == "35" || lower_bound_forecast || msg.Rt3 == "9" {
                        IntensityValue::Over
                    } else {
                        value
                    },
                })
            };
            body.intensity = Some(Intensity {
                forecast_max_int: maximum,
                regions: Some(regions),
                appendix,
                ..Intensity::default()
            });
        }
    }
    msg.raw_codes.ebi_times = msg.Ebis.iter().map(|e| e.Time.clone()).collect();
    msg.raw_codes.eip = msg.eip;
    msg.raw_codes.maximum_intensity = (!msg.Shindo.is_empty()).then_some(msg.Shindo);
    Ok(EewInformation {
        header,
        product,
        body,
        source: Some(Source {
            format: SourceFormat::Telegram,
            raw: raw.to_owned(),
            final_report,
            sent_at: Some(sent_at),
            code_table_version: Some("2026-10-07".into()),
            telegram: Some(msg.raw_codes),
        }),
    })
}
fn named(code: &str, table: fn(&str) -> Option<&'static str>) -> CodeName {
    CodeName {
        code: code.to_owned(),
        name: table(code).map(str::to_owned),
    }
}
fn known(code: &str) -> Option<String> {
    (!code.is_empty() && !code.contains('/')).then(|| code.to_owned())
}
fn warning_areas(
    current: &[String],
    added: &[String],
    first: bool,
    table: fn(&str) -> Option<&'static str>,
) -> Option<Vec<WarningArea>> {
    let list: Vec<_> = current
        .iter()
        .filter(|c| !c.bytes().all(|b| b == b'0' || b == b'/'))
        .map(|c| WarningArea {
            area: named(c, table),
            kind: WarningKind {
                current: CodeName {
                    code: "31".into(),
                    name: Some("緊急地震速報（警報）".into()),
                },
                // First-warning CAI is 0000 by protocol, but all areas are new.
                last_kind: Some(if first || added.contains(c) {
                    CodeName {
                        code: "00".into(),
                        name: Some("なし".into()),
                    }
                } else {
                    CodeName {
                        code: "31".into(),
                        name: Some("緊急地震速報（警報）".into()),
                    }
                }),
            },
        })
        .collect();
    (!list.is_empty()).then_some(list)
}
/// Official NCN and NCPN serial encoding. Unset is not zero.
pub fn decode_serial(raw: &str) -> Result<Option<u32>, EewError> {
    if raw.is_empty() || raw == "//" {
        return Ok(None);
    }
    let b = raw.as_bytes();
    if b.len() != 2 || !b[1].is_ascii_digit() {
        return Err(invalid("invalid NCN/NCPN serial"));
    }
    let number = match b[0] {
        b'0'..=b'9' => u32::from(b[0] - b'0') * 10 + u32::from(b[1] - b'0'),
        b'A'..=b'Z' => 100 + u32::from(b[0] - b'A') * 10 + u32::from(b[1] - b'0'),
        _ => return Err(invalid("invalid serial")),
    };
    if number == 0 {
        return Err(invalid("serial 00 is not an information number"));
    }
    Ok(Some(number))
}
fn timestamp(raw: &str) -> Result<String, EewError> {
    let full = format!("20{raw}");
    let value = NaiveDateTime::parse_from_str(&full, "%Y%m%d%H%M%S")
        .map_err(|_| invalid("invalid YYMMDDHHMMSS"))?;
    Ok(value
        .and_local_timezone(FixedOffset::east_opt(9 * 3600).expect("JST"))
        .single()
        .expect("fixed offset")
        .to_rfc3339())
}
fn intensity_value(raw: &str) -> Result<IntensityValue, EewError> {
    let value = match raw {
        "//" => "不明",
        "01" => "1",
        "02" => "2",
        "03" => "3",
        "04" => "4",
        "07" => "7",
        "05" => "5",
        "06" => "6",
        "5-" | "5+" | "6-" | "6+" => raw,
        _ => return Err(invalid(format!("undefined Telegram intensity {raw:?}"))),
    };
    Ok(IntensityValue::from_text(value))
}
fn measured_intensity(raw: &str) -> Option<f64> {
    let b = raw.as_bytes();
    if b.len() != 2 || !b[1].is_ascii_digit() {
        return None;
    }
    let tenth = f64::from(b[1] - b'0') / 10.0;
    match b[0] {
        b'0'..=b'9' => Some(f64::from(b[0] - b'0') + tenth),
        b'-' => Some(-tenth),
        b'A'..=b'H' => {
            let value = -f64::from(b[0] - b'A' + 1) - tenth;
            (value >= -8.0).then_some(value)
        }
        _ => None,
    }
}
fn intensity_class(v: f64) -> IntensityValue {
    match v {
        v if v < 0.5 => IntensityValue::Zero,
        v if v < 1.5 => IntensityValue::One,
        v if v < 2.5 => IntensityValue::Two,
        v if v < 3.5 => IntensityValue::Three,
        v if v < 4.5 => IntensityValue::Four,
        v if v < 5.0 => IntensityValue::FiveLower,
        v if v < 5.5 => IntensityValue::FiveUpper,
        v if v < 6.0 => IntensityValue::SixLower,
        v if v < 6.5 => IntensityValue::SixUpper,
        _ => IntensityValue::Seven,
    }
}
fn invalid(s: impl Into<String>) -> EewError {
    EewError::InvalidTelegram(s.into())
}

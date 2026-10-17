use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::model::TelegramCodes;

// Telegram main struct
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Telegram {
    pub CodeType: String,
    pub Section: String,
    pub MsgType: String,
    pub WarnTime: String,
    pub CommandCode: String,
    pub EqTime: String,
    pub EqID: String,
    pub WarnType: String,
    pub WarnCode: String,
    pub WarnNum: String,
    pub Shinou: String,
    pub ShinouLat: Option<f64>,
    pub ShinouLng: Option<f64>,
    pub ShinouDpth: Option<i64>,
    pub Magnitude: Option<f64>,
    pub Shindo: String,
    pub Rk1: String,
    pub Rk2: String,
    pub Rk3: String,
    pub Rk4: String,
    pub Rk5: String,
    pub Rt1: String,
    pub Rt2: String,
    pub Rt3: String,
    pub Rc1: String,
    pub Rc2: String,
    pub Ebis: Vec<Ebi>,
    pub Telegram: String,
    #[serde(default)]
    pub raw_codes: TelegramCodes,
    #[serde(default)]
    pub reduced_epicenter: Option<String>,
    #[serde(default)]
    pub eip: Vec<(String, String)>,
    #[serde(default)]
    pub text: Option<String>,
}

// Ebi in Telegram 予想震度と到達時
#[allow(non_snake_case)]
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Ebi {
    pub Chiiki: String,
    pub Shindo1: String,
    pub Shindo2: String,
    pub Time: String,
    pub Type: String,
    pub Arrive: String,
}

// ========== Public API ==========

pub fn decoder(s: &str, logical_telegram: String) -> Result<Telegram, AppError> {
    let from = reader(s)?;
    let mut to = Telegram {
        CodeType: code_type(&from.CodeType).to_string(),
        Section: section(&from.Section).to_string(),
        MsgType: msg_type(&from.MsgType).to_string(),
        WarnTime: format!("20{}", from.WarnTime),
        CommandCode: from.CommandCode.clone(),
        EqTime: if from.EqTime.is_empty() {
            String::new()
        } else {
            format!("20{}", from.EqTime)
        },
        EqID: from.EqID.clone(),
        WarnType: warn_type(&from.WarnType).to_string(),
        WarnCode: warn_code(&from.WarnCode).to_string(),
        WarnNum: from.WarnNum.clone(),
        Shinou: shinou_code(&from.Shinou).to_string(),
        ShinouLat: from.ShinouLat,
        ShinouLng: from.ShinouLng,
        ShinouDpth: from.ShinouDpth,
        Magnitude: from.Magnitude,
        Shindo: shindo(&from.Shindo).to_string(),
        Rk1: rk1(&from.Rk1).to_string(),
        Rk2: rk2(&from.Rk2).to_string(),
        Rk3: rk3(&from.Rk3, &from.WarnTime).to_string(),
        Rt1: rt1(&from.Rt1).to_string(),
        Rt2: rt2(&from.Rt2).to_string(),
        Rt3: rt3(&from.Rt3).to_string(),
        Rc1: rc1(&from.Rc1).to_string(),
        Rc2: rc2(&from.Rc2).to_string(),
        Rk4: from.Rk4.clone(),
        Rk5: from.Rk5.clone(),
        raw_codes: from.raw_codes.clone(),
        reduced_epicenter: from.reduced_epicenter.clone(),
        eip: from.eip.clone(),
        text: from.text.clone(),
        Telegram: logical_telegram,
        ..Telegram::default()
    };

    for e in &from.Ebis {
        to.Ebis.push(Ebi {
            Chiiki: chiiki_code(&e.Chiiki).to_string(),
            Shindo1: shindo(&e.Shindo1).to_string(),
            Shindo2: shindo(&e.Shindo2).to_string(),
            Time: e.Time.clone(),
            Type: ebiy1(&e.Type).to_string(),
            Arrive: ebiy2(&e.Arrive).to_string(),
        });
    }

    Ok(to)
}

impl Telegram {
    /// The raw JMA event identifier carried by ND.
    pub fn event_id(&self) -> &str {
        &self.EqID
    }

    /// The raw NCN/NCPN serial token. An empty token means the Telegram type
    /// does not carry an event serial.
    pub fn serial_token(&self) -> Option<&str> {
        (!self.WarnNum.is_empty()).then_some(self.WarnNum.as_str())
    }

    /// Converts the official 01..99/A0..Z9 serial representation without
    /// interpreting localized display strings or inventing a fallback value.
    pub fn serial(&self) -> Option<u32> {
        let token = self.serial_token()?;
        if token.len() != 2 || !token.is_ascii() {
            return None;
        }
        if token.bytes().all(|byte| byte.is_ascii_digit()) {
            return token.parse().ok();
        }
        let bytes = token.as_bytes();
        if bytes[0].is_ascii_uppercase() && bytes[1].is_ascii_digit() {
            return Some(100 + u32::from(bytes[0] - b'A') * 10 + u32::from(bytes[1] - b'0'));
        }
        None
    }

    pub fn is_final(&self) -> bool {
        !matches!(self.raw_codes.code_type.as_str(), "47" | "48" | "38")
            && self
                .raw_codes
                .ncn
                .as_deref()
                .is_some_and(|n| n.starts_with(['8', '9']))
    }
}

pub fn reader(s: &str) -> Result<Telegram, AppError> {
    const MAX_TOKENS: usize = 65_536;
    let tokens: Vec<&str> = s.split_whitespace().take(MAX_TOKENS + 1).collect();
    if tokens.len() > MAX_TOKENS {
        return Err(invalid("Telegram exceeds 65536 tokens"));
    }
    if tokens.len() < 6 {
        return Err(invalid("Telegram is too short"));
    }

    let mut msg = Telegram::default();
    let mut index = 0;

    msg.CodeType = take(&tokens, &mut index, "code type")?.to_string();
    if !matches!(
        msg.CodeType.as_str(),
        "35" | "36" | "37" | "38" | "39" | "47" | "48" | "61"
    ) {
        return Err(invalid(format!("unsupported code type {:?}", msg.CodeType)));
    }
    msg.Section = fixed(take(&tokens, &mut index, "section")?, 2, "section")?.to_string();
    msg.MsgType = fixed(
        take(&tokens, &mut index, "message type")?,
        2,
        "message type",
    )?
    .to_string();
    msg.WarnTime = fixed(
        take(&tokens, &mut index, "warning time")?,
        12,
        "warning time",
    )?
    .to_string();
    let command = fixed(
        take(&tokens, &mut index, "command code")?,
        3,
        "command code",
    )?;
    msg.CommandCode = command
        .strip_prefix('C')
        .ok_or_else(|| invalid("command code must start with C"))?
        .to_string();

    if !matches!(
        msg.MsgType.as_str(),
        "00" | "01" | "10" | "11" | "20" | "30"
    ) {
        return Err(invalid("unsupported message type"));
    }
    if msg.CommandCode != "11" {
        return Err(invalid(
            "fragmented Telegram requires reassembly before decoding (expected C11)",
        ));
    }
    msg.raw_codes.code_type.clone_from(&msg.CodeType);
    msg.raw_codes.section.clone_from(&msg.Section);
    msg.raw_codes.message_type.clone_from(&msg.MsgType);
    msg.raw_codes.command.clone_from(&msg.CommandCode);
    match msg.CodeType.as_str() {
        "35" | "36" | "37" | "39" | "61" => {
            parse_forecast(&tokens, &mut index, &mut msg)?;
        }
        "47" => parse_warning(&tokens, &mut index, &mut msg, true)?,
        "48" => parse_warning(&tokens, &mut index, &mut msg, false)?,
        "38" => {}
        _ => unreachable!(),
    }

    let end = take(&tokens, &mut index, "Telegram terminator")?;
    if end == "9999" && s.trim_end().ends_with('=') {
        // Official code-38 tests terminate the code block with 9999, then
        // append human-readable text before the Telegram's final '='.
        let offset = end.as_ptr() as usize - s.as_ptr() as usize + end.len();
        let text = s[offset..]
            .trim()
            .strip_suffix('=')
            .unwrap_or_default()
            .trim();
        msg.text = (!text.is_empty()).then(|| text.to_owned());
    } else if end != "9999=" || index != tokens.len() {
        return Err(invalid(
            "expected 9999=, or 9999 followed by text and terminal =",
        ));
    }

    Ok(msg)
}

fn parse_forecast(tokens: &[&str], index: &mut usize, msg: &mut Telegram) -> Result<(), AppError> {
    // NCP begins with ND, without repeating the earthquake time.
    if msg.CodeType != "47" {
        msg.EqTime = fixed(
            take(tokens, index, "earthquake time")?,
            12,
            "earthquake time",
        )?
        .to_string();
    }
    let nd = prefixed(take(tokens, index, "ND")?, "ND", 14)?;
    msg.EqID = nd.to_string();

    let ncn = prefixed(take(tokens, index, "NCN")?, "NCN", 3)?;
    msg.WarnType = "NCN".to_string();
    msg.raw_codes.ncn = Some(ncn.to_owned());
    if !matches!(&ncn[0..1], "0" | "6" | "7" | "8" | "9" | "/") {
        return Err(invalid("invalid NCN status"));
    }
    msg.WarnCode = ncn[0..1].to_string();
    msg.WarnNum = ncn[1..3].to_string();

    msg.raw_codes.jd = Some(prefixed(take(tokens, index, "JD")?, "JD", 14)?.to_owned());
    msg.raw_codes.jn = Some(prefixed(take(tokens, index, "JN")?, "JN", 3)?.to_owned());

    msg.Shinou = fixed(take(tokens, index, "epicenter code")?, 3, "epicenter code")?.to_string();
    msg.ShinouLat = parse_coordinate(take(tokens, index, "latitude")?, 'N', 'S', 3)?;
    msg.ShinouLng = parse_coordinate(take(tokens, index, "longitude")?, 'E', 'W', 4)?;
    msg.ShinouDpth = parse_unknown_i64(take(tokens, index, "depth")?, 3, "depth")?;
    msg.Magnitude = parse_unknown_decimal(take(tokens, index, "magnitude")?, 2, "magnitude")?;
    msg.Shindo = fixed(
        take(tokens, index, "maximum intensity")?,
        2,
        "maximum intensity",
    )?
    .to_string();

    let rk = prefixed(take(tokens, index, "RK")?, "RK", 5)?;
    msg.raw_codes.rk = Some(rk.to_owned());
    validate_digits_or_slashes(rk, "RK")?;
    msg.Rk1 = rk[0..1].to_string();
    msg.Rk2 = rk[1..2].to_string();
    msg.Rk3 = rk[2..3].to_string();
    msg.Rk4 = rk[3..4].to_string();
    msg.Rk5 = rk[4..5].to_string();

    let rt = prefixed(take(tokens, index, "RT")?, "RT", 5)?;
    msg.raw_codes.rt = Some(rt.to_owned());
    validate_digits_or_slashes(rt, "RT")?;
    msg.Rt1 = rt[0..1].to_string();
    msg.Rt2 = rt[1..2].to_string();
    msg.Rt3 = rt[2..3].to_string();

    let rc = prefixed(take(tokens, index, "RC")?, "RC", 5)?;
    msg.raw_codes.rc = Some(rc.to_owned());
    validate_digits_or_slashes(rc, "RC")?;
    msg.Rc1 = rc[0..1].to_string();
    msg.Rc2 = rc[1..2].to_string();

    match tokens.get(*index).copied() {
        Some("EBI") => {
            *index += 1;
            while tokens.get(*index).copied() != Some("9999=") {
                msg.Ebis.push(parse_ebi(tokens, index)?);
            }
        }
        Some("EIP") => {
            *index += 1;
            while tokens.get(*index).copied() != Some("9999=") {
                let code = take(tokens, index, "EIP station code")?;
                if !(7..=8).contains(&code.len())
                    || !code.bytes().all(|b| b.is_ascii_alphanumeric())
                {
                    return Err(invalid(
                        "EIP station code must be 7 ASCII alphanumeric bytes (8 in the official legacy example)",
                    ));
                }
                let value = prefixed(take(tokens, index, "EIP intensity")?, "P", 2)?;
                msg.eip.push((code.to_owned(), value.to_owned()));
            }
        }
        _ => {}
    }

    Ok(())
}

fn parse_warning(
    tokens: &[&str],
    index: &mut usize,
    msg: &mut Telegram,
    has_forecast: bool,
) -> Result<(), AppError> {
    msg.EqTime = fixed(
        take(tokens, index, "earthquake time")?,
        12,
        "earthquake time",
    )?
    .to_string();
    msg.EqID = prefixed(take(tokens, index, "ND")?, "ND", 14)?.to_string();
    let ncpn = prefixed(take(tokens, index, "NCPN")?, "NCPN", 2)?;
    let warning_number = ncpn.to_string();
    msg.raw_codes.ncpn = Some(ncpn.to_owned());
    msg.WarnType = "NCPN".to_string();
    msg.WarnNum.clone_from(&warning_number);

    msg.reduced_epicenter = Some(
        fixed(
            take(tokens, index, "reduced epicenter code")?,
            4,
            "reduced epicenter code",
        )?
        .to_owned(),
    );
    msg.ShinouLat = parse_coordinate(take(tokens, index, "latitude")?, 'N', 'S', 3)?;
    msg.ShinouLng = parse_coordinate(take(tokens, index, "longitude")?, 'E', 'W', 4)?;
    msg.ShinouDpth = parse_unknown_i64(take(tokens, index, "depth")?, 3, "depth")?;
    msg.raw_codes.prc = Some(prefixed(take(tokens, index, "PRC")?, "PRC", 5)?.to_owned());

    for (marker, next) in [
        ("CAI", "CPI"),
        ("CPI", "CBI"),
        ("CBI", "PAI"),
        ("PAI", "PPI"),
        ("PPI", "PBI"),
    ] {
        let values = consume_list(tokens, index, marker, next)?;
        match marker {
            "CAI" => msg.raw_codes.cai = values,
            "CPI" => msg.raw_codes.cpi = values,
            "CBI" => msg.raw_codes.cbi = values,
            "PAI" => msg.raw_codes.pai = values,
            "PPI" => msg.raw_codes.ppi = values,
            _ => unreachable!(),
        }
    }
    msg.raw_codes.pbi = consume_list(
        tokens,
        index,
        "PBI",
        if has_forecast { "NCP" } else { "9999=" },
    )?;

    if has_forecast {
        if take(tokens, index, "NCP")? != "NCP" {
            return Err(invalid("expected NCP before forecast block"));
        }
        let outer_id = msg.EqID.clone();
        parse_forecast(tokens, index, msg)?;
        if outer_id != msg.EqID {
            return Err(invalid(
                "outer warning and NCP have different ND identifiers",
            ));
        }
        // Nested NCP fields populate the forecast detail, but the JSON report
        // identity remains the top-level general warning and its NCPN serial.
        if msg.raw_codes.ncpn.is_none() {
            return Err(invalid("missing warning serial"));
        }
        msg.WarnType = "NCPN".to_string();
        msg.WarnNum = warning_number;
    }

    Ok(())
}

fn consume_list(
    tokens: &[&str],
    index: &mut usize,
    marker: &str,
    next: &str,
) -> Result<Vec<String>, AppError> {
    if take(tokens, index, marker)? != marker {
        return Err(invalid(format!("expected {marker}")));
    }
    let start = *index;
    while tokens.get(*index).copied() != Some(next) {
        if *index >= tokens.len() {
            return Err(invalid(format!("unterminated {marker} list")));
        }
        *index += 1;
    }
    if *index == start {
        return Err(invalid(format!("empty {marker} list")));
    }
    let width = if matches!(marker, "CBI" | "PBI") {
        3
    } else {
        4
    };
    tokens[start..*index]
        .iter()
        .map(|t| {
            fixed(t, width, marker)?;
            validate_digits_or_slashes(t, marker)?;
            Ok((*t).to_owned())
        })
        .collect()
}

fn parse_ebi(tokens: &[&str], index: &mut usize) -> Result<Ebi, AppError> {
    let area = fixed(take(tokens, index, "EBI area")?, 3, "EBI area")?;
    let intensity = fixed(take(tokens, index, "EBI intensity")?, 5, "EBI intensity")?;
    let intensity = intensity
        .strip_prefix('S')
        .ok_or_else(|| invalid("EBI intensity must start with S"))?;
    let time = fixed(take(tokens, index, "EBI time")?, 6, "EBI time")?;
    let flags = fixed(take(tokens, index, "EBI flags")?, 2, "EBI flags")?;
    Ok(Ebi {
        Chiiki: area.to_string(),
        Shindo1: intensity[0..2].to_string(),
        Shindo2: intensity[2..4].to_string(),
        Time: time.to_string(),
        Type: flags[0..1].to_string(),
        Arrive: flags[1..2].to_string(),
    })
}

fn take<'a>(tokens: &[&'a str], index: &mut usize, field: &str) -> Result<&'a str, AppError> {
    let token = tokens
        .get(*index)
        .copied()
        .ok_or_else(|| invalid(format!("missing {field}")))?;
    *index += 1;
    Ok(token)
}

fn fixed<'a>(token: &'a str, length: usize, field: &str) -> Result<&'a str, AppError> {
    if token.is_ascii() && token.len() == length {
        Ok(token)
    } else {
        Err(invalid(format!(
            "{field} must be exactly {length} ASCII bytes"
        )))
    }
}

fn prefixed<'a>(token: &'a str, prefix: &str, body_len: usize) -> Result<&'a str, AppError> {
    fixed(token, prefix.len() + body_len, prefix)?
        .strip_prefix(prefix)
        .ok_or_else(|| invalid(format!("expected {prefix}")))
}

fn parse_coordinate(
    token: &str,
    positive: char,
    negative: char,
    digits: usize,
) -> Result<Option<f64>, AppError> {
    fixed(token, digits + 1, "coordinate")?;
    if token[1..].chars().all(|character| character == '/') {
        if token.starts_with([positive, negative, '/']) {
            return Ok(None);
        }
        return Err(invalid("invalid unknown coordinate hemisphere"));
    }
    let sign = match token.as_bytes()[0] as char {
        value if value == positive => 1.0,
        value if value == negative => -1.0,
        _ => return Err(invalid("invalid coordinate hemisphere")),
    };
    let value = token[1..]
        .parse::<u32>()
        .map_err(|_| invalid("invalid coordinate digits"))?;
    let value = sign * f64::from(value) / 10.0;
    if value.abs() > if positive == 'N' { 90.0 } else { 180.0 } {
        return Err(invalid("coordinate out of range"));
    }
    Ok(Some(value))
}

fn parse_unknown_i64(token: &str, length: usize, field: &str) -> Result<Option<i64>, AppError> {
    fixed(token, length, field)?;
    if token.chars().all(|c| c == '/') {
        return Ok(None);
    }
    validate_digits_or_slashes(token, field)?;
    token
        .parse()
        .map(Some)
        .map_err(|_| invalid(format!("invalid {field}")))
}

fn parse_unknown_decimal(token: &str, length: usize, field: &str) -> Result<Option<f64>, AppError> {
    fixed(token, length, field)?;
    if token.chars().all(|c| c == '/') {
        return Ok(None);
    }
    token
        .parse::<u32>()
        .map(|value| Some(f64::from(value) / 10.0))
        .map_err(|_| invalid(format!("invalid {field}")))
}

fn validate_digits_or_slashes(token: &str, field: &str) -> Result<(), AppError> {
    if token.bytes().all(|b| b.is_ascii_digit() || b == b'/') {
        Ok(())
    } else {
        Err(invalid(format!("invalid {field}")))
    }
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::InvalidTelegram(message.into())
}

// CodeType 基本コード
fn code_type(code: &str) -> &'static str {
    match code {
        "35" => "最大予測震度のみの緊急地震速報",
        "36" => "Ｍ、最大予測震度及び主要動到達予測時刻の緊急地震速報",
        "37" => "Ｍ、最大予測震度及び主要動到達予測時刻の緊急地震速報",
        "38" => "テスト電文",
        "39" => "キャンセル報",
        "47" => "一般向け緊急地震速報",
        "48" => "キャンセル報",
        "61" => "リアルタイム震度（工学的基盤面の値）",
        _ => "",
    }
}

// Section 発信官署
fn section(code: &str) -> &'static str {
    match code {
        "01" => "札幌",
        "02" => "仙台",
        "03" => "東京",
        "04" => "大阪",
        "05" => "福岡",
        "06" => "沖縄",
        _ => "",
    }
}

// MsgType 訓練等の識別符
fn msg_type(code: &str) -> &'static str {
    match code {
        "00" => "通常",
        "01" => "訓練",
        "10" => "取り消し",
        "11" => "訓練取り消し",
        "20" => "参考/テスト",
        "30" => "コードのみ配信試験",
        _ => "",
    }
}

// Shindo 震度
fn shindo(code: &str) -> &'static str {
    match code {
        "01" => "1",
        "02" => "2",
        "03" => "3",
        "04" => "4",
        "5-" => "5弱",
        "5+" => "5強",
        "6-" => "6弱",
        "6+" => "6強",
        "07" => "7",
        "//" => "不明",
        _ => "",
    }
}

// Rk1 データの確からしさ
fn rk1(code: &str) -> &'static str {
    match code {
        "1" => "P 波／S 波レベル超え、IPF 法（1 点）、または仮定震源要素",
        "2" => "IPF 法（2 点）",
        "3" => "IPF 法（3 点／4 点）",
        "4" => "IPF 法（5 点以上）",
        "5" => "防災科研システム（4 点以下、または精度情報なし）",
        "6" => "防災科研システム（5 点以上）",
        "7" => "EPOS（海域〔観測網外〕）",
        "8" => "EPOS（内陸〔観測網内〕）",
        "9" => "予備",
        "/" => "不明、未設定時、キャンセル時",
        _ => "",
    }
}

// Rk2 震源の深さの確からしさ
fn rk2(code: &str) -> &'static str {
    match code {
        "1" => "P 波／S 波レベル超え、IPF 法（1 点）、または仮定震源要素",
        "2" => "IPF 法（2 点）",
        "3" => "IPF 法（3 点／4 点）",
        "4" => "IPF 法（5 点以上）",
        "5" => "防災科研システム（4 点以下、または精度情報なし）",
        "6" => "防災科研システム（5 点以上）",
        "7" => "EPOS（海域〔観測網外〕）",
        "8" => "EPOS（内陸〔観測網内〕）",
        "9" => "予備",
        "/" => "不明、未設定時、キャンセル時",
        _ => "",
    }
}

// Rk3 マグニチュードの確からしさ
fn rk3(code: &str, sent_at: &str) -> &'static str {
    match code {
        "1" => "未定義",
        "2" if sent_at >= "230926140000" => "速度マグニチュード",
        "2" => "防災科研システム（Hi-netデータ）",
        "3" => "全点P 相",
        "4" => "P 相／全相混在",
        "5" => "全点全相",
        "6" => "EPOS",
        "7" => "未定義",
        "8" => "P 波／S 波レベル超え、または仮定震源要素",
        "9" => "予備",
        "/" => "不明、未設定時、キャンセル時",
        _ => "",
    }
}

// Rt1 地震の発生場所
fn rt1(code: &str) -> &'static str {
    match code {
        "0" => "内陸",
        "1" => "海域",
        "2" => "未定義",
        "3" => "未定義",
        "4" => "未定義",
        "5" => "未定義",
        "6" => "未定義",
        "7" => "未定義",
        "8" => "未定義",
        "9" => "未定義",
        "/" => "不明、未設定時、キャンセル時、または仮定震源要素の場合",
        _ => "",
    }
}

// Rt2 警報発表対象の識別符
fn rt2(code: &str) -> &'static str {
    match code {
        "0" => "緊急地震速報（予報）",
        "1" => "緊急地震速報（警報）",
        "2" => "未定義",
        "3" => "未定義",
        "4" => "未定義",
        "5" => "未定義",
        "6" => "未定義",
        "7" => "未定義",
        "8" => "未定義",
        "9" => "未定義",
        "/" => "不明、未設定時、キャンセル時",
        _ => "",
    }
}

// Rt3 予測手法
fn rt3(code: &str) -> &'static str {
    match code {
        "0" => "未定義",
        "1" => "未定義",
        "2" => "未定義",
        "3" => "未定義",
        "4" => "未定義",
        "5" => "未定義",
        "6" => "未定義",
        "7" => "未定義",
        "8" => "未定義",
        "9" => {
            "震源とマグニチュードによる震度推定手法において震源要素が推定できず、 PLUM 法による震度予測のみが有効である場合"
        }
        "/" => "不明、未設定時、キャンセル時",
        _ => "",
    }
}

// Rc1 最大予測震度の変化
fn rc1(code: &str) -> &'static str {
    match code {
        "0" => "ほとんど変化なし",
        "1" => "最大予測震度が1.0 以上大きくなった。",
        "2" => "最大予測震度が1.0 以上小さくなった。",
        "3" => "未定義",
        "4" => "未定義",
        "5" => "未定義",
        "6" => "未定義",
        "7" => "未定義",
        "8" => "未定義",
        "9" => "未定義",
        "/" => "不明、未設定時、キャンセル時",
        _ => "",
    }
}

// Rc2 最大予測震度の変化の理由
fn rc2(code: &str) -> &'static str {
    match code {
        "0" => "変化なし",
        "1" => "主としてＭが変化したため(1.0 以上)。",
        "2" => "主として震源位置が変化したため(10.0km 以上)。",
        "3" => "Ｍ及び震源位置が変化したため(1 と2 の複合条件)。",
        "4" => "震源の深さが変化したため(上記のいずれにもあてはまらず、30.0km 以上の 変化)。",
        "5" => "未定義",
        "6" => "未定義",
        "7" => "未定義",
        "8" => "未定義",
        "9" => "PLUM 法による予測により変化したため。",
        "/" => "不明、未設定時、キャンセル時",
        _ => "",
    }
}

// Ebiy1 各地域の警報の識別符
fn ebiy1(code: &str) -> &'static str {
    match code {
        "0" => "予報",
        "1" => "警報",
        "2" => "未定義",
        "3" => "未定義",
        "4" => "未定義",
        "5" => "未定義",
        "6" => "未定義",
        "7" => "未定義",
        "8" => "未定義",
        "9" => "未定義",
        "/" => "不明または未設定時",
        _ => "",
    }
}

// Ebiy2 主要動の到達予測状況
fn ebiy2(code: &str) -> &'static str {
    match code {
        "0" => "未到達",
        "1" => "既に到達と予測",
        "2" => "未定義",
        "3" => "未定義",
        "4" => "未定義",
        "5" => "未定義",
        "6" => "未定義",
        "7" => "未定義",
        "8" => "未定義",
        "9" => "主要動到達時刻の予測なし（PLUM 法による予測）",
        "/" => "不明または未設定時",
        _ => "",
    }
}

// WarnType 電文タイプ
fn warn_type(code: &str) -> &'static str {
    match code {
        "NCN" => "高度利用者向け",
        "NPN" => "一般向け",
        "NCPN" => "一般向け",
        "PRC" => "強い揺れが推定される地域の追加",
        "EBI" => "予想震度と到達時間",
        "CAI" => "新たに追加された強い揺れが推定される地方",
        "CPI" => "新たに追加された強い揺れが推定される県",
        "CBI" => "新たに追加された強い揺れが推定される地域",
        "PAI" => "強い揺れが推定される地方",
        "PPI" => "強い揺れが推定される県",
        "PBI" => "強い揺れが推定される地域",
        _ => "",
    }
}

// WarnCode 発表状況
fn warn_code(code: &str) -> &'static str {
    match code {
        "0" => "通常",
        "6" => "訂正",
        "7" => "訂正(キャンセル誤発表)",
        "8" => "訂正を含む最終",
        "9" => "最終",
        "/" => "不明、未設定時",
        _ => "",
    }
}

fn shinou_code(code: &str) -> &'static str {
    crate::codes::epicenter(code).unwrap_or("")
}

fn chiiki_code(code: &str) -> &'static str {
    crate::codes::region(code).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::telegram_for_json;

    const DATA: &str = include_str!("../data.txt");

    #[test]
    fn data_txt_regression_and_all_ebi_blocks() {
        let decoded = decoder(DATA, telegram_for_json(DATA)).unwrap();
        assert_eq!(decoded.EqID, "20240101161010");
        assert_eq!(decoded.WarnNum, "46");
        assert_eq!(decoded.ShinouLat, Some(37.6));
        assert_eq!(decoded.ShinouLng, Some(137.2));
        assert_eq!(decoded.ShinouDpth, Some(10));
        assert_eq!(decoded.Magnitude, Some(7.4));
        assert_eq!(decoded.Shindo, "7");
        assert_eq!(decoded.Ebis.len(), 45);
        assert!(decoded.Telegram.ends_with('='));
        assert!(!decoded.Telegram.contains('\n'));
    }

    #[test]
    fn single_line_and_multiline_telegrams_decode_identically() {
        let single = telegram_for_json(DATA);
        let multiline = single
            .replacen(" C11 ", " C11\n", 1)
            .replacen(" ND", "\nND", 1)
            .replacen(" 495 ", "\n495 ", 1)
            .replacen(" EBI ", "\nEBI ", 1)
            .replacen(" 9999=", "\n9999=", 1);
        let one = reader(&single).unwrap();
        let many = reader(&multiline).unwrap();
        assert_eq!(one.Ebis.len(), many.Ebis.len());
        assert_eq!(one.EqID, many.EqID);
        assert_eq!(one.Magnitude, many.Magnitude);
    }

    #[test]
    fn south_and_west_coordinates_are_negative() {
        let telegram = "37 03 00 260716090200 C11 260716090107 ND20260716090114 NCN001 JD////////////// JN/// 340 S357 W1407 050 45 03 RK44559 RT00/// RC0//// 9999=";
        let decoded = decoder(telegram, telegram.to_string()).unwrap();
        assert_eq!(decoded.ShinouLat, Some(-35.7));
        assert_eq!(decoded.ShinouLng, Some(-140.7));
    }

    #[test]
    fn slash_accuracy_values_decode_as_unknown() {
        let telegram = "39 03 10 260716090200 C11 260716090107 ND20260716090114 NCN001 JD////////////// JN/// /// N/// E//// /// // // RK///// RT///// RC///// 9999=";
        let decoded = decoder(telegram, telegram.to_string()).unwrap();
        assert_eq!(decoded.CodeType, "キャンセル報");
        assert_eq!(decoded.MsgType, "取り消し");
        assert_eq!(decoded.EqID, "20260716090114");
        assert_eq!(decoded.Rk1, "不明、未設定時、キャンセル時");
        assert_eq!(decoded.Rk2, "不明、未設定時、キャンセル時");
        assert_eq!(decoded.Rt2, "不明、未設定時、キャンセル時");
        assert_eq!(decoded.Rc2, "不明、未設定時、キャンセル時");
    }

    #[test]
    fn incomplete_telegram_is_an_error() {
        let without_end = DATA.trim_end_matches('=').trim_end();
        assert!(matches!(
            reader(without_end),
            Err(AppError::InvalidTelegram(_))
        ));
    }

    #[test]
    fn telegram_json_display_preserves_internal_spaces() {
        let telegram = "37  03 00 260716090200 C11\n\n260716090107\nND20260716090114 NCN001 JD////////////// JN///\n340 N357 E1407 050 45 03 RK44559 RT00/// RC0////\n9999=";
        let display = telegram_for_json(telegram);
        assert!(display.starts_with("37  03"));
        assert!(!display.contains('\n'));
        let decoded = decoder(telegram, display.clone()).unwrap();
        assert_eq!(decoded.Telegram, display);
    }

    #[test]
    fn direct_code_48_warning_cancellation_is_supported() {
        let telegram = "48 03 10 061004150300 C11 061004145930 ND20061004145955 NCPN02 9762 N336 E1362 040 PRC///// CAI //// CPI //// CBI /// PAI //// PPI //// PBI /// 9999=";
        let decoded = decoder(telegram, telegram.to_string()).unwrap();
        assert_eq!(decoded.MsgType, "取り消し");
        assert_eq!(decoded.EqID, "20061004145955");
        assert_eq!(decoded.WarnType, "一般向け");
        assert_eq!(decoded.WarnNum, "02");
    }
}

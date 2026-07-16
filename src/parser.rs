use serde::{Deserialize, Serialize};

use crate::error::AppError;

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
    pub ShinouLat: f64,
    pub ShinouLng: f64,
    pub ShinouDpth: i64,
    pub Magnitude: f64,
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
        EqTime: format!("20{}", from.EqTime),
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
        Rk3: rk3(&from.Rk3).to_string(),
        Rt1: rt1(&from.Rt1).to_string(),
        Rt2: rt2(&from.Rt2).to_string(),
        Rt3: rt3(&from.Rt3).to_string(),
        Rc1: rc1(&from.Rc1).to_string(),
        Rc2: rc2(&from.Rc2).to_string(),
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
        self.WarnCode == "最終"
    }
}

pub fn reader(s: &str) -> Result<Telegram, AppError> {
    let tokens: Vec<&str> = s.split_whitespace().collect();
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

    match msg.CodeType.as_str() {
        "35" | "36" | "37" | "39" | "61" => {
            parse_forecast(&tokens, &mut index, &mut msg)?;
        }
        "47" => parse_warning(&tokens, &mut index, &mut msg, true)?,
        "48" => parse_warning(&tokens, &mut index, &mut msg, false)?,
        "38" => {}
        _ => unreachable!(),
    }

    if take(&tokens, &mut index, "Telegram terminator")? != "9999=" {
        return Err(invalid("expected 9999= terminator"));
    }
    if index != tokens.len() {
        return Err(invalid("unexpected data after 9999="));
    }

    Ok(msg)
}

fn parse_forecast(tokens: &[&str], index: &mut usize, msg: &mut Telegram) -> Result<(), AppError> {
    msg.EqTime = fixed(
        take(tokens, index, "earthquake time")?,
        12,
        "earthquake time",
    )?
    .to_string();
    let nd = prefixed(take(tokens, index, "ND")?, "ND", 14)?;
    msg.EqID = nd.to_string();

    let ncn = prefixed(take(tokens, index, "NCN")?, "NCN", 3)?;
    msg.WarnType = "NCN".to_string();
    msg.WarnCode = ncn[0..1].to_string();
    msg.WarnNum = ncn[1..3].to_string();

    prefixed(take(tokens, index, "JD")?, "JD", 14)?;
    prefixed(take(tokens, index, "JN")?, "JN", 3)?;

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
    msg.Rk1 = rk[0..1].to_string();
    msg.Rk2 = rk[1..2].to_string();
    msg.Rk3 = rk[2..3].to_string();
    msg.Rk4 = rk[3..4].to_string();
    msg.Rk5 = rk[4..5].to_string();

    let rt = prefixed(take(tokens, index, "RT")?, "RT", 5)?;
    msg.Rt1 = rt[0..1].to_string();
    msg.Rt2 = rt[1..2].to_string();
    msg.Rt3 = rt[2..3].to_string();

    let rc = prefixed(take(tokens, index, "RC")?, "RC", 5)?;
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
                fixed(
                    take(tokens, index, "EIP station code")?,
                    7,
                    "EIP station code",
                )?;
                let value = take(tokens, index, "EIP intensity")?;
                if value.len() != 3 || !value.starts_with('P') {
                    return Err(invalid("invalid EIP intensity"));
                }
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
    msg.WarnType = "NCPN".to_string();
    msg.WarnNum.clone_from(&warning_number);

    fixed(
        take(tokens, index, "reduced epicenter code")?,
        4,
        "reduced epicenter code",
    )?;
    msg.ShinouLat = parse_coordinate(take(tokens, index, "latitude")?, 'N', 'S', 3)?;
    msg.ShinouLng = parse_coordinate(take(tokens, index, "longitude")?, 'E', 'W', 4)?;
    msg.ShinouDpth = parse_unknown_i64(take(tokens, index, "depth")?, 3, "depth")?;
    prefixed(take(tokens, index, "PRC")?, "PRC", 5)?;

    for (marker, next) in [
        ("CAI", "CPI"),
        ("CPI", "CBI"),
        ("CBI", "PAI"),
        ("PAI", "PPI"),
        ("PPI", "PBI"),
    ] {
        consume_list(tokens, index, marker, next)?;
    }
    consume_list(
        tokens,
        index,
        "PBI",
        if has_forecast { "NCP" } else { "9999=" },
    )?;

    if has_forecast {
        if take(tokens, index, "NCP")? != "NCP" {
            return Err(invalid("expected NCP before forecast block"));
        }
        parse_forecast(tokens, index, msg)?;
        // Nested NCP fields populate the forecast detail, but the JSON report
        // identity remains the top-level general warning and its NCPN serial.
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
) -> Result<(), AppError> {
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
    Ok(())
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
) -> Result<f64, AppError> {
    fixed(token, digits + 1, "coordinate")?;
    if token[1..].chars().all(|character| character == '/') {
        return Ok(0.0);
    }
    let sign = match token.as_bytes()[0] as char {
        value if value == positive => 1.0,
        value if value == negative => -1.0,
        _ => return Err(invalid("invalid coordinate hemisphere")),
    };
    let value = token[1..]
        .parse::<u32>()
        .map_err(|_| invalid("invalid coordinate digits"))?;
    Ok(sign * f64::from(value) / 10.0)
}

fn parse_unknown_i64(token: &str, length: usize, field: &str) -> Result<i64, AppError> {
    fixed(token, length, field)?;
    if token.chars().all(|c| c == '/') {
        return Ok(0);
    }
    token
        .parse()
        .map_err(|_| invalid(format!("invalid {field}")))
}

fn parse_unknown_decimal(token: &str, length: usize, field: &str) -> Result<f64, AppError> {
    fixed(token, length, field)?;
    if token.chars().all(|c| c == '/') {
        return Ok(0.0);
    }
    token
        .parse::<u32>()
        .map(|value| f64::from(value) / 10.0)
        .map_err(|_| invalid(format!("invalid {field}")))
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
fn rk3(code: &str) -> &'static str {
    match code {
        "1" => "未定義",
        "2" => "防災科研システム",
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
        "8" => "訂正(予報)",
        "9" => "最終",
        "/" => "不明、未設定時",
        _ => "",
    }
}

fn shinou_code(code: &str) -> &'static str {
    match code {
        "000" => "",
        "011" => "北海道地方",
        "012" => "東北地方",
        "013" => "北陸地方",
        "014" => "関東甲信地方",
        "015" => "小笠原地方",
        "016" => "東海地方",
        "017" => "近畿地方",
        "018" => "中国地方",
        "019" => "四国地方",
        "020" => "九州地方",
        "021" => "沖縄地方",
        "100" => "石狩地方北部",
        "101" => "石狩地方中部",
        "102" => "石狩地方南部",
        "105" => "渡島地方北部",
        "106" => "渡島地方東部",
        "107" => "渡島地方西部",
        "110" => "檜山地方",
        "115" => "後志地方北部",
        "116" => "後志地方東部",
        "117" => "後志地方西部",
        "120" => "空知地方北部",
        "121" => "空知地方中部",
        "122" => "空知地方南部",
        "125" => "上川地方北部",
        "126" => "上川地方中部",
        "127" => "上川地方南部",
        "130" => "留萌地方中北部",
        "131" => "留萌地方南部",
        "135" => "宗谷地方北部",
        "136" => "宗谷地方南部",
        "140" => "網走地方",
        "141" => "北見地方",
        "142" => "紋別地方",
        "145" => "胆振地方西部",
        "146" => "胆振地方中東部",
        "150" => "日高地方西部",
        "151" => "日高地方中部",
        "152" => "日高地方東部",
        "155" => "十勝地方北部",
        "156" => "十勝地方中部",
        "157" => "十勝地方南部",
        "160" => "釧路地方北部",
        "161" => "釧路地方中南部",
        "165" => "根室地方北部",
        "166" => "根室地方中部",
        "167" => "根室地方南部",
        "180" => "北海道南西沖",
        "181" => "北海道西方沖",
        "182" => "石狩湾",
        "183" => "北海道北西沖",
        "184" => "宗谷海峡",
        "186" => "国後島付近",
        "187" => "択捉島付近",
        "188" => "北海道東方沖",
        "189" => "根室半島南東沖",
        "190" => "釧路沖",
        "191" => "十勝沖",
        "192" => "浦河沖",
        "193" => "苫小牧沖",
        "194" => "内浦湾",
        "195" => "宗谷東方沖",
        "196" => "網走沖",
        "197" => "択捉島南東沖",
        "200" => "青森県津軽北部",
        "201" => "青森県津軽南部",
        "202" => "青森県三八上北地方",
        "203" => "青森県下北地方",
        "210" => "岩手県沿岸北部",
        "211" => "岩手県沿岸南部",
        "212" => "岩手県内陸北部",
        "213" => "岩手県内陸南部",
        "220" => "宮城県北部",
        "221" => "宮城県南部",
        "222" => "宮城県中部",
        "230" => "秋田県沿岸北部",
        "231" => "秋田県沿岸南部",
        "232" => "秋田県内陸北部",
        "233" => "秋田県内陸南部",
        "240" => "山形県庄内地方",
        "241" => "山形県最上地方",
        "242" => "山形県村山地方",
        "243" => "山形県置賜地方",
        "250" => "福島県中通り",
        "251" => "福島県浜通り",
        "252" => "福島県会津",
        "280" => "津軽海峡",
        "281" => "山形県沖",
        "282" => "秋田県沖",
        "283" => "青森県西方沖",
        "284" => "陸奥湾",
        "285" => "青森県東方沖",
        "286" => "岩手県沖",
        "287" => "宮城県沖",
        "288" => "三陸沖",
        "289" => "福島県沖",
        "300" => "茨城県北部",
        "301" => "茨城県南部",
        "309" => "千葉県南東沖",
        "310" => "栃木県北部",
        "311" => "栃木県南部",
        "320" => "群馬県北部",
        "321" => "群馬県南部",
        "330" => "埼玉県北部",
        "331" => "埼玉県南部",
        "332" => "埼玉県秩父地方",
        "340" => "千葉県北東部",
        "341" => "千葉県北西部",
        "342" => "千葉県南部",
        "349" => "房総半島南方沖",
        "350" => "東京都２３区",
        "351" => "東京都多摩東部",
        "352" => "東京都多摩西部",
        "360" => "神奈川県東部",
        "361" => "神奈川県西部",
        "370" => "新潟県上越地方",
        "371" => "新潟県中越地方",
        "372" => "新潟県下越地方",
        "378" => "新潟県下越沖",
        "379" => "新潟県上中越沖",
        "380" => "富山県東部",
        "381" => "富山県西部",
        "390" => "石川県能登地方",
        "391" => "石川県加賀地方",
        "400" => "福井県嶺北",
        "401" => "福井県嶺南",
        "411" => "山梨県中・西部",
        "412" => "山梨県東部・富士五湖",
        "420" => "長野県北部",
        "421" => "長野県中部",
        "422" => "長野県南部",
        "430" => "岐阜県飛騨地方",
        "431" => "岐阜県美濃東部",
        "432" => "岐阜県美濃中西部",
        "440" => "静岡県伊豆地方",
        "441" => "静岡県東部",
        "442" => "静岡県中部",
        "443" => "静岡県西部",
        "450" => "愛知県東部",
        "451" => "愛知県西部",
        "460" => "三重県北部",
        "461" => "三重県中部",
        "462" => "三重県南部",
        "469" => "三重県南東沖",
        "471" => "茨城県沖",
        "472" => "関東東方沖",
        "473" => "千葉県東方沖",
        "475" => "八丈島東方沖",
        "476" => "八丈島近海",
        "477" => "東京湾",
        "478" => "相模湾",
        "480" => "伊豆大島近海",
        "481" => "伊豆半島東方沖",
        "482" => "三宅島近海",
        "483" => "新島・神津島近海",
        "485" => "駿河湾",
        "486" => "駿河湾南方沖",
        "487" => "遠州灘",
        "489" => "三河湾",
        "490" => "伊勢湾",
        "492" => "若狭湾",
        "493" => "福井県沖",
        "494" => "石川県西方沖",
        "495" => "能登半島沖",
        "497" => "富山湾",
        "498" => "佐渡付近",
        "499" => "東海道南方沖",
        "500" => "滋賀県北部",
        "501" => "滋賀県南部",
        "510" => "京都府北部",
        "511" => "京都府南部",
        "520" => "大阪府北部",
        "521" => "大阪府南部",
        "530" => "兵庫県北部",
        "531" => "兵庫県南東部",
        "532" => "兵庫県南西部",
        "540" => "奈良県",
        "550" => "和歌山県北部",
        "551" => "和歌山県南部",
        "560" => "鳥取県東部",
        "562" => "鳥取県中部",
        "563" => "鳥取県西部",
        "570" => "島根県東部",
        "571" => "島根県西部",
        "580" => "岡山県北部",
        "581" => "岡山県南部",
        "590" => "広島県北部",
        "591" => "広島県南東部",
        "592" => "広島県南西部",
        "600" => "徳島県北部",
        "601" => "徳島県南部",
        "610" => "香川県東部",
        "611" => "香川県西部",
        "620" => "愛媛県東予",
        "621" => "愛媛県中予",
        "622" => "愛媛県南予",
        "630" => "高知県東部",
        "631" => "高知県中部",
        "632" => "高知県西部",
        "673" => "土佐湾",
        "674" => "紀伊水道",
        "675" => "大阪湾",
        "676" => "播磨灘",
        "677" => "瀬戸内海中部",
        "678" => "安芸灘",
        "679" => "周防灘",
        "680" => "伊予灘",
        "681" => "豊後水道",
        "682" => "山口県北西沖",
        "683" => "島根県沖",
        "684" => "鳥取県沖",
        "685" => "隠岐島近海",
        "686" => "兵庫県北方沖",
        "687" => "京都府沖",
        "688" => "淡路島付近",
        "689" => "和歌山県南方沖",
        "700" => "山口県北部",
        "702" => "山口県西部",
        "703" => "山口県東部",
        "704" => "山口県中部",
        "710" => "福岡県福岡地方",
        "711" => "福岡県北九州地方",
        "712" => "福岡県筑豊地方",
        "713" => "福岡県筑後地方",
        "720" => "佐賀県北部",
        "721" => "佐賀県南部",
        "730" => "長崎県北部",
        "731" => "長崎県南西部",
        "732" => "長崎県島原半島",
        "740" => "熊本県阿蘇地方",
        "741" => "熊本県熊本地方",
        "742" => "熊本県球磨地方",
        "743" => "熊本県天草・芦北地方",
        "750" => "大分県北部",
        "751" => "大分県中部",
        "752" => "大分県南部",
        "753" => "大分県西部",
        "760" => "宮崎県北部平野部",
        "761" => "宮崎県北部山沿い",
        "762" => "宮崎県南部平野部",
        "763" => "宮崎県南部山沿い",
        "770" => "鹿児島県薩摩地方",
        "771" => "鹿児島県大隅地方",
        "783" => "五島列島近海",
        "784" => "天草灘",
        "785" => "有明海",
        "786" => "橘湾",
        "787" => "鹿児島湾",
        "790" => "種子島近海",
        "791" => "日向灘",
        "793" => "奄美大島近海",
        "795" => "壱岐・対馬近海",
        "796" => "福岡県北西沖",
        "797" => "薩摩半島西方沖",
        "798" => "トカラ列島近海",
        "799" => "奄美大島北西沖",
        "820" => "大隅半島東方沖",
        "821" => "九州地方南東沖",
        "822" => "種子島南東沖",
        "823" => "奄美大島北東沖",
        "850" => "沖縄本島近海",
        "851" => "南大東島近海",
        "852" => "沖縄本島南方沖",
        "853" => "宮古島近海",
        "854" => "石垣島近海",
        "855" => "石垣島南方沖",
        "856" => "西表島付近",
        "857" => "与那国島近海",
        "858" => "沖縄本島北西沖",
        "859" => "宮古島北西沖",
        "860" => "石垣島北西沖",
        "900" => "台湾付近",
        "901" => "東シナ海",
        "902" => "四国沖",
        "903" => "鳥島近海",
        "904" => "鳥島東方沖",
        "905" => "オホーツク海南部",
        "906" => "サハリン西方沖",
        "907" => "日本海北部",
        "908" => "日本海中部",
        "909" => "日本海西部",
        "911" => "父島近海",
        "912" => "千島列島",
        "913" => "千島列島南東沖",
        "914" => "北海道南東沖",
        "915" => "東北地方東方沖",
        "916" => "小笠原諸島西方沖",
        "917" => "硫黄島近海",
        "918" => "小笠原諸島東方沖",
        "919" => "南海道南方沖",
        "920" => "薩南諸島東方沖",
        "921" => "本州南方沖",
        "922" => "サハリン南部付近",
        "930" => "北西太平洋",
        "932" => "マリアナ諸島",
        "933" => "黄海",
        "934" => "朝鮮半島南部",
        "935" => "朝鮮半島北部",
        "936" => "中国東北部",
        "937" => "ウラジオストク付近",
        "938" => "シベリア南部",
        "939" => "サハリン近海",
        "940" => "アリューシャン列島",
        "941" => "カムチャツカ半島付近",
        "942" => "北米西部",
        "943" => "北米中部",
        "944" => "北米東部",
        "945" => "中米",
        "946" => "南米西部",
        "947" => "南米中部",
        "948" => "南米東部",
        "949" => "北東太平洋",
        "950" => "南太平洋",
        "951" => "インドシナ半島付近",
        "952" => "フィリピン付近",
        "953" => "インドネシア付近",
        "954" => "グアム付近",
        "955" => "ニューギニア付近",
        "956" => "ニュージーランド付近",
        "957" => "オーストラリア付近",
        "958" => "シベリア付近",
        "959" => "ロシア西部",
        "960" => "ロシア中部",
        "961" => "ロシア東部",
        "962" => "中央アジア",
        "963" => "中国西部",
        "964" => "中国中部",
        "965" => "中国東部",
        "966" => "インド付近",
        "967" => "インド洋",
        "968" => "中東",
        "969" => "ヨーロッパ西部",
        "970" => "ヨーロッパ中部",
        "971" => "ヨーロッパ東部",
        "972" => "地中海",
        "973" => "アフリカ西部",
        "974" => "アフリカ中部",
        "975" => "アフリカ東部",
        "976" => "北大西洋",
        "977" => "南大西洋",
        "978" => "北極付近",
        "979" => "南極付近",
        "999" => "遠地",
        _ => "",
    }
}

fn chiiki_code(code: &str) -> &'static str {
    match code {
        "100" => "石狩地方北部",
        "101" => "石狩地方中部",
        "102" => "石狩地方南部",
        "105" => "渡島地方北部",
        "106" => "渡島地方東部",
        "107" => "渡島地方西部",
        "110" => "檜山地方",
        "115" => "後志地方北部",
        "116" => "後志地方東部",
        "117" => "後志地方西部",
        "119" => "北海道奥尻島",
        "120" => "空知地方北部",
        "121" => "空知地方中部",
        "122" => "空知地方南部",
        "125" => "上川地方北部",
        "126" => "上川地方中部",
        "127" => "上川地方南部",
        "130" => "留萌地方中北部",
        "131" => "留萌地方南部",
        "135" => "宗谷地方北部",
        "136" => "宗谷地方南部",
        "139" => "北海道利尻礼文",
        "140" => "網走地方",
        "141" => "北見地方",
        "142" => "紋別地方",
        "145" => "胆振地方西部",
        "146" => "胆振地方中東部",
        "150" => "日高地方西部",
        "151" => "日高地方中部",
        "152" => "日高地方東部",
        "155" => "十勝地方北部",
        "156" => "十勝地方中部",
        "157" => "十勝地方南部",
        "160" => "釧路地方北部",
        "161" => "釧路地方中南部",
        "165" => "根室地方北部",
        "166" => "根室地方中部",
        "167" => "根室地方南部",
        "200" => "青森県津軽北部",
        "201" => "青森県津軽南部",
        "202" => "青森県三八上北",
        "203" => "青森県下北",
        "210" => "岩手県沿岸北部",
        "211" => "岩手県沿岸南部",
        "212" => "岩手県内陸北部",
        "213" => "岩手県内陸南部",
        "220" => "宮城県北部",
        "221" => "宮城県南部",
        "222" => "宮城県中部",
        "230" => "秋田県沿岸北部",
        "231" => "秋田県沿岸南部",
        "232" => "秋田県内陸北部",
        "233" => "秋田県内陸南部",
        "240" => "山形県庄内",
        "241" => "山形県最上",
        "242" => "山形県村山",
        "243" => "山形県置賜",
        "250" => "福島県中通り",
        "251" => "福島県浜通り",
        "252" => "福島県会津",
        "300" => "茨城県北部",
        "301" => "茨城県南部",
        "310" => "栃木県北部",
        "311" => "栃木県南部",
        "320" => "群馬県北部",
        "321" => "群馬県南部",
        "330" => "埼玉県北部",
        "331" => "埼玉県南部",
        "332" => "埼玉県秩父",
        "340" => "千葉県北東部",
        "341" => "千葉県北西部",
        "342" => "千葉県南部",
        "350" => "東京都２３区",
        "351" => "東京都多摩東部",
        "352" => "東京都多摩西部",
        "354" => "神津島",
        "355" => "伊豆大島",
        "356" => "新島",
        "357" => "三宅島",
        "358" => "八丈島",
        "359" => "小笠原",
        "360" => "神奈川県東部",
        "361" => "神奈川県西部",
        "370" => "新潟県上越",
        "371" => "新潟県中越",
        "372" => "新潟県下越",
        "375" => "新潟県佐渡",
        "380" => "富山県東部",
        "381" => "富山県西部",
        "390" => "石川県能登",
        "391" => "石川県加賀",
        "400" => "福井県嶺北",
        "401" => "福井県嶺南",
        "411" => "山梨県中・西部",
        "412" => "山梨県東部・富士五湖",
        "420" => "長野県北部",
        "421" => "長野県中部",
        "422" => "長野県南部",
        "430" => "岐阜県飛騨",
        "431" => "岐阜県美濃東部",
        "432" => "岐阜県美濃中西部",
        "440" => "静岡県伊豆",
        "441" => "静岡県東部",
        "442" => "静岡県中部",
        "443" => "静岡県西部",
        "450" => "愛知県東部",
        "451" => "愛知県西部",
        "460" => "三重県北部",
        "461" => "三重県中部",
        "462" => "三重県南部",
        "500" => "滋賀県北部",
        "501" => "滋賀県南部",
        "510" => "京都府北部",
        "511" => "京都府南部",
        "520" => "大阪府北部",
        "521" => "大阪府南部",
        "530" => "兵庫県北部",
        "531" => "兵庫県南東部",
        "532" => "兵庫県南西部",
        "535" => "兵庫県淡路島",
        "540" => "奈良県",
        "550" => "和歌山県北部",
        "551" => "和歌山県南部",
        "560" => "鳥取県東部",
        "562" => "鳥取県中部",
        "563" => "鳥取県西部",
        "570" => "島根県東部",
        "571" => "島根県西部",
        "575" => "島根県隠岐",
        "580" => "岡山県北部",
        "581" => "岡山県南部",
        "590" => "広島県北部",
        "591" => "広島県南東部",
        "592" => "広島県南西部",
        "600" => "徳島県北部",
        "601" => "徳島県南部",
        "610" => "香川県東部",
        "611" => "香川県西部",
        "620" => "愛媛県東予",
        "621" => "愛媛県中予",
        "622" => "愛媛県南予",
        "630" => "高知県東部",
        "631" => "高知県中部",
        "632" => "高知県西部",
        "700" => "山口県北部",
        "702" => "山口県西部",
        "703" => "山口県東部",
        "704" => "山口県中部",
        "710" => "福岡県福岡",
        "711" => "福岡県北九州",
        "712" => "福岡県筑豊",
        "713" => "福岡県筑後",
        "720" => "佐賀県北部",
        "721" => "佐賀県南部",
        "730" => "長崎県北部",
        "731" => "長崎県南西部",
        "732" => "長崎県島原半島",
        "735" => "長崎県対馬",
        "736" => "長崎県壱岐",
        "737" => "長崎県五島",
        "740" => "熊本県阿蘇",
        "741" => "熊本県熊本",
        "742" => "熊本県球磨",
        "743" => "熊本県天草・芦北",
        "750" => "大分県北部",
        "751" => "大分県中部",
        "752" => "大分県南部",
        "753" => "大分県西部",
        "760" => "宮崎県北部平野部",
        "761" => "宮崎県北部山沿い",
        "762" => "宮崎県南部平野部",
        "763" => "宮崎県南部山沿い",
        "770" => "鹿児島県薩摩",
        "771" => "鹿児島県大隅",
        "774" => "鹿児島県十島村",
        "775" => "鹿児島県甑島",
        "776" => "鹿児島県種子島",
        "777" => "鹿児島県屋久島",
        "778" => "鹿児島県奄美北部",
        "779" => "鹿児島県奄美南部",
        "800" => "沖縄県本島北部",
        "801" => "沖縄県本島中南部",
        "802" => "沖縄県久米島",
        "803" => "沖縄県大東島",
        "804" => "沖縄県宮古島",
        "805" => "沖縄県石垣島",
        "806" => "沖縄県与那国島",
        "807" => "沖縄県西表島",
        _ => "",
    }
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
        assert_eq!(decoded.ShinouLat, 37.6);
        assert_eq!(decoded.ShinouLng, 137.2);
        assert_eq!(decoded.ShinouDpth, 10);
        assert_eq!(decoded.Magnitude, 7.4);
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
        assert_eq!(decoded.ShinouLat, -35.7);
        assert_eq!(decoded.ShinouLng, -140.7);
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

//! Semantic EEW information. Optional members distinguish absent data from
//! explicit unknown quantity values (`value: null`). Codes remain strings so
//! future JMA code values are preserved.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Product {
    #[serde(rename = "VXSE42")]
    Test,
    #[serde(rename = "VXSE43")]
    Warning,
    #[serde(rename = "VXSE44")]
    Forecast,
    #[serde(rename = "VXSE45")]
    GroundMotionForecast,
    #[serde(rename = "VXSE47")]
    RealTimeIntensity,
    /// Code Telegram does not identify VXSE44 versus VXSE45.
    TelegramForecast,
    #[default]
    Unknown,
}
impl Product {
    pub fn from_control_title(title: &str) -> Self {
        match title {
            "緊急地震速報配信テスト" | "緊急地震速報テスト" => Self::Test,
            "緊急地震速報（警報）" => Self::Warning,
            "緊急地震速報（予報）" => Self::Forecast,
            "緊急地震速報（地震動予報）" => Self::GroundMotionForecast,
            "リアルタイム震度" => Self::RealTimeIntensity,
            _ => Self::Unknown,
        }
    }
    pub fn is_forecast(self) -> bool {
        matches!(
            self,
            Self::Forecast | Self::GroundMotionForecast | Self::TelegramForecast
        )
    }
}

macro_rules! text_enum {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub enum $name { $($variant,)+ Other(String) }
        impl $name {
            pub fn as_str(&self) -> &str { match self { $(Self::$variant => $text,)+ Self::Other(s) => s } }
            pub fn from_text(s: &str) -> Self { match s { $($text => Self::$variant,)+ _ => Self::Other(s.to_owned()) } }
        }
        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> { s.serialize_str(self.as_str()) }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                Ok(Self::from_text(&String::deserialize(d)?))
            }
        }
    }
}
text_enum!(Status { Normal => "通常", Training => "訓練", Test => "試験" });
text_enum!(InfoType { Publication => "発表", Correction => "訂正", Cancellation => "取消", Delayed => "遅延" });
text_enum!(IntensityValue { Zero => "0", One => "1", Two => "2", Three => "3", Four => "4", FiveLower => "5-", FiveUpper => "5+", SixLower => "6-", SixUpper => "6+", Seven => "7", Over => "over", Unknown => "不明" });
text_enum!(LgIntensityValue { Zero => "0", One => "1", Two => "2", Three => "3", Four => "4", Over => "over", Unknown => "不明" });

impl IntensityValue {
    pub fn rank(&self) -> Option<u8> {
        Some(match self {
            Self::Zero => 0,
            Self::One => 1,
            Self::Two => 2,
            Self::Three => 3,
            Self::Four => 4,
            Self::FiveLower => 5,
            Self::FiveUpper => 6,
            Self::SixLower => 7,
            Self::SixUpper => 8,
            Self::Seven => 9,
            Self::Over => u8::MAX,
            Self::Unknown | Self::Other(_) => return None,
        })
    }
}
impl LgIntensityValue {
    pub fn rank(&self) -> Option<u8> {
        Some(match self {
            Self::Zero => 0,
            Self::One => 1,
            Self::Two => 2,
            Self::Three => 3,
            Self::Four => 4,
            Self::Over => u8::MAX,
            Self::Unknown | Self::Other(_) => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EewInformation {
    #[serde(flatten)]
    pub header: Header,
    #[serde(default)]
    pub product: Product,
    pub body: Body,
    /// Exact input, never an XML-derived imitation of a received Telegram.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}
impl EewInformation {
    pub fn event_id(&self) -> Option<&str> {
        self.header.event_id.as_deref()
    }
    pub fn serial(&self) -> Option<u32> {
        self.header.serial_no.as_deref()?.parse().ok()
    }
    pub fn product(&self) -> Product {
        if self.product == Product::Unknown {
            self.header
                .information_type
                .as_deref()
                .map(Product::from_control_title)
                .unwrap_or_default()
        } else {
            self.product
        }
    }
    /// Strict raw JMA final-report marker; cannot be inferred from a converted
    /// reference JSON's `isLastInfo` alone.
    pub fn is_final(&self) -> bool {
        self.source.as_ref().is_some_and(|s| s.final_report)
    }
    pub fn message_time(&self) -> Option<&str> {
        self.header
            .report_date_time
            .as_deref()
            .or_else(|| self.source.as_ref().and_then(|s| s.sent_at.as_deref()))
    }
    pub fn is_last_info(&self) -> bool {
        self.body.is_last_info
    }
    pub fn is_canceled(&self) -> bool {
        self.body.is_canceled
    }
    pub fn is_warning(&self) -> Option<bool> {
        self.body.is_warning
    }
    pub fn is_training_or_test(&self) -> Option<bool> {
        if self.product() == Product::Test {
            return Some(true);
        }
        match self.header.status.as_ref()? {
            Status::Normal => Some(false),
            Status::Training | Status::Test => Some(true),
            Status::Other(_) => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub information_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info_type: Option<InfoType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editorial_office: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publishing_office: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub press_date_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_date_time: Option<String>,
    pub target_date_time: Option<String>,
    pub event_id: Option<String>,
    pub serial_no: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info_kind_version: Option<String>,
    pub headline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_date_time_dubious: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_duration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_date_time: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Body {
    pub is_last_info: bool,
    pub is_canceled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_warning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zones: Option<Vec<WarningArea>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefectures: Option<Vec<WarningArea>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regions: Option<Vec<WarningArea>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub earthquake: Option<Earthquake>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intensity: Option<Intensity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comments: Option<Comments>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_advisory: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeName {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WarningKind {
    #[serde(flatten)]
    pub current: CodeName,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_kind: Option<CodeName>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarningArea {
    #[serde(flatten)]
    pub area: CodeName,
    pub kind: WarningKind,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Earthquake {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrival_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    pub hypocenter: Hypocenter,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub magnitude: Option<Quantity>,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hypocenter {
    #[serde(flatten)]
    pub area: CodeName,
    pub coordinate: Coordinate,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<Quantity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce: Option<CodeName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub land_or_sea: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accuracy: Option<Accuracy>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Coordinate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<CoordinateAxis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<CoordinateAxis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<Quantity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geodetic_system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinateAxis {
    pub text: String,
    pub value: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuantityBound {
    #[serde(rename = "lower")]
    Lower,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quantity {
    #[serde(rename = "type")]
    pub kind: String,
    pub unit: String,
    /// Telegram pattern 2 magnitude is a lower bound, unlike a point estimate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound: Option<QuantityBound>,
    /// None is an explicitly unknown value, serialized as null.
    pub value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}
impl Quantity {
    pub fn value_f64(&self) -> Option<f64> {
        self.value.as_deref()?.parse().ok()
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Accuracy {
    pub epicenters: [Option<String>; 2],
    pub depth: Option<String>,
    pub magnitude_calculation: Option<String>,
    pub number_of_magnitude_calculation: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForecastRange<T> {
    pub from: T,
    pub to: T,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Intensity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forecast_max_int: Option<ForecastRange<IntensityValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forecast_max_lg_int: Option<ForecastRange<LgIntensityValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub appendix: Option<Appendix>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regions: Option<Vec<ForecastRegion>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stations: Option<Vec<Station>>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Appendix {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_int_change: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_lg_int_change: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_int_change_reason: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForecastRegion {
    #[serde(flatten)]
    pub area: CodeName,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_plum: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_warning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forecast_max_int: Option<ForecastRange<IntensityValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forecast_max_lg_int: Option<ForecastRange<LgIntensityValue>>,
    pub kind: CodeName,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrival_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrival_time_of_day: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Station {
    #[serde(flatten)]
    pub station: CodeName,
    pub int: IntensityValue,
    pub k: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comments {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<WarningComment>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarningComment {
    pub text: String,
    pub codes: Vec<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceFormat {
    Xml,
    Telegram,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub format: SourceFormat,
    pub raw: String,
    pub final_report: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_table_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telegram: Option<TelegramCodes>,
}
/// The old 2013 manual defines NCN correction states 6/7/8. The current
/// manual defines only 0/9/unset; no exact retirement date is assumed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NcnConvention {
    #[default]
    Current,
    Legacy2013,
}
/// Fields with no XML equivalent stay here, including unset '/' values.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramCodes {
    #[serde(default)]
    pub ncn_convention: NcnConvention,
    pub code_type: String,
    pub section: String,
    pub message_type: String,
    pub command: String,
    pub ncn: Option<String>,
    pub ncpn: Option<String>,
    pub jd: Option<String>,
    pub jn: Option<String>,
    pub rk: Option<String>,
    pub rt: Option<String>,
    pub rc: Option<String>,
    pub prc: Option<String>,
    pub cai: Vec<String>,
    pub cpi: Vec<String>,
    pub cbi: Vec<String>,
    pub pai: Vec<String>,
    pub ppi: Vec<String>,
    pub pbi: Vec<String>,
    pub event_time: Option<String>,
    pub ebi_times: Vec<String>,
    pub eip: Vec<(String, String)>,
    pub maximum_intensity: Option<String>,
}

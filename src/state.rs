//! Optional, in-memory delivery policy. Parsing itself is stateless.
use crate::{EewInformation, InfoType, NcnConvention, Product, Status};
use chrono::{DateTime, FixedOffset};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProductFamily {
    Forecast,
    Warning,
    RealTimeIntensity,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EventKey {
    pub event_id: String,
    pub product: ProductFamily,
    pub status: Status,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Lifecycle {
    #[default]
    Active,
    Final,
    Canceled,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventState {
    /// Highest accepted ordinary report serial; cancellation never lowers it.
    pub latest_serial: Option<u32>,
    /// Whether an accepted report carried a final marker. Cancellation is separate.
    pub is_final: bool,
    pub is_cancelled: bool,
    pub lifecycle: Lifecycle,
    pub latest_message_time: Option<DateTime<FixedOffset>>,
    pub cancellation_serial: Option<u32>,
    last_fingerprint: Option<u64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventDecision {
    Accepted,
    Duplicate,
    OlderThanLatest,
    Final,
    Cancelled {
        existed: bool,
    },
    /// The default policy never revives a canceled series automatically.
    AfterCancellation,
    /// A correction after cancellation needs application policy; no resurrection.
    CorrectionAfterCancellation,
    IgnoredTest,
    MissingIdentity,
    /// An explicit historical NCN7 corrects an erroneous cancellation.
    Reinstated,
}
#[derive(Debug, Default)]
pub struct EventTracker {
    events: HashMap<EventKey, EventState>,
}
impl EventTracker {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn key(message: &EewInformation) -> Option<EventKey> {
        let product = match message.product() {
            Product::Warning => ProductFamily::Warning,
            Product::RealTimeIntensity => ProductFamily::RealTimeIntensity,
            p if p.is_forecast() => ProductFamily::Forecast,
            _ => return None,
        };
        Some(EventKey {
            event_id: message.event_id()?.to_owned(),
            product,
            status: message.header.status.clone()?,
        })
    }
    pub fn observe(&mut self, message: &EewInformation) -> EventDecision {
        if message.product() == Product::Test {
            return EventDecision::IgnoredTest;
        }
        let Some(key) = Self::key(message) else {
            return EventDecision::MissingIdentity;
        };
        let existed = self.events.contains_key(&key);
        let state = self.events.entry(key).or_default();
        // JMA revisions are ordered by Control/DateTime, not report time or serial.
        let time = message
            .header
            .press_date_time
            .as_deref()
            .or_else(|| message.message_time())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok());
        let fingerprint = fingerprint(message);
        if state.last_fingerprint == Some(fingerprint) {
            return EventDecision::Duplicate;
        }
        if message.is_canceled() {
            if state.is_cancelled
                && time == state.latest_message_time
                && message.serial() == state.cancellation_serial
            {
                return EventDecision::Duplicate;
            }
            if older(time, state.latest_message_time) {
                return EventDecision::OlderThanLatest;
            }
            state.is_cancelled = true;
            state.lifecycle = Lifecycle::Canceled;
            state.cancellation_serial = message.serial();
            state.latest_message_time = time.or(state.latest_message_time);
            state.last_fingerprint = Some(fingerprint);
            return EventDecision::Cancelled { existed };
        }
        let correction = message.header.info_type == Some(InfoType::Correction);
        let reinstate = state.is_cancelled
            && message
                .source
                .as_ref()
                .and_then(|s| s.telegram.as_ref())
                .is_some_and(|c| {
                    c.ncn_convention == NcnConvention::Legacy2013
                        && c.ncn.as_deref().is_some_and(|n| n.starts_with('7'))
                });
        if state.is_cancelled && !reinstate {
            return if correction {
                EventDecision::CorrectionAfterCancellation
            } else {
                EventDecision::AfterCancellation
            };
        }
        if older(time, state.latest_message_time) {
            return EventDecision::OlderThanLatest;
        }
        if let (Some(serial), Some(latest)) = (message.serial(), state.latest_serial) {
            if serial < latest && !reinstate {
                return EventDecision::OlderThanLatest;
            }
            if serial == latest && !correction && (!message.is_last_info() || state.is_final) {
                return EventDecision::Duplicate;
            }
        }
        if let Some(serial) = message.serial() {
            state.latest_serial = Some(state.latest_serial.map_or(serial, |n| n.max(serial)));
        }
        if reinstate {
            state.is_cancelled = false;
        }
        state.is_final = message.is_final() || message.is_last_info();
        state.lifecycle = if state.is_final {
            Lifecycle::Final
        } else {
            Lifecycle::Active
        };
        state.latest_message_time = time.or(state.latest_message_time);
        state.last_fingerprint = Some(fingerprint);
        if reinstate {
            EventDecision::Reinstated
        } else if state.is_final {
            EventDecision::Final
        } else {
            EventDecision::Accepted
        }
    }
    /// Explicit product/status lookup. Forecast covers VXSE44/45 and legacy
    /// Telegram forecasts; warning and real-time intensity are independent.
    pub fn state(&self, key: &EventKey) -> Option<&EventState> {
        self.events.get(key)
    }
    /// Explicitly forget a series (for retention or application-approved restart).
    pub fn remove(&mut self, key: &EventKey) -> Option<EventState> {
        self.events.remove(key)
    }
    pub fn clear(&mut self) {
        self.events.clear();
    }
    pub fn len(&self) -> usize {
        self.events.len()
    }
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}
fn older(a: Option<DateTime<FixedOffset>>, b: Option<DateTime<FixedOffset>>) -> bool {
    matches!((a,b), (Some(a),Some(b)) if a < b)
}
fn fingerprint(m: &EewInformation) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    m.header.serial_no.hash(&mut h);
    m.header.info_type.hash(&mut h);
    m.header.report_date_time.hash(&mut h);
    m.header.press_date_time.hash(&mut h);
    m.body.is_last_info.hash(&mut h);
    m.body.is_canceled.hash(&mut h);
    if let Some(s) = &m.source {
        s.raw.hash(&mut h);
    }
    h.finish()
}

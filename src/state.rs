use std::collections::HashMap;

use crate::EewOutcome;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventState {
    pub latest_serial: Option<u32>,
    pub is_final: bool,
    pub is_cancelled: bool,
    last_serial_token: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventDecision {
    Accepted,
    Duplicate,
    OlderThanLatest,
    Final,
    Cancelled { existed: bool },
}

#[derive(Debug, Default)]
pub struct EventTracker {
    events: HashMap<String, EventState>,
}

impl EventTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Classifies an outcome and updates in-memory state. It never suppresses,
    /// persists, broadcasts, or otherwise consumes the caller's event.
    pub fn observe(&mut self, outcome: &EewOutcome) -> EventDecision {
        match outcome {
            EewOutcome::Cancellation(cancellation) => {
                let existed = self.events.contains_key(&cancellation.event_id);
                self.events
                    .entry(cancellation.event_id.clone())
                    .or_default()
                    .is_cancelled = true;
                EventDecision::Cancelled { existed }
            }
            EewOutcome::Report(report) => {
                let state = self
                    .events
                    .entry(report.event_id().to_string())
                    .or_default();
                let token = report.serial_token();
                let decision = match report.serial() {
                    Some(serial) => match state.latest_serial {
                        Some(latest) if serial == latest => EventDecision::Duplicate,
                        Some(latest) if serial < latest => EventDecision::OlderThanLatest,
                        _ => {
                            state.latest_serial = Some(serial);
                            state.last_serial_token = token.map(str::to_string);
                            EventDecision::Accepted
                        }
                    },
                    None if token.is_some() && state.last_serial_token.as_deref() == token => {
                        EventDecision::Duplicate
                    }
                    None => {
                        state.last_serial_token = token.map(str::to_string);
                        EventDecision::Accepted
                    }
                };

                if decision == EventDecision::Accepted && report.is_final() {
                    state.is_final = true;
                    EventDecision::Final
                } else {
                    decision
                }
            }
        }
    }

    pub fn state(&self, event_id: &str) -> Option<&EventState> {
        self.events.get(event_id)
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

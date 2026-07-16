use eew_parser::{EewOutcome, EventDecision, EventTracker, parse_auto};

fn handle_payload(
    tracker: &mut EventTracker,
    payload: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let outcome = parse_auto(payload)?;
    let decision = tracker.observe(&outcome);

    match &outcome {
        EewOutcome::Report(report) => {
            // Duplicate/older policy belongs to the application. Serialize only
            // here, at the example's hypothetical WebSocket/API boundary.
            if !matches!(
                decision,
                EventDecision::Duplicate | EventDecision::OlderThanLatest
            ) {
                println!("{}", serde_json::to_string(report)?);
            }
        }
        EewOutcome::Cancellation(cancel) => {
            // Broadcast cancellation even when decision is Cancelled { existed: false }.
            println!("{}", serde_json::to_string(cancel)?);
        }
    }

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let payload: &[u8] = include_bytes!("../data.txt");
    let mut tracker = EventTracker::new();
    handle_payload(&mut tracker, payload)
}

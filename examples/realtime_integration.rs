use eew_parser::{EventDecision, EventTracker, parse_auto};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let information = parse_auto(include_bytes!("../data.txt"))?;
    let mut tracker = EventTracker::new();
    if matches!(
        tracker.observe(&information),
        EventDecision::Accepted | EventDecision::Final | EventDecision::Cancelled { .. }
    ) {
        println!("{}", serde_json::to_string(&information)?);
    }
    Ok(())
}

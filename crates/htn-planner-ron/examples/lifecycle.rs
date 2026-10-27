//! A standalone warehouse lifecycle with host-owned progress and persistence.
#[path = "support/lifecycle.rs"]
mod lifecycle;
use htn_planner::Value;
use lifecycle::{Event, Host, Observation};
use std::io::Write;
fn report(event: &Event) -> Result<(), String> {
    writeln!(std::io::stdout(), "{event:?}").map_err(|error| error.to_string())
}
fn main() -> Result<(), String> {
    let domain = lifecycle::domain().map_err(|error| format!("{error:?}"))?;
    let observation = Observation::ready(7, 1);
    let mut host = Host::new(&domain);
    report(&host.decide(&domain, &observation, 0, &[Value::Int(7)]))?;
    report(&host.advance(&domain, &observation, 0))?;
    report(&host.advance(&domain, &observation, 1))?;
    let saved = ron::ser::to_string(&host.checkpoint()).map_err(|error| error.to_string())?;
    let decoded = ron::from_str(&saved).map_err(|error| error.to_string())?;
    let mut host = Host::resume(&domain, decoded).map_err(str::to_owned)?;
    report(&host.decide(&domain, &observation, 2, &[Value::Int(7)]))?;
    report(&host.advance(&domain, &observation, 2))?;
    report(&host.advance(&domain, &observation, 2))?;
    let mut fresh = Observation::ready(7, 2);
    report(&host.advance(&domain, &fresh, 3))?;
    report(&host.advance(&domain, &fresh, 3))?;
    report(&host.advance(&domain, &fresh, 4))?;
    fresh.fail_execution = true;
    report(&host.advance(&domain, &fresh, 5))?;
    host.reset();
    report(&host.advance(&domain, &fresh, 6))
}

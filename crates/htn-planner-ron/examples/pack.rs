#[path = "support/pack.rs"]
mod fixture;
use htn_planner::{FactSnapshot, Limits, Outcome, plan};
use std::io::Write;
fn main() -> Result<(), String> {
    let schema = fixture::schema();
    let mut limits = Limits::small();
    limits.work = 100_000;
    limits.storage = 65_536;
    limits.bindings = 256;
    let domain = htn_planner_ron::compile("pack", fixture::PACK, schema.clone(), limits)
        .map_err(|error| error.message)?;
    let facts = FactSnapshot::new(
        &schema,
        vec![
            fixture::row("eligible", 1, "front"),
            fixture::row("eligible", 2, "front"),
            fixture::row("eligible", 3, "left"),
            fixture::row("eligible", 1, "right"),
            fixture::row("assigned", 9, "bench"),
        ],
    )?;
    let result = plan(&domain, &facts, "entry", &[]);
    match result.outcome {
        Outcome::Success(entries) => {
            for entry in entries {
                writeln!(std::io::stdout(), "{entry:?}").map_err(|error| error.to_string())?;
            }
            Ok(())
        }
        other => Err(format!("pack failed: {other:?}")),
    }
}

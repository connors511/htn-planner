//! Supplied-text warehouse allocation, speculation rollback and fresh deferred decisions.
#[path = "warehouse/contracts.rs"]
mod contracts;
#[path = "warehouse/fixture.rs"]
mod fixture;
use htn_planner::{CompiledDomain, Fact, FactSnapshot, Outcome, PlanEntry, expand_deferred, plan};
use std::io::Write;

// The standalone consumer executes this supplied-text frontend before the
// unchanged RON warehouse compatibility and full result-contract journeys.
fn domain_frontend_journey() -> Result<(), String> {
    use htn_planner::{Signature, Type, Value};
    use htn_planner_ron::domain::{Source, compile};
    let schema = htn_planner::Schema {
        predicates: vec![Signature {
            name: "candidate".into(),
            arguments: vec![Type::Int],
        }],
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![Type::Int],
        }],
        ..htn_planner::Schema::default()
    };
    let source = Source {
        id: "warehouse.domain",
        text: include_str!("warehouse.domain"),
    };
    // The unused input has an explicit host type; source spelling is not a type.
    let entry = Signature {
        name: "select".into(),
        arguments: vec![Type::Int],
    };
    let domain = compile(
        source.id,
        &[source],
        schema.clone(),
        &[entry],
        fixture::limits(),
    )
    .map_err(|error| error.message)?;
    let value = Value::Int(7);
    let facts = FactSnapshot::new(
        &schema,
        vec![Fact {
            predicate: "candidate".into(),
            arguments: vec![value.clone()],
        }],
    )?;
    let before = facts.clone();
    let arguments = [Value::Int(3)];
    let result = plan(&domain, &facts, "select", &arguments);
    if result != plan(&domain, &facts, "select", &arguments) {
        return Err("domain frontend result/work ordering changed".into());
    }
    let Outcome::Success(entries) = result.outcome else {
        return Err("domain frontend could not plan".into());
    };
    if entries.len() != 1
        || !entries[0]
            .as_action()
            .is_some_and(|action| action.name == "emit" && action.arguments == [value])
        || facts != before
    {
        return Err("domain frontend failed its typed supplied-fact journey".into());
    }
    Ok(())
}

fn main() -> Result<(), String> {
    domain_frontend_journey()?;
    let schema = fixture::schema();
    let domain = htn_planner_ron::compile(
        "warehouse.ron",
        include_str!("warehouse.ron"),
        schema.clone(),
        fixture::limits(),
    )
    .map_err(|e| e.message)?;
    let parcel = fixture::reference("Parcel", 17);
    let facts = FactSnapshot::new(&schema, fixture::facts(&parcel))?;
    let before = facts.clone();
    let report = plan(&domain, &facts, "ship", std::slice::from_ref(&parcel));
    if report != plan(&domain, &facts, "ship", std::slice::from_ref(&parcel)) {
        return Err("unstable repeated ordering".into());
    }
    let Outcome::Success(entries) = report.outcome else {
        return Err("warehouse allocation failed".into());
    };
    let expected = [(2, "front"), (3, "left"), (1, "right")];
    if entries.len() != expected.len() + 1 {
        return Err("incomplete allocation".into());
    }
    for (entry, (worker, role)) in entries.iter().zip(expected) {
        let action = entry.as_action().ok_or("expected inert assignment")?;
        if action.name != "assign"
            || action.arguments
                != vec![
                    parcel.clone(),
                    fixture::reference("Worker", worker),
                    fixture::role(role),
                ]
        {
            return Err("later-failure allocation did not backtrack".into());
        }
        if action.locator.task.as_deref() != Some("role") {
            return Err("missing causal task locator".into());
        }
    }
    if facts != before {
        return Err("speculative reservations changed caller reality".into());
    }
    let PlanEntry::Deferred(token) = entries.last().ok_or("missing deferred choice")? else {
        return Err("expected deferred bay choice".into());
    };
    let fresh = FactSnapshot::new(
        &schema,
        vec![Fact {
            predicate: "ready".into(),
            arguments: vec![parcel.clone(), fixture::bay(77)],
        }],
    )?;
    let Outcome::Success(expanded) = expand_deferred(&domain, &fresh, token).outcome else {
        return Err("fresh deferred expansion failed".into());
    };
    if expanded.len() != 1
        || !expanded[0]
            .as_action()
            .is_some_and(|a| a.arguments == vec![parcel.clone(), fixture::bay(77)])
    {
        return Err("deferred choice used stale facts/captured input".into());
    }
    let mut changed = domain.limits();
    changed.work -= 1;
    let incompatible = CompiledDomain::admit(
        "warehouse.ron",
        schema,
        domain.definition().clone(),
        changed,
    )
    .map_err(|e| e.message)?;
    if expand_deferred(&incompatible, &fresh, token).outcome != Outcome::InvalidInput {
        return Err("stale deferred identity accepted".into());
    }
    let mut changed_definition = domain.definition().clone();
    changed_definition.name.push_str("-revised");
    let revised = CompiledDomain::admit(
        "warehouse.ron",
        domain.schema().clone(),
        changed_definition,
        domain.limits(),
    )
    .map_err(|e| e.message)?;
    if expand_deferred(&revised, &fresh, token).outcome != Outcome::InvalidInput {
        return Err("changed definition accepted old deferred token".into());
    }
    contracts::run(&domain, &facts)?;
    writeln!(std::io::stdout(),"warehouse: typed allocation, rollback, fresh deferred decisions and result contracts passed").map_err(|e|e.to_string())
}

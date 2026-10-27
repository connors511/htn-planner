use super::state::{Arguments, Branch, Choices, Item, Meter};
use crate::{CompiledDomain, FactSnapshot, Outcome, TraceId};

pub(super) fn check(
    (domain, facts, queries): (
        &CompiledDomain,
        &FactSnapshot,
        &mut super::queries::Queries<'_>,
    ),
    query: (usize, Option<TraceId>),
    item: &Item,
    active: &Branch,
    choices: &Choices,
    output: &Arguments,
    meter: &mut Meter,
) -> Result<bool, Outcome> {
    let (predicate, trace_step) = query;
    meter.report.stages.record(crate::counters::Stage::Query);
    let trace_query = meter.trace.emit(
        trace_step,
        crate::TraceEvent::FactQueryEntered,
        crate::trace::Site {
            source: &domain.source,
            task: Some(&domain.definition.tasks[item.task].name),
            declaration: &domain.schema.predicates[predicate].name,
            step: item.step.and_then(|step| step.checked_sub(1)),
        },
        Some(crate::trace::Snapshot::Values(
            &output.values,
            &output.costs,
        )),
        &meter.report,
    );
    let batch_id = queries.ensure_values(
        domain,
        predicate,
        output,
        (
            active
                .cost
                .saturating_add(choices.cost)
                .saturating_add(item.cost())
                .saturating_add(output.size),
            active
                .bindings
                .saturating_add(choices.bindings)
                .saturating_add(item.slots()),
            trace_query,
        ),
        meter,
    )?;
    let batch = queries.get(batch_id);
    let mut found = false;
    let mut predicate_seen = false;
    for index in 0..active.overlay.count(facts, batch) {
        let candidate = active.overlay.row(facts, batch, index, meter)?;
        let mut matches = false;
        if let Some((row, values, costs, cost)) = candidate
            && row == predicate
        {
            predicate_seen = true;
            meter.charge(cost.saturating_add(output.size))?;
            matches = values == output.values;
            if !matches
                && batch.is_some_and(|batch| {
                    index >= facts.facts.len() && index - facts.facts.len() < batch.rows.len()
                })
            {
                meter.trace.inherit(
                    trace_query,
                    crate::TraceEvent::Rejected(crate::TraceRejection::UnificationConflict),
                    Some(crate::trace::Snapshot::Values(values, costs)),
                    &meter.report,
                );
            }
        }
        meter.report.stages.record(crate::counters::Stage::Row);
        if matches {
            found = true;
            break;
        }
    }
    if !found {
        super::explanation::reject_values(
            meter,
            item,
            super::explanation::Predicate::Fact(predicate),
            &output.values,
            if predicate_seen {
                crate::TraceRejection::UnificationConflict
            } else {
                crate::TraceRejection::NoMatchingTuple
            },
            batch_id,
        );
        meter.trace.inherit(
            trace_query,
            crate::TraceEvent::Rejected(crate::TraceRejection::NoMatchingTuple),
            Some(crate::trace::Snapshot::Values(
                &output.values,
                &output.costs,
            )),
            &meter.report,
        );
        meter.reason = Some("required tuple absent");
        return Ok(false);
    }
    meter.trace.inherit(
        trace_query,
        crate::TraceEvent::Witness,
        Some(crate::trace::Snapshot::Values(
            &output.values,
            &output.costs,
        )),
        &meter.report,
    );
    meter.trace.inherit(
        trace_step,
        crate::TraceEvent::StepCompleted,
        Some(crate::trace::Snapshot::Values(
            &output.values,
            &output.costs,
        )),
        &meter.report,
    );
    Ok(true)
}

use super::expressions;
use super::state::{Bindings, Branch, Choices, Item, Meter};
use crate::{CompiledDomain, FactSnapshot, Outcome};

pub(super) fn apply(
    (domain, facts): (&CompiledDomain, &FactSnapshot),
    queries: &mut super::queries::Queries<'_>,
    (effect, arguments, trace_step): (
        usize,
        &[crate::compiler::CompiledExpression],
        Option<crate::TraceId>,
    ),
    item: &Item,
    active: &mut Branch,
    choices: &Choices,
    meter: &mut Meter,
) -> Result<(), Outcome> {
    let trace_effect = meter.trace.emit(
        trace_step,
        crate::TraceEvent::EffectEntered,
        crate::trace::Site {
            source: &domain.source,
            task: Some(&domain.definition.tasks[item.task].name),
            declaration: &domain.schema.effects[effect].name,
            step: item.step,
        },
        Some(crate::trace::Snapshot::Expressions(
            arguments,
            &item.args.values,
            &item.args.costs,
        )),
        &meter.report,
    );
    let size = expressions::size(arguments, &item.args, meter)?;
    choices.reserve(
        active,
        item.cost() + size,
        item.slots() + arguments.len(),
        meter,
    )?;
    let values = expressions::evaluate(arguments, &item.args, size, meter)?;
    meter.charge(arguments.len())?;
    let env = Bindings {
        values: values.values.into_iter().map(Some).collect(),
        costs: values.costs,
        size,
    };
    for (ordinal, operation) in domain.effects[effect].operations.iter().enumerate() {
        meter.charge(1)?;
        let cost = expressions::size(&operation.arguments, &env, meter)?;
        choices.reserve(
            active,
            item.cost() + size + cost + 1,
            item.slots() + env.values.len() + operation.arguments.len(),
            meter,
        )?;
        let args = expressions::evaluate(&operation.arguments, &env, cost, meter)?;
        let batch_id = queries.ensure_values(
            domain,
            operation.predicate,
            &args,
            (
                active
                    .cost
                    .saturating_add(choices.cost)
                    .saturating_add(item.cost())
                    .saturating_add(size)
                    .saturating_add(cost),
                active
                    .bindings
                    .saturating_add(choices.bindings)
                    .saturating_add(item.slots())
                    .saturating_add(env.values.len()),
                trace_effect,
            ),
            meter,
        )?;
        let batch = queries.get(batch_id);
        let mut found = false;
        for index in 0..active.overlay.count(facts, batch) {
            let matches =
                active
                    .overlay
                    .matches(facts, batch, index, operation.predicate, &args, meter)?;
            meter.report.stages.record(crate::counters::Stage::Row);
            if matches {
                found = true;
                if operation.assert {
                    break;
                }
                let extra = super::overlay::Overlay::removal_cost(facts, batch, index);
                choices.reserve(
                    active,
                    item.cost() + size + cost + extra,
                    item.slots() + env.values.len() + operation.arguments.len(),
                    meter,
                )?;
                meter.charge(extra + 1)?;
                active.overlay.remove(facts, batch, index);
                active.cost += extra;
            }
        }
        if operation.assert && !found {
            choices.reserve(
                active,
                item.cost() + size + 1 + cost,
                item.slots() + env.values.len() + operation.arguments.len(),
                meter,
            )?;
            meter.charge(1)?;
            active.overlay.insert(operation.predicate, args);
            active.cost += 1 + cost;
        } else {
            meter.charge(cost)?;
        }
        meter.report.stages.record(crate::counters::Stage::Effect);
        meter.trace.inherit(
            trace_effect,
            crate::TraceEvent::EffectOperationCompleted {
                ordinal,
                operation: if operation.assert {
                    crate::TraceEffectOperation::Assert
                } else {
                    crate::TraceEffectOperation::Retract
                },
            },
            Some(crate::trace::Snapshot::Expressions(
                &operation.arguments,
                &env.values,
                &env.costs,
            )),
            &meter.report,
        );
    }
    meter.charge(env.size)?;
    Ok(())
}

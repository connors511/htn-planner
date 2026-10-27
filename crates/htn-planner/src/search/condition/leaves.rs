use super::{Op, control, push, reserve};
use crate::compiler::{CompiledCondition, CompiledExpression};
use crate::search::expressions;
use crate::search::state::{Bindings, Branch, Choices, Item, Meter};
use crate::{Comparison, CompiledDomain, FactSnapshot, NumericOperation, Outcome, Value};

pub(super) fn fact(
    (domain, facts, queries): (
        &CompiledDomain,
        &FactSnapshot,
        &mut crate::search::queries::Queries<'_>,
    ),
    (predicate, arguments): (usize, &[CompiledExpression]),
    (node, row): (usize, usize),
    mut item: Item,
    active: &mut Branch,
    choices: &mut Choices,
    meter: &mut Meter,
) -> Result<bool, Outcome> {
    let batch_id = queries.ensure(
        domain,
        predicate,
        (arguments, item.environment()),
        (
            active
                .cost
                .saturating_add(choices.cost)
                .saturating_add(item.cost()),
            active
                .bindings
                .saturating_add(choices.bindings)
                .saturating_add(item.slots()),
            item.control.as_ref().and_then(|control| control.parent),
        ),
        meter,
    )?;
    let batch = queries.get(batch_id);
    let count = active.overlay.count(facts, batch);
    let mut predicate_seen = false;
    for index in row..count {
        let Some((candidate, values, costs, _)) = active.overlay.row(facts, batch, index, meter)?
        else {
            meter.report.stages.record(crate::counters::Stage::Row);
            continue;
        };
        predicate_seen |= candidate == predicate;
        let compatible = candidate == predicate
            && expressions::compatible(arguments, values, costs, item.environment(), meter)?;
        meter.report.stages.record(crate::counters::Stage::Row);
        if !compatible {
            if candidate == predicate
                && batch.is_some_and(|batch| {
                    index >= facts.facts.len() && index - facts.facts.len() < batch.rows.len()
                })
            {
                meter.trace.inherit(
                    item.control.as_ref().and_then(|control| control.parent),
                    crate::TraceEvent::Rejected(crate::TraceRejection::UnificationConflict),
                    Some(crate::trace::Snapshot::Values(values, costs)),
                    &meter.report,
                );
            }
            continue;
        }
        if index + 1 < count {
            reserve(&item, active, choices, 2, 0, meter)?;
            push(
                &mut item,
                Op::Eval {
                    node,
                    row: index + 1,
                },
                meter,
            )?;
            active.push(item);
            choices.reserve(
                active,
                active.cost.saturating_add(1),
                active.bindings,
                meter,
            )?;
            choices.retain(active, meter)?;
            item = active.pop().ok_or(Outcome::InvalidInput)?;
            control(&mut item)?.pop();
        }
        let Some((_, values, costs, _)) = active.overlay.row(facts, batch, index, meter)? else {
            return Err(Outcome::InvalidInput);
        };
        let extra = expressions::additional(arguments, costs, item.environment(), meter)?;
        reserve(&item, active, choices, extra + 1, 0, meter)?;
        expressions::bind(arguments, values, costs, item.environment_mut()?, meter)?;
        item.growth(extra);
        let parent = item.control.as_ref().and_then(|control| control.parent);
        let env = item.environment();
        meter.trace.inherit(
            parent,
            crate::TraceEvent::Witness,
            Some(crate::trace::Snapshot::Expressions(
                arguments,
                &env.values,
                &env.costs,
            )),
            &meter.report,
        );
        active.push(item);
        return Ok(true);
    }
    if row == 0 {
        crate::search::explanation::reject_expressions(
            meter,
            &item,
            crate::search::explanation::Predicate::Fact(predicate),
            arguments,
            if predicate_seen {
                crate::TraceRejection::UnificationConflict
            } else {
                crate::TraceRejection::NoMatchingTuple
            },
            batch_id,
        );
    } else if let Some(explanation) = &mut meter.explanation {
        explanation.exhausted_queries = explanation.exhausted_queries.saturating_add(1);
    }
    crate::search::tracing::event(
        &item,
        crate::TraceEvent::Rejected(crate::TraceRejection::NoMatchingTuple),
        meter,
    );
    meter.reason = Some("condition has no compatible tuple");
    meter.charge(item.cost())?;
    Ok(false)
}
pub(super) fn enter(
    domain: &CompiledDomain,
    (query, arguments, node): (usize, &[CompiledExpression], usize),
    item: &mut Item,
    active: &Branch,
    choices: &Choices,
    meter: &mut Meter,
) -> Result<(), Outcome> {
    let query = &domain.queries[query];
    if let Some(control) = &item.control
        && item
            .depth
            .saturating_add(control.frames.len())
            .saturating_add(control.scopes.len())
            .saturating_add(3)
            > meter.limits.depth
    {
        return Err(Outcome::CapacityExceeded);
    }
    let size = expressions::size(arguments, item.environment(), meter)?;
    reserve(
        item,
        active,
        choices,
        size + 2 * query.locals + 4,
        query.parameters + query.locals,
        meter,
    )?;
    let values = expressions::evaluate(arguments, item.environment(), size, meter)?;
    meter.charge(2 * query.locals + query.parameters)?;
    let mut env = Bindings {
        values: values.values.into_iter().map(Some).collect(),
        costs: values.costs,
        size,
    };
    env.locals(query.locals);
    let control = control(item)?;
    control.size += 1 + env.size;
    control.bindings += env.values.len();
    control.scopes.push(env);
    push(item, Op::Return(node), meter)?;
    push(
        item,
        Op::Eval {
            node: query.root,
            row: 0,
        },
        meter,
    )?;
    Ok(())
}
pub(super) fn returned(
    domain: &CompiledDomain,
    node: usize,
    item: &mut Item,
    active: &Branch,
    choices: &Choices,
    meter: &mut Meter,
) -> Result<bool, Outcome> {
    let CompiledCondition::Derived { query, outputs, .. } = &domain.conditions[node] else {
        return Err(Outcome::InvalidInput);
    };
    let query_id = *query;
    let query = &domain.queries[query_id];
    let size = expressions::size(&query.result, item.environment(), meter)?;
    reserve(item, active, choices, size + 1, query.result.len(), meter)?;
    let returned = expressions::evaluate(&query.result, item.environment(), size, meter)?;
    meter.trace.inherit(
        crate::search::tracing::parent(item),
        crate::TraceEvent::Witness,
        Some(crate::trace::Snapshot::Values(
            &returned.values,
            &returned.costs,
        )),
        &meter.report,
    );
    let control = control(item)?;
    let env = control.scopes.pop().ok_or(Outcome::InvalidInput)?;
    control.size -= 1 + env.size;
    control.bindings -= env.values.len();
    // Disposal is a traversal too; charge it before dropping the bounded private scope.
    meter.charge(env.size)?;
    drop(env);
    if !expressions::compatible(
        outputs,
        &returned.values,
        &returned.costs,
        item.environment(),
        meter,
    )? {
        crate::search::explanation::reject_values(
            meter,
            item,
            crate::search::explanation::Predicate::Derived(query_id),
            &returned.values,
            crate::TraceRejection::UnificationConflict,
            None,
        );
        return Ok(false);
    }
    let extra = expressions::additional(outputs, &returned.costs, item.environment(), meter)?;
    reserve(
        item,
        active,
        choices,
        size + extra + 1,
        query.result.len(),
        meter,
    )?;
    expressions::bind(
        outputs,
        &returned.values,
        &returned.costs,
        item.environment_mut()?,
        meter,
    )?;
    item.growth(extra);
    Ok(true)
}
fn operands(
    left: &CompiledExpression,
    right: &CompiledExpression,
    item: &Item,
    meter: &mut Meter,
) -> Result<(i64, i64, Option<u8>), Outcome> {
    let (left, lc) = expressions::value(left, item.environment())?;
    let (right, rc) = expressions::value(right, item.environment())?;
    meter.charge(lc.saturating_add(rc).saturating_add(1))?;
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok((*a, *b, None)),
        (
            Value::Fixed { raw: a, scale },
            Value::Fixed {
                raw: b,
                scale: other,
            },
        ) if scale == other => Ok((*a, *b, Some(*scale))),
        _ => Err(Outcome::InvalidInput),
    }
}
pub(super) fn compare(
    op: Comparison,
    left: &CompiledExpression,
    right: &CompiledExpression,
    item: &Item,
    meter: &mut Meter,
) -> Result<bool, Outcome> {
    let (left, right, _) = operands(left, right, item, meter)?;
    Ok(match op {
        Comparison::Equal => left == right,
        Comparison::NotEqual => left != right,
        Comparison::Less => left < right,
        Comparison::LessEqual => left <= right,
        Comparison::Greater => left > right,
        Comparison::GreaterEqual => left >= right,
    })
}
pub(super) fn calculate(
    (op, left, right, output): (
        NumericOperation,
        &CompiledExpression,
        &CompiledExpression,
        &CompiledExpression,
    ),
    item: &mut Item,
    active: &Branch,
    choices: &Choices,
    meter: &mut Meter,
) -> Result<bool, Outcome> {
    let (left_expression, right_expression) = (left, right);
    let (left, right, scale) = operands(left, right, item, meter)?;
    let Some(raw) = (match op {
        NumericOperation::Add => left.checked_add(right),
        NumericOperation::Subtract => left.checked_sub(right),
    }) else {
        crate::search::explanation::reject_numeric(
            meter,
            item,
            "arithmetic_overflow",
            left_expression,
            right_expression,
            crate::TraceRejection::ArithmeticOverflow,
        );
        crate::search::tracing::numeric(
            item,
            crate::TraceEvent::Rejected(crate::TraceRejection::ArithmeticOverflow),
            left_expression,
            right_expression,
            Some(output),
            meter,
        );
        meter.reason = Some("checked numeric arithmetic overflow");
        return Err(Outcome::InvalidInput);
    };
    reserve(item, active, choices, 2, 0, meter)?;
    let value = scale.map_or(Value::Int(raw), |scale| Value::Fixed { raw, scale });
    let output = std::slice::from_ref(output);
    if !expressions::compatible(
        output,
        std::slice::from_ref(&value),
        &[1],
        item.environment(),
        meter,
    )? {
        return Ok(false);
    }
    let extra = expressions::additional(output, &[1], item.environment(), meter)?;
    reserve(item, active, choices, extra + 2, 0, meter)?;
    expressions::bind(output, &[value], &[1], item.environment_mut()?, meter)?;
    item.growth(extra);
    Ok(true)
}

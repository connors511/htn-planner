use super::{Op, control, push, reserve};
use crate::compiler::CompiledCondition;
use crate::search::state::{Branch, Choices, Item, Meter};
use crate::{CompiledDomain, Outcome};

pub(super) fn alternative(
    domain: &CompiledDomain,
    (node, next, boundary): (usize, usize, Option<u64>),
    mut item: Item,
    active: &mut Branch,
    choices: &mut Choices,
    meter: &mut Meter,
) -> Result<bool, Outcome> {
    let (CompiledCondition::Or(children) | CompiledCondition::FirstSuccess(children)) =
        &domain.conditions[node]
    else {
        return Err(Outcome::InvalidInput);
    };
    let Some(child) = children.get(next) else {
        meter.charge(item.cost())?;
        return Ok(false);
    };
    // Sibling fallback lives only in Choices, never in a selected-child snapshot.
    // FinishFirst may delete it while preserving every witness inside that child.
    if next + 1 < children.len() {
        reserve(&item, active, choices, 2, 0, meter)?;
        push(
            &mut item,
            Op::Alternative {
                node,
                next: next + 1,
                boundary,
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
        choices.retain_tagged(active, boundary, meter)?;
        item = active.pop().ok_or(Outcome::InvalidInput)?;
        control(&mut item)?.pop();
    }
    reserve(&item, active, choices, 3, 0, meter)?;
    let trace_parent = crate::search::tracing::event(
        &item,
        crate::TraceEvent::AlternativeEntered { ordinal: next },
        meter,
    );
    control(&mut item)?.parent = trace_parent;
    if let Some(id) = boundary {
        push(&mut item, Op::FinishFirst(id), meter)?;
    }
    push(
        &mut item,
        Op::Eval {
            node: *child,
            row: 0,
        },
        meter,
    )?;
    active.push(item);
    Ok(true)
}
pub(super) fn negate(
    child: usize,
    mut item: Item,
    active: &mut Branch,
    choices: &mut Choices,
    meter: &mut Meter,
) -> Result<bool, Outcome> {
    // Exhausting the probe restores this absence baseline exactly once. A witness
    // discards only choices created since entry, then fails to an outer choice.
    let base = choices.count();
    let absence_parent = crate::search::tracing::parent(&item);
    active.push(item);
    choices.reserve(
        active,
        active.cost.saturating_add(1),
        active.bindings,
        meter,
    )?;
    choices.retain(active, meter)?;
    choices.mark_absence(absence_parent);
    item = active.pop().ok_or(Outcome::InvalidInput)?;
    reserve(&item, active, choices, 3, 0, meter)?;
    push(&mut item, Op::NotEnd(base), meter)?;
    push(
        &mut item,
        Op::Eval {
            node: child,
            row: 0,
        },
        meter,
    )?;
    active.push(item);
    Ok(true)
}

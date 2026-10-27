use super::state::{Item, Meter};
use crate::compiler::CompiledCondition;
use crate::trace::{Site, Snapshot};
use crate::{CompiledDomain, TraceControl, TraceEvent, TraceId};
pub(super) fn parent(item: &Item) -> Option<TraceId> {
    item.control
        .as_ref()
        .and_then(|control| control.parent)
        .or(item.trace_method)
        .or(item.trace_task)
}
pub(super) fn condition(
    domain: &CompiledDomain,
    item: &mut Item,
    node: usize,
    row: usize,
    meter: &mut Meter,
) {
    if row != 0 {
        return;
    }
    let parent = parent(item);
    let env = item.environment();
    let condition = &domain.conditions[node];
    let id = match condition {
        CompiledCondition::Fact {
            predicate,
            arguments,
        } => meter.trace.emit(
            parent,
            TraceEvent::FactQueryEntered,
            Site {
                source: &domain.source,
                task: Some(&domain.definition.tasks[item.task].name),
                declaration: &domain.schema.predicates[*predicate].name,
                step: item.step.and_then(|step| step.checked_sub(1)),
            },
            Some(Snapshot::Expressions(arguments, &env.values, &env.costs)),
            &meter.report,
        ),
        CompiledCondition::Derived {
            query, arguments, ..
        } => meter.trace.emit(
            parent,
            TraceEvent::DerivedQueryEntered,
            Site {
                source: &domain.source,
                task: Some(&domain.definition.tasks[item.task].name),
                declaration: &domain.definition.queries[*query].name,
                step: item.step.and_then(|step| step.checked_sub(1)),
            },
            Some(Snapshot::Expressions(arguments, &env.values, &env.costs)),
            &meter.report,
        ),
        _ => {
            let kind = match condition {
                CompiledCondition::And(_) => TraceControl::And,
                CompiledCondition::Or(_) => TraceControl::Or,
                CompiledCondition::FirstSuccess(_) => TraceControl::FirstSuccess,
                CompiledCondition::Not(_) => TraceControl::Not,
                CompiledCondition::Compare { .. } => TraceControl::Compare,
                CompiledCondition::Calculate { .. } => TraceControl::Calculate,
                CompiledCondition::Fact { .. } | CompiledCondition::Derived { .. } => return,
            };
            let arguments = match condition {
                CompiledCondition::Compare { left, right, .. } => Snapshot::Numeric {
                    left,
                    right,
                    output: None,
                    values: &env.values,
                    costs: &env.costs,
                },
                CompiledCondition::Calculate {
                    left,
                    right,
                    output,
                    ..
                } => Snapshot::Numeric {
                    left,
                    right,
                    output: Some(output),
                    values: &env.values,
                    costs: &env.costs,
                },
                _ => Snapshot::Slots(&env.values, &env.costs),
            };
            meter.trace.inherit(
                parent,
                TraceEvent::ControlEntered(kind),
                Some(arguments),
                &meter.report,
            )
        }
    };
    if let Some(control) = &mut item.control {
        control.parent = id;
    }
}
pub(super) fn event(item: &Item, event: TraceEvent, meter: &mut Meter) -> Option<TraceId> {
    let env = item.environment();
    meter.trace.inherit(
        parent(item),
        event,
        Some(Snapshot::Slots(&env.values, &env.costs)),
        &meter.report,
    )
}

pub(super) fn step(
    domain: &CompiledDomain,
    item: &Item,
    instruction: &crate::compiler::Instruction,
    meter: &mut Meter,
) -> Option<TraceId> {
    let step = item.step?;
    let kind = match &domain.definition.tasks[item.task].methods[item.method].steps[step] {
        crate::Step::Condition(_) => crate::TraceStep::Condition,
        crate::Step::Query { .. } => crate::TraceStep::Query,
        crate::Step::Require { .. } => crate::TraceStep::Require,
        crate::Step::Action { .. } => crate::TraceStep::Action,
        crate::Step::Task { .. } => crate::TraceStep::Task,
        crate::Step::Call { .. } => crate::TraceStep::Call,
        crate::Step::Defer { .. } => crate::TraceStep::Defer,
        crate::Step::Effect { .. } => crate::TraceStep::Effect,
    };
    let arguments = match instruction {
        crate::compiler::Instruction::Condition { .. } => {
            Snapshot::Slots(&item.args.values, &item.args.costs)
        }
        crate::compiler::Instruction::Effect { arguments, .. }
        | crate::compiler::Instruction::Require { arguments, .. }
        | crate::compiler::Instruction::Action { arguments, .. }
        | crate::compiler::Instruction::Task { arguments, .. }
        | crate::compiler::Instruction::Defer { arguments, .. } => {
            Snapshot::Expressions(arguments, &item.args.values, &item.args.costs)
        }
    };
    meter.trace.emit(
        item.trace_method,
        TraceEvent::StepEntered(kind),
        super::trace_site(domain, item.task, item.method, Some(step)),
        Some(arguments),
        &meter.report,
    )
}

pub(super) fn numeric(
    item: &Item,
    event: TraceEvent,
    left: &crate::compiler::CompiledExpression,
    right: &crate::compiler::CompiledExpression,
    output: Option<&crate::compiler::CompiledExpression>,
    meter: &mut Meter,
) {
    let env = item.environment();
    meter.trace.inherit(
        parent(item),
        event,
        Some(Snapshot::Numeric {
            left,
            right,
            output,
            values: &env.values,
            costs: &env.costs,
        }),
        &meter.report,
    );
}

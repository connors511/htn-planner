mod branching;
mod leaves;
use super::state::{Bindings, Branch, Choices, Item, Meter};
use crate::compiler::CompiledCondition;
use crate::{CompiledDomain, FactSnapshot, Outcome};

#[derive(Clone)]
pub(super) struct Control {
    pub frames: Vec<Frame>,
    pub parent: Option<crate::TraceId>,
    pub scopes: Vec<Bindings>,
    pub size: usize,
    pub bindings: usize,
}
#[derive(Clone, Copy)]
pub(super) struct Frame {
    operation: Op,
    pub parent: Option<crate::TraceId>,
}
#[derive(Clone, Copy)]
pub(super) enum Op {
    Eval {
        node: usize,
        row: usize,
    },
    Sequence {
        node: usize,
        next: usize,
    },
    Alternative {
        node: usize,
        next: usize,
        boundary: Option<u64>,
    },
    FinishFirst(u64),
    NotEnd(usize),
    Return(usize),
}
impl Control {
    pub(super) fn negated_probe(&self) -> bool {
        self.frames
            .iter()
            .any(|frame| matches!(frame.operation, Op::NotEnd(_)))
    }
    pub fn new(node: usize, parent: Option<crate::TraceId>) -> Self {
        Self {
            frames: vec![Frame {
                operation: Op::Eval { node, row: 0 },
                parent,
            }],
            parent,
            scopes: vec![],
            size: 2,
            bindings: 0,
        }
    }
    pub fn push(&mut self, operation: Op) {
        self.frames.push(Frame {
            operation,
            parent: self.parent,
        });
        self.size += 1;
    }
    fn pop(&mut self) -> Option<Op> {
        let frame = self.frames.pop()?;
        self.size -= 1;
        self.parent = frame.parent;
        Some(frame.operation)
    }
}
fn control(item: &mut Item) -> Result<&mut Control, Outcome> {
    item.control.as_mut().ok_or(Outcome::InvalidInput)
}
fn push(item: &mut Item, frame: Op, meter: &Meter) -> Result<(), Outcome> {
    let depth = item.depth;
    let control = control(item)?;
    if depth
        .saturating_add(control.frames.len())
        .saturating_add(control.scopes.len())
        .saturating_add(1)
        > meter.limits.depth
    {
        return Err(Outcome::CapacityExceeded);
    }
    control.push(frame);
    Ok(())
}
fn reserve(
    item: &Item,
    active: &Branch,
    choices: &Choices,
    extra: usize,
    bindings: usize,
    meter: &mut Meter,
) -> Result<(), Outcome> {
    if let Some(control) = &item.control
        && item
            .depth
            .saturating_add(control.frames.len())
            .saturating_add(control.scopes.len())
            > meter.limits.depth
    {
        return Err(Outcome::CapacityExceeded);
    }
    choices.reserve(
        active,
        item.cost().saturating_add(extra),
        item.slots().saturating_add(bindings),
        meter,
    )
}
pub(super) fn advance(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    queries: &mut super::queries::Queries<'_>,
    mut item: Item,
    active: &mut Branch,
    choices: &mut Choices,
    meter: &mut Meter,
) -> Result<bool, Outcome> {
    meter.charge(3)?;
    let Some(frame) = control(&mut item)?.pop() else {
        if !control(&mut item)?.scopes.is_empty() {
            return Err(Outcome::InvalidInput);
        }
        item.control = None;
        active.push(item);
        return Ok(true);
    };
    // The popped frame remains live until this transition returns.
    reserve(&item, active, choices, 3, 0, meter)?;
    match frame {
        Op::Eval { node, row } => {
            if row == 0 {
                meter
                    .report
                    .stages
                    .record(crate::counters::Stage::Condition);
                if matches!(
                    &domain.conditions[node],
                    CompiledCondition::Fact { .. } | CompiledCondition::Derived { .. }
                ) {
                    meter.report.stages.record(crate::counters::Stage::Query);
                }
            }
            super::tracing::condition(domain, &mut item, node, row, meter);
            match domain.conditions.get(node).ok_or(Outcome::InvalidInput)? {
                CompiledCondition::Fact {
                    predicate,
                    arguments,
                } => {
                    return leaves::fact(
                        (domain, facts, queries),
                        (*predicate, arguments),
                        (node, row),
                        item,
                        active,
                        choices,
                        meter,
                    );
                }
                CompiledCondition::Derived {
                    query, arguments, ..
                } => {
                    leaves::enter(
                        domain,
                        (*query, arguments, node),
                        &mut item,
                        active,
                        choices,
                        meter,
                    )?;
                }
                CompiledCondition::And(_) => {
                    push(&mut item, Op::Sequence { node, next: 0 }, meter)?;
                }
                CompiledCondition::Or(_) => push(
                    &mut item,
                    Op::Alternative {
                        node,
                        next: 0,
                        boundary: None,
                    },
                    meter,
                )?,
                CompiledCondition::FirstSuccess(_) => {
                    meter.charge(1)?;
                    let id = meter.next_boundary;
                    meter.next_boundary = id.checked_add(1).ok_or(Outcome::CapacityExceeded)?;
                    push(
                        &mut item,
                        Op::Alternative {
                            node,
                            next: 0,
                            boundary: Some(id),
                        },
                        meter,
                    )?;
                }
                CompiledCondition::Not(child) => {
                    return branching::negate(*child, item, active, choices, meter);
                }
                CompiledCondition::Compare { op, left, right } => {
                    if !leaves::compare(*op, left, right, &item, meter)? {
                        let name = match op {
                            crate::Comparison::Equal => "equal",
                            crate::Comparison::NotEqual => "not_equal",
                            crate::Comparison::Less => "less",
                            crate::Comparison::LessEqual => "less_equal",
                            crate::Comparison::Greater => "greater",
                            crate::Comparison::GreaterEqual => "greater_equal",
                        };
                        super::explanation::reject_numeric(
                            meter,
                            &item,
                            name,
                            left,
                            right,
                            crate::TraceRejection::ConditionFalse,
                        );
                        super::tracing::numeric(
                            &item,
                            crate::TraceEvent::Rejected(crate::TraceRejection::ConditionFalse),
                            left,
                            right,
                            None,
                            meter,
                        );
                        meter.charge(item.cost())?;
                        return Ok(false);
                    }
                    super::tracing::numeric(
                        &item,
                        crate::TraceEvent::Witness,
                        left,
                        right,
                        None,
                        meter,
                    );
                }
                CompiledCondition::Calculate {
                    op,
                    left,
                    right,
                    output,
                } => {
                    if !leaves::calculate(
                        (*op, left, right, output),
                        &mut item,
                        active,
                        choices,
                        meter,
                    )? {
                        super::explanation::reject_numeric(
                            meter,
                            &item,
                            "calculation_output",
                            left,
                            right,
                            crate::TraceRejection::UnificationConflict,
                        );
                        super::tracing::numeric(
                            &item,
                            crate::TraceEvent::Rejected(crate::TraceRejection::UnificationConflict),
                            left,
                            right,
                            Some(output),
                            meter,
                        );
                        meter.charge(item.cost())?;
                        return Ok(false);
                    }
                    super::tracing::numeric(
                        &item,
                        crate::TraceEvent::Witness,
                        left,
                        right,
                        Some(output),
                        meter,
                    );
                }
            }
        }
        Op::Sequence { node, next } => {
            let CompiledCondition::And(children) = &domain.conditions[node] else {
                return Err(Outcome::InvalidInput);
            };
            if let Some(child) = children.get(next) {
                push(
                    &mut item,
                    Op::Sequence {
                        node,
                        next: next + 1,
                    },
                    meter,
                )?;
                push(
                    &mut item,
                    Op::Eval {
                        node: *child,
                        row: 0,
                    },
                    meter,
                )?;
            } else {
                super::tracing::event(&item, crate::TraceEvent::Witness, meter);
            }
        }
        Op::Alternative {
            node,
            next,
            boundary,
        } => {
            return branching::alternative(
                domain,
                (node, next, boundary),
                item,
                active,
                choices,
                meter,
            );
        }
        Op::FinishFirst(id) => {
            choices.commit(id, meter)?;
            if let Some(explanation) = &mut meter.explanation {
                explanation.first_success_cuts = explanation.first_success_cuts.saturating_add(1);
            }
            super::tracing::event(&item, crate::TraceEvent::FirstSuccessSelected, meter);
        }
        Op::NotEnd(base) => {
            super::explanation::reject_values(
                meter,
                &item,
                super::explanation::Predicate::Control("not"),
                &[],
                crate::TraceRejection::ProbeFoundWitness,
                None,
            );
            super::tracing::event(
                &item,
                crate::TraceEvent::Rejected(crate::TraceRejection::ProbeFoundWitness),
                meter,
            );
            choices.truncate(base, meter)?;
            meter.charge(item.cost())?;
            return Ok(false);
        }
        Op::Return(node) => {
            if !leaves::returned(domain, node, &mut item, active, choices, meter)? {
                meter.charge(item.cost())?;
                return Ok(false);
            }
        }
    }
    reserve(&item, active, choices, 0, 0, meter)?;
    active.push(item);
    Ok(true)
}

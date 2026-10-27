mod explanation;
mod expressions;
pub use explanation::{
    AbsenceExplanation, ExplainedPlanningResult, PredicateFailure, SearchExplanation,
    SearchResolution,
};
mod queries;
pub use queries::{QueryBudget, QueryError, QueryProvider};
mod invocation;
pub use invocation::{
    expand_deferred, expand_deferred_explained_with_queries,
    expand_deferred_explained_with_queries_focused, expand_deferred_traced,
    expand_deferred_traced_with_queries, expand_deferred_traced_with_queries_focused,
    expand_deferred_with_queries, plan, plan_traced, plan_traced_with_queries,
    plan_traced_with_queries_focused, plan_with_queries,
};
mod condition;
mod effects;
mod overlay;
mod require;
mod state;
mod tracing;
use crate::compiler::Instruction;
use crate::{
    Action, COMPATIBILITY_ID, CompiledDomain, DeferredTask, FactSnapshot, Locator, Outcome,
    PlanEntry,
};
use state::{Bindings, Branch, Choices, Item, Meter};

fn locator(domain: &CompiledDomain, task: usize, method: usize, step: Option<usize>) -> Locator {
    let declaration = domain.tasks[task]
        .methods
        .get(method)
        .map_or(&domain.definition.tasks[task].name, |method| &method.name)
        .clone();
    Locator {
        task: Some(domain.definition.tasks[task].name.clone()),
        source: domain.source.clone(),
        declaration,
        step,
    }
}
fn trace_site(
    domain: &CompiledDomain,
    task: usize,
    method: usize,
    step: Option<usize>,
) -> crate::trace::Site<'_> {
    crate::trace::Site {
        source: &domain.source,
        task: Some(&domain.definition.tasks[task].name),
        declaration: domain.tasks[task]
            .methods
            .get(method)
            .map_or(domain.definition.tasks[task].name.as_str(), |method| {
                method.name.as_str()
            }),
        step,
    }
}
fn search(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    mut active: Branch,
    queries: &mut queries::Queries<'_>,
    meter: &mut Meter,
) -> Result<Outcome, Outcome> {
    let mut choices = Choices::default();
    loop {
        meter.charge(1)?;
        let Some(mut item) = active.pop() else {
            meter.charge(active.overlay.size.saturating_add(choices.cost))?;
            if let Some(explanation) = &mut meter.explanation {
                explanation.selected = active.selected_empty;
            }
            return Ok(Outcome::Success(active.actions));
        };
        let source_step = if item.control.is_some() {
            item.step.and_then(|step| step.checked_sub(1))
        } else {
            item.step
        };
        meter.site = Some((item.task, item.method, source_step));
        if item.depth > domain.limits.depth {
            return Err(Outcome::CapacityExceeded);
        }
        if item.control.is_some() {
            if !condition::advance(
                domain,
                facts,
                queries,
                item,
                &mut active,
                &mut choices,
                meter,
            )? && !choices.restore(&mut active, meter)?
            {
                return Ok(Outcome::NoPlan);
            }
            continue;
        }
        if !item.invocation_entered {
            if let Some(explanation) = &mut meter.explanation {
                item.explanation_invocation = explanation.invocation();
            }
            meter.report.stages.record(crate::counters::Stage::Task);
            let location = trace_site(domain, item.task, usize::MAX, None);
            item.trace_task = meter.trace.emit(
                item.trace_caller,
                crate::TraceEvent::TaskEntered,
                location,
                Some(crate::trace::Snapshot::Slots(
                    &item.args.values,
                    &item.args.costs,
                )),
                &meter.report,
            );
            item.invocation_entered = true;
        }
        let methods = &domain.tasks[item.task].methods;
        let Some(step) = item.step else {
            if item.method >= methods.len() {
                explanation::reject_values(
                    meter,
                    &item,
                    explanation::Predicate::Control("task_methods"),
                    &[],
                    crate::TraceRejection::NoSuccessfulMethod,
                    None,
                );
                tracing::event(
                    &item,
                    crate::TraceEvent::Rejected(crate::TraceRejection::NoSuccessfulMethod),
                    meter,
                );
                meter.reason = Some("task has no successful method");
                if !choices.restore(&mut active, meter)? {
                    return Ok(Outcome::NoPlan);
                }
                continue;
            }
            if item.method + 1 < methods.len() {
                item.method += 1;
                active.push(item);
                choices.retain(&active, meter)?;
                item = active.pop().ok_or(Outcome::InvalidInput)?;
                item.method -= 1;
            }
            let locals = methods[item.method].locals;
            choices.reserve(
                &active,
                item.cost().saturating_add(2 * locals),
                item.args.values.len().saturating_add(locals),
                meter,
            )?;
            meter.charge(2 * locals)?;
            item.args.locals(locals);
            item.step = Some(0);
            meter.report.stages.record(crate::counters::Stage::Method);
            let location = trace_site(domain, item.task, item.method, None);
            item.trace_method = meter.trace.emit(
                item.trace_task,
                crate::TraceEvent::MethodEntered,
                location,
                Some(crate::trace::Snapshot::Slots(
                    &item.args.values,
                    &item.args.costs,
                )),
                &meter.report,
            );
            active.push(item);
            continue;
        };
        let method = &methods[item.method];
        let Some(instruction) = method.steps.get(step) else {
            if !returned(domain, &item, &mut active, &choices, meter)?
                && !choices.restore(&mut active, meter)?
            {
                return Ok(Outcome::NoPlan);
            }
            continue;
        };
        let trace_step = tracing::step(domain, &item, instruction, meter);
        if let Instruction::Condition { condition } = instruction {
            if item.depth.saturating_add(1) > meter.limits.depth {
                return Err(Outcome::CapacityExceeded);
            }
            choices.reserve(&active, item.cost().saturating_add(2), item.slots(), meter)?;
            meter.charge(2)?;
            item.control = Some(condition::Control::new(*condition, trace_step));
            item.step = Some(step + 1);
            active.push(item);
            continue;
        }
        if let Instruction::Effect { effect, arguments } = instruction {
            effects::apply(
                (domain, facts),
                queries,
                (*effect, arguments, trace_step),
                &item,
                &mut active,
                &choices,
                meter,
            )?;
            item.step = Some(step + 1);
            active.push(item);
            continue;
        }
        let arguments = match instruction {
            Instruction::Require { arguments, .. }
            | Instruction::Action { arguments, .. }
            | Instruction::Task { arguments, .. }
            | Instruction::Defer { arguments, .. } => arguments,
            Instruction::Condition { .. } | Instruction::Effect { .. } => {
                return Err(Outcome::InvalidInput);
            }
        };
        let output_size = expressions::size(arguments, &item.args, meter)?;
        let mut extra = item.cost().saturating_add(output_size);
        if matches!(instruction, Instruction::Task { .. }) {
            extra = extra.saturating_add(1);
        }
        if let Instruction::Defer { task, .. } = instruction {
            extra = extra.saturating_add(
                1 + 32
                    + COMPATIBILITY_ID.len()
                    + domain.definition.tasks[*task].name.len()
                    + domain.source.len()
                    + method.name.len()
                    + 1
                    + domain.definition.tasks[item.task].name.len(),
            );
        }
        if let Instruction::Action { action, .. } = instruction {
            extra = extra.saturating_add(
                1 + domain.schema.actions[*action].name.len()
                    + domain.source.len()
                    + method.name.len()
                    + 1
                    + domain.definition.tasks[item.task].name.len(),
            );
        }
        choices.reserve(
            &active,
            extra,
            item.args.values.len().saturating_add(arguments.len()),
            meter,
        )?;
        let output = expressions::evaluate(arguments, &item.args, output_size, meter)?;
        item.step = Some(step + 1);
        match instruction {
            Instruction::Require { predicate, .. } => {
                if !require::check(
                    (domain, facts, queries),
                    (*predicate, trace_step),
                    &item,
                    &active,
                    &choices,
                    &output,
                    meter,
                )? {
                    if !choices.restore(&mut active, meter)? {
                        return Ok(Outcome::NoPlan);
                    }
                    continue;
                }
                active.push(item);
            }
            Instruction::Action { action, .. } => {
                meter.charge(1)?;
                if active.actions.len() >= domain.limits.plan {
                    return Err(Outcome::CapacityExceeded);
                }
                let name = &domain.schema.actions[*action].name;
                let cost = 1
                    + output.size
                    + name.len()
                    + domain.source.len()
                    + method.name.len()
                    + 1
                    + domain.definition.tasks[item.task].name.len();
                meter.charge(cost)?;
                meter.trace.inherit(
                    trace_step,
                    crate::TraceEvent::StepCompleted,
                    Some(crate::trace::Snapshot::Values(
                        &output.values,
                        &output.costs,
                    )),
                    &meter.report,
                );
                active.actions.push(PlanEntry::Action(Action {
                    name: name.clone(),
                    arguments: output.values,
                    locator: locator(domain, item.task, item.method, Some(step)),
                }));
                active.cost += cost;
                active.push(item);
            }
            Instruction::Defer { task, .. } => {
                meter.charge(1)?;
                if active.actions.len() >= domain.limits.plan {
                    return Err(Outcome::CapacityExceeded);
                }
                let task_name = &domain.definition.tasks[*task].name;
                let cost = 1
                    + output.size
                    + 32
                    + COMPATIBILITY_ID.len()
                    + task_name.len()
                    + domain.source.len()
                    + method.name.len()
                    + 1
                    + domain.definition.tasks[item.task].name.len();
                meter.charge(cost)?;
                meter.trace.inherit(
                    trace_step,
                    crate::TraceEvent::StepCompleted,
                    Some(crate::trace::Snapshot::Values(
                        &output.values,
                        &output.costs,
                    )),
                    &meter.report,
                );
                active.actions.push(PlanEntry::Deferred(DeferredTask {
                    definition: domain.identity(),
                    semantics: COMPATIBILITY_ID.into(),
                    task: task_name.clone(),
                    arguments: output.values,
                    locator: locator(domain, item.task, item.method, Some(step)),
                }));
                active.cost += cost;
                active.push(item);
            }
            Instruction::Task { task, .. } => {
                let depth = item.depth + 1;
                let trace_caller = trace_step;
                active.push(item);
                active.push(Item {
                    explanation_invocation: 0,
                    trace_task: None,
                    trace_method: None,
                    trace_caller,
                    invocation_entered: false,
                    control: None,
                    task: *task,
                    method: 0,
                    step: None,
                    args: Bindings {
                        values: output.values.into_iter().map(Some).collect(),
                        costs: output.costs,
                        size: output.size,
                    },
                    depth,
                });
            }
            Instruction::Condition { .. } | Instruction::Effect { .. } => {
                return Err(Outcome::InvalidInput);
            }
        }
        choices.reserve(&active, 0, 0, meter)?;
    }
}
fn returned(
    domain: &CompiledDomain,
    child: &Item,
    active: &mut Branch,
    choices: &Choices,
    meter: &mut Meter,
) -> Result<bool, Outcome> {
    let method = &domain.tasks[child.task].methods[child.method];
    let size = expressions::size(&method.result, &child.args, meter)?;
    choices.reserve(
        active,
        child.cost().saturating_add(size),
        child.args.values.len().saturating_add(method.result.len()),
        meter,
    )?;
    let result = expressions::evaluate(&method.result, &child.args, size, meter)?;
    meter.trace.inherit(
        child.trace_method,
        crate::TraceEvent::Witness,
        Some(crate::trace::Snapshot::Values(
            &result.values,
            &result.costs,
        )),
        &meter.report,
    );
    let Some(mut parent) = active.pop() else {
        explanation::returned(domain, child, active, meter);
        return Ok(true);
    };
    let step = parent
        .step
        .and_then(|step| step.checked_sub(1))
        .ok_or(Outcome::InvalidInput)?;
    let Instruction::Task { outputs, .. } =
        &domain.tasks[parent.task].methods[parent.method].steps[step]
    else {
        return Err(Outcome::InvalidInput);
    };
    if !expressions::compatible(outputs, &result.values, &result.costs, &parent.args, meter)? {
        explanation::reject_values(
            meter,
            child,
            explanation::Predicate::Control("task_result"),
            &result.values,
            crate::TraceRejection::UnificationConflict,
            None,
        );
        meter.trace.inherit(
            child.trace_caller,
            crate::TraceEvent::Rejected(crate::TraceRejection::UnificationConflict),
            Some(crate::trace::Snapshot::Values(
                &result.values,
                &result.costs,
            )),
            &meter.report,
        );
        meter.reason = Some("task return conflicts with bound caller values");
        return Ok(false);
    }
    let extra = expressions::additional(outputs, &result.costs, &parent.args, meter)?;
    choices.reserve(
        active,
        child
            .cost()
            .saturating_add(parent.cost())
            .saturating_add(result.size)
            .saturating_add(extra),
        child
            .args
            .values
            .len()
            .saturating_add(parent.args.values.len())
            .saturating_add(result.values.len()),
        meter,
    )?;
    expressions::bind(
        outputs,
        &result.values,
        &result.costs,
        &mut parent.args,
        meter,
    )?;
    meter.trace.inherit(
        child.trace_caller,
        crate::TraceEvent::StepCompleted,
        Some(crate::trace::Snapshot::Values(
            &result.values,
            &result.costs,
        )),
        &meter.report,
    );
    active.push(parent);
    explanation::returned(domain, child, active, meter);
    Ok(true)
}

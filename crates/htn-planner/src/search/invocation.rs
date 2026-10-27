use super::state::{Bindings, Branch, Item, Meter};
use crate::{
    COMPATIBILITY_ID, CompiledDomain, DeferredTask, FactSnapshot, Locator, Outcome, PlanningResult,
    TraceFocus, Value, WorkReport,
};
/// Search ordered methods and fact rows over complete continuations.
/// Facts and caller inputs are borrowed; unsuccessful results expose no executable prefix.
pub fn plan(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    entry: &str,
    arguments: &[Value],
) -> PlanningResult {
    decide(
        domain,
        facts,
        (entry, arguments),
        None,
        (None, None),
        None,
        None,
    )
    .result
}
/// Expand a declared deferred invocation against fresh facts with one shared allowance.
/// Deserialized tokens are untrusted: identity, semantics, target, schema and inputs are checked.
pub fn expand_deferred(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    token: &DeferredTask,
) -> PlanningResult {
    decide(
        domain,
        facts,
        (token.task(), token.arguments()),
        Some(token),
        (None, None),
        None,
        None,
    )
    .result
}
/// Plan with independently bounded optional reasoning retention.
pub fn plan_traced(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    entry: &str,
    arguments: &[Value],
    requested_capacity: usize,
) -> crate::TracedPlanningResult {
    decide(
        domain,
        facts,
        (entry, arguments),
        None,
        (Some(requested_capacity), None),
        None,
        None,
    )
}
/// Expand against fresh facts; the returned trace has its own parent-ID universe.
pub fn expand_deferred_traced(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    token: &DeferredTask,
    requested_capacity: usize,
) -> crate::TracedPlanningResult {
    decide(
        domain,
        facts,
        (token.task(), token.arguments()),
        Some(token),
        (Some(requested_capacity), None),
        None,
        None,
    )
}
/// Plan with a pure provider borrowed only for this synchronous attempt.
/// Each registered input prefix is materialized at most once.
pub fn plan_with_queries(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    provider: &dyn super::queries::QueryProvider,
    entry: &str,
    arguments: &[Value],
) -> PlanningResult {
    decide(
        domain,
        facts,
        (entry, arguments),
        None,
        (None, None),
        Some(provider),
        None,
    )
    .result
}
/// Plan with independently capped trace retention and lazy immutable rows.
pub fn plan_traced_with_queries(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    provider: &dyn super::queries::QueryProvider,
    entry: &str,
    arguments: &[Value],
    capacity: usize,
) -> crate::TracedPlanningResult {
    decide(
        domain,
        facts,
        (entry, arguments),
        None,
        (Some(capacity), None),
        Some(provider),
        None,
    )
}
/// Expand using fresh provider observations and a new attempt-local cache.
pub fn expand_deferred_with_queries(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    provider: &dyn super::queries::QueryProvider,
    token: &DeferredTask,
) -> PlanningResult {
    decide(
        domain,
        facts,
        (token.task(), token.arguments()),
        Some(token),
        (None, None),
        Some(provider),
        None,
    )
    .result
}
/// Expand with fresh lazy observations and an independent trace parent universe.
pub fn expand_deferred_traced_with_queries(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    provider: &dyn super::queries::QueryProvider,
    token: &DeferredTask,
    capacity: usize,
) -> crate::TracedPlanningResult {
    decide(
        domain,
        facts,
        (token.task(), token.arguments()),
        Some(token),
        (Some(capacity), None),
        Some(provider),
        None,
    )
}
/// Expand once with independently bounded causal evidence, even when trace capture is off.
/// Rejected alternatives are samples of evaluation, never a guessed unique cause.
pub fn expand_deferred_explained_with_queries(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    provider: &dyn super::queries::QueryProvider,
    token: &DeferredTask,
    trace_capacity: Option<usize>,
) -> super::explanation::ExplainedPlanningResult {
    let mut explanation = super::explanation::SearchExplanation::default();
    let traced = decide(
        domain,
        facts,
        (token.task(), token.arguments()),
        Some(token),
        (trace_capacity, None),
        Some(provider),
        Some(&mut explanation),
    );
    super::explanation::ExplainedPlanningResult {
        result: traced.result,
        trace: trace_capacity.map(|_| traced.trace),
        explanation,
    }
}
/// Plan normally, retaining payload only for the exact task and optional declaration.
///
/// Unknown sites retain no nodes. Task/declaration names over1024bytes retain no payload and report
/// `<oversized-task-focus-omitted>`; all result/work and fixed metadata remain ordinary.
pub fn plan_traced_with_queries_focused(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    provider: &dyn super::queries::QueryProvider,
    entry: &str,
    arguments: &[Value],
    capacity: usize,
    focus: TraceFocus<'_>,
) -> crate::TracedPlanningResult {
    decide(
        domain,
        facts,
        (entry, arguments),
        None,
        (Some(capacity), Some(focus)),
        Some(provider),
        None,
    )
}
/// Expand normally with payload restricted to the exact site focus, in a fresh ID universe.
/// Unknown/oversized focus handling matches `plan_traced_with_queries_focused`.
pub fn expand_deferred_traced_with_queries_focused(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    provider: &dyn super::queries::QueryProvider,
    token: &DeferredTask,
    capacity: usize,
    focus: TraceFocus<'_>,
) -> crate::TracedPlanningResult {
    decide(
        domain,
        facts,
        (token.task(), token.arguments()),
        Some(token),
        (Some(capacity), Some(focus)),
        Some(provider),
        None,
    )
}
/// Preserve the ordinary bounded explanation collector while focusing optional trace payload.
/// Focus affects neither explanation nor result/work, including unknown/oversized names.
pub fn expand_deferred_explained_with_queries_focused(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    provider: &dyn super::queries::QueryProvider,
    token: &DeferredTask,
    trace_capacity: Option<usize>,
    focus: TraceFocus<'_>,
) -> super::explanation::ExplainedPlanningResult {
    let mut explanation = super::explanation::SearchExplanation::default();
    let traced = decide(
        domain,
        facts,
        (token.task(), token.arguments()),
        Some(token),
        (trace_capacity, Some(focus)),
        Some(provider),
        Some(&mut explanation),
    );
    super::explanation::ExplainedPlanningResult {
        result: traced.result,
        trace: trace_capacity.map(|_| traced.trace),
        explanation,
    }
}
fn decide(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    (entry, arguments): (&str, &[Value]),
    token: Option<&DeferredTask>,
    (requested_capacity, focus): (Option<usize>, Option<TraceFocus<'_>>),
    provider: Option<&dyn super::queries::QueryProvider>,
    explanation: Option<&mut super::explanation::SearchExplanation>,
) -> crate::TracedPlanningResult {
    let mut meter = Meter {
        explanation: explanation
            .as_ref()
            .map(|_| super::explanation::Collector::default()),
        trace: focus.map_or_else(
            || crate::trace::Capture::new(domain, requested_capacity),
            |focus| crate::trace::Capture::new_focused(domain, requested_capacity, Some(focus)),
        ),
        limits: domain.limits,
        report: WorkReport::default(),
        site: None,
        provider_predicate: None,
        reason: None,
        next_boundary: 0,
        query_storage: 0,
    };
    meter.trace.attempt(domain, token.is_some(), &meter.report);
    let mut queries = super::queries::Queries::new(provider);
    let outcome = (|| {
        if let Some(token) = token {
            guard(domain, token, &mut meter)?;
        }
        let active = initial(domain, facts, entry, arguments, token.is_some(), &mut meter)?;
        super::search(domain, facts, active, &mut queries, &mut meter)
    })()
    .unwrap_or_else(|failure| failure);
    let outcome = match meter.charge(meter.query_storage) {
        Ok(()) => outcome,
        Err(failure) => failure,
    };
    let message = match &outcome {
        Outcome::Success(_) => None,
        Outcome::NoPlan => Some(meter.reason.unwrap_or("all admitted alternatives failed")),
        Outcome::BudgetExhausted => Some("work allowance exhausted"),
        Outcome::CapacityExceeded => {
            Some("search storage, binding, depth or plan capacity exceeded")
        }
        Outcome::InvalidInput => Some(
            meter
                .reason
                .unwrap_or("entry, schema or arguments violate the admitted contract"),
        ),
    };
    let diagnostic = message.map(|message| crate::Error {
        locator: meter.site.map_or_else(
            || Locator {
                task: None,
                source: domain.source.clone(),
                declaration: if entry.len() <= 1024 {
                    entry.into()
                } else {
                    "<oversized entry>".into()
                },
                step: None,
            },
            |(task, method, step)| super::locator(domain, task, method, step),
        ),
        message: message.into(),
    });
    meter.trace.trace.final_outcome = crate::TraceOutcome::from(&outcome);
    meter.trace.trace.final_work = meter.report.clone();
    meter.trace.emit(
        meter.trace.root,
        crate::TraceEvent::Finished(crate::TraceOutcome::from(&outcome)),
        crate::trace::Site {
            source: &domain.source,
            declaration: domain.name(),
            task: None,
            step: None,
        },
        None,
        &meter.report,
    );
    let result = PlanningResult {
        outcome,
        work: meter.report,
        diagnostic,
    };
    if let Some(output) = explanation
        && let Some(collector) = meter.explanation
    {
        *output = collector.finish(domain, &result.outcome);
    }
    crate::TracedPlanningResult {
        result,
        trace: meter.trace.trace,
    }
}
fn initial(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    entry: &str,
    arguments: &[Value],
    deferred: bool,
    meter: &mut Meter,
) -> Result<Branch, Outcome> {
    meter.charge(domain.schema_cost.saturating_add(facts.schema_cost))?;
    if !std::sync::Arc::ptr_eq(&facts.schema, &domain.snapshot_schema)
        && facts.schema.as_ref() != &domain.schema
    {
        return Err(Outcome::InvalidInput);
    }
    let mut task = None;
    for (index, candidate) in domain.definition.tasks.iter().enumerate() {
        meter.charge(
            1usize
                .saturating_add(candidate.name.len())
                .saturating_add(entry.len()),
        )?;
        if candidate.name == entry {
            task = Some(index);
            break;
        }
    }
    let task = task.ok_or(Outcome::InvalidInput)?;
    if deferred {
        meter.charge(domain.deferred_targets.len())?;
        if !domain.deferred_targets.contains(&task) {
            meter.reason = Some("deferred task target was not declared");
            return Err(Outcome::InvalidInput);
        }
    } else {
        let mut admitted = false;
        for candidate in &domain.definition.entries {
            meter.charge(
                1usize
                    .saturating_add(candidate.len())
                    .saturating_add(entry.len()),
            )?;
            if candidate == entry {
                admitted = true;
                break;
            }
        }
        if !admitted {
            return Err(Outcome::InvalidInput);
        }
    }
    let mut scratch_peak = 0;
    let validated = crate::values::validate_values(
        &domain.schema,
        &domain.definition.tasks[task].parameters,
        arguments,
        domain.limits.storage.min(1024),
        &mut scratch_peak,
        |units| meter.charge(units),
    );
    meter.report.peak_storage = meter.report.peak_storage.max(scratch_peak);
    let costs = validated?;
    meter.charge(costs.len())?;
    let size = costs
        .iter()
        .fold(2 * costs.len(), |total, cost| total.saturating_add(*cost));
    meter.reserve(2usize.saturating_add(size), arguments.len())?;
    meter.charge(size)?;
    let args = Bindings {
        values: arguments.iter().cloned().map(Some).collect(),
        costs,
        size,
    };
    let mut active = Branch {
        selected_empty: None,
        overlay: super::overlay::Overlay::default(),
        pending: Vec::new(),
        actions: Vec::new(),
        cost: 1,
        bindings: 0,
    };
    active.push(Item {
        explanation_invocation: 0,
        trace_task: None,
        trace_method: None,
        trace_caller: meter.trace.root,
        invocation_entered: false,
        control: None,
        task,
        method: 0,
        step: None,
        args,
        depth: 1,
    });
    Ok(active)
}

fn guard(domain: &CompiledDomain, token: &DeferredTask, meter: &mut Meter) -> Result<(), Outcome> {
    meter.charge(64)?;
    if token.definition_identity() != domain.identity() {
        meter.reason = Some("deferred definition identity mismatch");
        return Err(Outcome::InvalidInput);
    }
    meter.charge(
        token
            .semantics()
            .len()
            .saturating_add(COMPATIBILITY_ID.len()),
    )?;
    if token.semantics() != COMPATIBILITY_ID {
        meter.reason = Some("deferred planner semantics mismatch");
        return Err(Outcome::InvalidInput);
    }
    Ok(())
}

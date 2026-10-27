//! Task focus retains actual late evidence without changing the underlying attempt.
use htn_planner::{
    CompiledDomain, Definition, Expression as E, Fact, FactSnapshot, Limits, Method, Outcome,
    PlanEntry, QueryBudget, QueryError, QueryInputs, QueryProvider, Schema, Signature, Step, Task,
    TraceArguments, TraceEvent, TraceFocus, Type, Value, expand_deferred_explained_with_queries,
    expand_deferred_explained_with_queries_focused, expand_deferred_traced_with_queries_focused,
    plan_traced_with_queries, plan_traced_with_queries_focused, plan_with_queries,
};
use std::cell::Cell;

fn fixture() -> Result<(CompiledDomain, FactSnapshot), String> {
    let schema = Schema {
        predicates: vec![
            Signature {
                name: "gate".into(),
                arguments: vec![],
            },
            Signature {
                name: "candidate".into(),
                arguments: vec![Type::Int],
            },
            Signature {
                name: "usable".into(),
                arguments: vec![Type::Int],
            },
        ],
        query_inputs: vec![QueryInputs {
            predicate: "candidate".into(),
            bound: 0,
        }],
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    };
    let method = |name: &str, locals, steps| Method {
        name: name.into(),
        locals,
        steps,
        result: vec![],
    };
    let task = |name: &str, method| Task {
        name: name.into(),
        parameters: vec![],
        outputs: vec![],
        methods: vec![method],
    };
    let definition = Definition {
        version: 1,
        name: "focused".into(),
        entries: vec!["entry".into()],
        queries: vec![],
        tasks: vec![
            task(
                "entry",
                method(
                    "sequence",
                    vec![],
                    vec![
                        Step::Task {
                            name: "noise".into(),
                            arguments: vec![],
                        },
                        Step::Task {
                            name: "chosen".into(),
                            arguments: vec![],
                        },
                        Step::Defer {
                            name: "chosen".into(),
                            arguments: vec![],
                        },
                    ],
                ),
            ),
            task(
                "noise",
                method(
                    "early",
                    vec![],
                    (0..64)
                        .map(|_| Step::Require {
                            name: "gate".into(),
                            arguments: vec![],
                        })
                        .collect(),
                ),
            ),
            task(
                "chosen",
                method(
                    "late",
                    vec![Type::Int],
                    vec![
                        Step::Query {
                            name: "candidate".into(),
                            arguments: vec![E::Local(0)],
                        },
                        Step::Require {
                            name: "usable".into(),
                            arguments: vec![E::Local(0)],
                        },
                        Step::Action {
                            name: "emit".into(),
                            arguments: vec![E::Local(0)],
                        },
                    ],
                ),
            ),
        ],
    };
    let domain = CompiledDomain::admit(
        "focus-source",
        schema,
        definition,
        Limits {
            work: 100_000,
            storage: 16_384,
            depth: 16,
            plan: 8,
            bindings: 128,
            diagnostics: 4096,
        },
    )
    .map_err(|error| error.message)?;
    let facts = domain
        .snapshot(vec![
            Fact {
                predicate: "gate".into(),
                arguments: vec![],
            },
            Fact {
                predicate: "usable".into(),
                arguments: vec![Value::Int(9)],
            },
        ])
        .map_err(|error| format!("{error:?}"))?;
    Ok((domain, facts))
}
struct Rows {
    calls: Cell<usize>,
    observing: Cell<bool>,
    oversized: bool,
}
impl QueryProvider for Rows {
    fn query(
        &self,
        predicate: &str,
        inputs: &[Value],
        budget: &mut QueryBudget<'_>,
    ) -> Result<Vec<Vec<Value>>, QueryError> {
        assert_eq!(predicate, "candidate");
        assert!(inputs.is_empty());
        budget.charge(8)?;
        budget.reserve(64)?;
        self.calls.set(self.calls.get() + 1);
        self.observing.set(budget.observations_enabled());
        // These are scalar observations of the actual supplied batch, not extra queries.
        if budget.observations_enabled() {
            budget.observe("batch_values", &[7, 9]);
            if self.oversized {
                budget.observe("oversized", &[1; 11]);
            }
        }
        Ok(vec![vec![Value::Int(7)], vec![Value::Int(9)]])
    }
}
const fn rows() -> Rows {
    Rows {
        calls: Cell::new(0),
        observing: Cell::new(false),
        oversized: false,
    }
}
fn parents_valid(trace: &htn_planner::Trace) {
    for node in &trace.nodes {
        assert_eq!(node.locator.task.as_deref(), Some("chosen"));
        if let Some(parent) = node.parent {
            assert!(parent < node.id);
            assert!(trace.nodes.iter().any(|node| node.id == parent));
        }
    }
}

#[test_tag::tag(invariant)]
#[test]
fn focused_late_task_retains_actual_selected_witness_after_full_trace_clips() -> Result<(), String>
{
    let (domain, facts) = fixture()?;
    let provider = rows();
    let ordinary = plan_with_queries(&domain, &facts, &provider, "entry", &[]);
    assert!(
        !provider.observing.get(),
        "ordinary OFF skips observation emission loops"
    );
    let full = plan_traced_with_queries(&domain, &facts, &provider, "entry", &[], 4096);
    let focused = plan_traced_with_queries_focused(
        &domain,
        &facts,
        &provider,
        "entry",
        &[],
        4096,
        TraceFocus {
            task: "chosen",
            declaration: None,
        },
    );
    assert_eq!(ordinary, full.result);
    assert_eq!(ordinary, focused.result);
    assert!(provider.observing.get());
    assert_eq!(
        provider.calls.get(),
        3,
        "focus cannot force another materialization"
    );
    assert!(full.trace.truncated);
    assert!(
        full.trace
            .nodes
            .iter()
            .all(|node| node.locator.task.as_deref() != Some("chosen"))
    );
    assert!(!focused.trace.truncated);
    assert_eq!(focused.trace.focus_task.as_deref(), Some("chosen"));
    parents_valid(&focused.trace);
    assert!(focused.trace.nodes.iter().any(|node| node.event == TraceEvent::Witness
        && matches!(&node.arguments, TraceArguments::Slots(values) if values == &vec![Some(Value::Int(9))])));
    assert!(focused.trace.nodes.iter().any(|node| node.event == TraceEvent::HostObservation
        && node.locator.declaration == "batch_values"
        && matches!(&node.arguments, TraceArguments::Slots(values) if values == &vec![Some(Value::Int(7)), Some(Value::Int(9))])));
    let Outcome::Success(entries) = &ordinary.outcome else {
        return Err("no selected action".into());
    };
    assert!(
        matches!(&entries[0], PlanEntry::Action(action) if action.arguments == vec![Value::Int(9)])
    );
    assert_eq!(focused.trace.final_work, ordinary.work);
    assert_eq!(focused.trace.domain, domain.identity());
    Ok(())
}

#[test_tag::tag(invariant)]
#[test]
fn focused_cap_zero_clipped_unknown_and_oversized_filters_preserve_result_and_work()
-> Result<(), String> {
    let (domain, facts) = fixture()?;
    let provider = rows();
    let ordinary = plan_with_queries(&domain, &facts, &provider, "entry", &[]);
    for cap in [0, 16, 4096] {
        let focused = plan_traced_with_queries_focused(
            &domain,
            &facts,
            &provider,
            "entry",
            &[],
            cap,
            TraceFocus {
                task: "chosen",
                declaration: None,
            },
        );
        assert_eq!(focused.result, ordinary);
        assert!(focused.trace.retained_units <= cap);
        parents_valid(&focused.trace);
        if cap <= 16 {
            assert!(focused.trace.nodes.is_empty() && focused.trace.truncated);
        }
    }
    for task in ["unknown".to_owned(), "x".repeat(1024), "x".repeat(1025)] {
        let focused = plan_traced_with_queries_focused(
            &domain,
            &facts,
            &provider,
            "entry",
            &[],
            4096,
            TraceFocus {
                task: &task,
                declaration: None,
            },
        );
        assert_eq!(focused.result, ordinary);
        assert!(focused.trace.nodes.is_empty() && !focused.trace.truncated);
        assert_eq!(
            focused.trace.focus_task.as_deref(),
            Some(if task.len() > 1024 {
                "<oversized-task-focus-omitted>"
            } else {
                &task
            })
        );
        let heap = focused.trace.heap_footprint();
        let retained_task = focused.trace.focus_task.as_ref().unwrap();
        assert_eq!(heap.clone_bytes, retained_task.len());
        assert_eq!(heap.capacity_bytes, retained_task.capacity());
    }
    let full = plan_traced_with_queries(&domain, &facts, &provider, "entry", &[], 0);
    assert!(
        full.trace.focus_task.is_none(),
        "the full profile has no focus"
    );
    Ok(())
}

#[test_tag::tag(invariant)]
#[test]
fn focused_deferred_capture_preserves_fixed_explanation_and_fresh_parent_universe()
-> Result<(), String> {
    let (domain, facts) = fixture()?;
    let provider = rows();
    let root = plan_with_queries(&domain, &facts, &provider, "entry", &[]);
    let Outcome::Success(entries) = root.outcome else {
        return Err("root failed".into());
    };
    let token = entries
        .iter()
        .find_map(|entry| match entry {
            PlanEntry::Deferred(token) => Some(token),
            PlanEntry::Action(_) => None,
        })
        .ok_or("no token")?;
    let full =
        expand_deferred_explained_with_queries(&domain, &facts, &provider, token, Some(4096));
    for cap in [0, 16, 4096] {
        let focus = expand_deferred_traced_with_queries_focused(
            &domain,
            &facts,
            &provider,
            token,
            cap,
            TraceFocus {
                task: "chosen",
                declaration: None,
            },
        );
        let explained = expand_deferred_explained_with_queries_focused(
            &domain,
            &facts,
            &provider,
            token,
            Some(cap),
            TraceFocus {
                task: "chosen",
                declaration: None,
            },
        );
        assert_eq!(focus.result, full.result);
        assert_eq!(explained.result, full.result);
        assert_eq!(explained.explanation, full.explanation);
        assert_eq!(explained.trace.as_ref(), Some(&focus.trace));
        parents_valid(&focus.trace);
    }
    let off = expand_deferred_explained_with_queries_focused(
        &domain,
        &facts,
        &provider,
        token,
        None,
        TraceFocus {
            task: "chosen",
            declaration: None,
        },
    );
    assert_eq!(off.result, full.result);
    assert_eq!(off.explanation, full.explanation);
    assert!(off.trace.is_none());
    Ok(())
}

#[test_tag::tag(invariant)]
#[test]
fn overlong_actual_host_observation_is_explicitly_clipped_without_affecting_search()
-> Result<(), String> {
    let (domain, facts) = fixture()?;
    let provider = Rows {
        calls: Cell::new(0),
        observing: Cell::new(false),
        oversized: true,
    };
    let ordinary = plan_with_queries(&domain, &facts, &provider, "entry", &[]);
    let focused = plan_traced_with_queries_focused(
        &domain,
        &facts,
        &provider,
        "entry",
        &[],
        4096,
        TraceFocus {
            task: "chosen",
            declaration: None,
        },
    );
    assert_eq!(ordinary, focused.result);
    assert!(focused.trace.truncated && focused.trace.dropped_emissions > 0);
    assert!(
        focused
            .trace
            .nodes
            .iter()
            .all(|node| node.locator.declaration != "oversized")
    );
    let unknown = plan_traced_with_queries_focused(
        &domain,
        &facts,
        &provider,
        "entry",
        &[],
        4096,
        TraceFocus {
            task: "absent",
            declaration: None,
        },
    );
    assert_eq!(unknown.result, ordinary);
    assert!(!unknown.trace.truncated && unknown.trace.nodes.is_empty());
    Ok(())
}

struct Exhaust;
impl QueryProvider for Exhaust {
    fn query(
        &self,
        _: &str,
        _: &[Value],
        budget: &mut QueryBudget<'_>,
    ) -> Result<Vec<Vec<Value>>, QueryError> {
        // One impossible actual provider charge, before any row allocation.
        budget.charge(usize::MAX)?;
        Ok(Vec::new())
    }
}
#[test_tag::tag(invariant)]
#[test]
fn focused_work_exhaustion_preserves_result_and_fixed_failure_metadata() -> Result<(), String> {
    let (domain, facts) = fixture()?;
    let ordinary = plan_with_queries(&domain, &facts, &Exhaust, "entry", &[]);
    assert_eq!(ordinary.outcome, Outcome::BudgetExhausted);
    let full = plan_traced_with_queries(&domain, &facts, &Exhaust, "entry", &[], 4096);
    assert_eq!(ordinary, full.result);
    let failure = full
        .trace
        .work_failure
        .ok_or("missing terminal provider charge")?;
    assert!(failure.provider_predicate.is_some() && failure.site.is_some());
    for task in ["chosen", "unknown"] {
        for cap in [0, 16, 4096] {
            let focused = plan_traced_with_queries_focused(
                &domain,
                &facts,
                &Exhaust,
                "entry",
                &[],
                cap,
                TraceFocus {
                    task,
                    declaration: None,
                },
            );
            assert_eq!(focused.result, ordinary);
            assert_eq!(focused.trace.domain, full.trace.domain);
            assert_eq!(focused.trace.final_work, full.trace.final_work);
            assert_eq!(focused.trace.final_outcome, full.trace.final_outcome);
            assert_eq!(focused.trace.work_failure, full.trace.work_failure);
            assert_eq!(focused.trace.provider_work, full.trace.provider_work);
            assert!(focused.trace.retained_units <= cap);
        }
    }
    Ok(())
}

#[path = "focused_trace/declaration.rs"]
mod declaration;

//! Public pure providers retain complete ordered alternatives for one decision.
use htn_planner::{
    CompiledDomain, Definition, Expression as E, Fact, Limits, Method, Outcome, QueryBudget,
    QueryError, QueryInputs, QueryProvider, Schema, Signature, Step, Task, Type, Value,
    plan_with_queries,
};
use std::cell::Cell;

fn domain() -> Result<CompiledDomain, String> {
    let schema = Schema {
        query_inputs: vec![QueryInputs {
            predicate: "candidate".into(),
            bound: 1,
        }],
        predicates: vec![
            Signature {
                name: "candidate".into(),
                arguments: vec![Type::Int, Type::Int],
            },
            Signature {
                name: "usable".into(),
                arguments: vec![Type::Int],
            },
        ],
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    };
    CompiledDomain::admit(
        "lazy",
        schema,
        Definition {
            version: 1,
            name: "lazy".into(),
            entries: vec!["entry".into()],
            queries: vec![],
            tasks: vec![Task {
                name: "entry".into(),
                parameters: vec![Type::Int],
                outputs: vec![],
                methods: vec![Method {
                    name: "select".into(),
                    locals: vec![Type::Int],
                    result: vec![],
                    steps: vec![
                        Step::Query {
                            name: "candidate".into(),
                            arguments: vec![E::Parameter(0), E::Local(0)],
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
                }],
            }],
        },
        Limits::small(),
    )
    .map_err(|error| error.message)
}
struct Candidates(Cell<usize>);
impl QueryProvider for Candidates {
    fn query(
        &self,
        predicate: &str,
        inputs: &[Value],
        budget: &mut QueryBudget<'_>,
    ) -> Result<Vec<Vec<Value>>, QueryError> {
        assert_eq!(predicate, "candidate");
        assert_eq!(inputs, &[Value::Int(3)]);
        budget.charge(8)?;
        budget.reserve(32)?;
        self.0.set(self.0.get() + 1);
        Ok(vec![
            vec![Value::Int(3), Value::Int(5)],
            vec![Value::Int(3), Value::Int(7)],
        ])
    }
}
#[test]
#[test_tag::tag(invariant)]
fn later_failure_revisits_lazy_candidates_without_rematerializing_prefix() -> Result<(), String> {
    let domain = domain()?;
    let facts = domain
        .snapshot(vec![Fact {
            predicate: "usable".into(),
            arguments: vec![Value::Int(7)],
        }])
        .map_err(|error| format!("{error:?}"))?;
    let provider = Candidates(Cell::new(0));
    let result = plan_with_queries(&domain, &facts, &provider, "entry", &[Value::Int(3)]);
    let Outcome::Success(entries) = result.outcome else {
        return Err(format!("expected complete second candidate: {result:?}"));
    };
    assert_eq!(
        entries[0]
            .as_action()
            .ok_or("expected concrete action")?
            .arguments,
        vec![Value::Int(7)]
    );
    // One materialization per concrete prefix is the public provider contract.
    assert_eq!(provider.0.get(), 1);
    Ok(())
}

#[test]
#[test_tag::tag(invariant)]
fn lazy_require_trace_explains_nonmatching_candidate_before_witness() -> Result<(), String> {
    let original = domain()?;
    let mut definition = original.definition().clone();
    definition.tasks[0].methods[0].steps = vec![
        Step::Require {
            name: "candidate".into(),
            arguments: vec![E::Parameter(0), E::Literal(Value::Int(7))],
        },
        Step::Action {
            name: "emit".into(),
            arguments: vec![E::Literal(Value::Int(7))],
        },
    ];
    let domain = CompiledDomain::admit(
        "require",
        original.schema().clone(),
        definition,
        Limits {
            diagnostics: 4096,
            ..Limits::small()
        },
    )
    .map_err(|error| format!("{error:?}"))?;
    let facts = domain
        .snapshot(vec![])
        .map_err(|error| format!("{error:?}"))?;
    let plain_provider = Candidates(Cell::new(0));
    let expected = plan_with_queries(&domain, &facts, &plain_provider, "entry", &[Value::Int(3)]);
    assert!(matches!(expected.outcome, Outcome::Success(_)));
    let provider = Candidates(Cell::new(0));
    let traced = htn_planner::plan_traced_with_queries(
        &domain,
        &facts,
        &provider,
        "entry",
        &[Value::Int(3)],
        4096,
    );
    assert_eq!(traced.result, expected);
    assert_eq!(provider.0.get(), plain_provider.0.get());
    let candidates = traced
        .trace
        .nodes
        .iter()
        .filter(|n| n.event == htn_planner::TraceEvent::QueryCandidate)
        .collect::<Vec<_>>();
    assert_eq!(candidates.len(), 2);
    assert!(
        candidates
            .iter()
            .all(|node| node.locator.declaration == "candidate")
    );
    assert!(
        traced.trace.nodes.iter().any(|node| {
            node.event
                == htn_planner::TraceEvent::Rejected(
                    htn_planner::TraceRejection::UnificationConflict,
                )
                && node.arguments
                    == htn_planner::TraceArguments::Slots(vec![
                        Some(Value::Int(3)),
                        Some(Value::Int(5)),
                    ])
        }),
        "the rejected concrete candidate must remain inspectable before the later witness"
    );
    Ok(())
}

#[path = "lazy_queries/lifecycle.rs"]
mod lifecycle;
#[path = "lazy_queries/overlay.rs"]
mod overlay;
#[path = "lazy_queries/validation.rs"]
mod validation;

struct Exhausting;
impl QueryProvider for Exhausting {
    fn query(
        &self,
        _: &str,
        _: &[Value],
        budget: &mut QueryBudget<'_>,
    ) -> Result<Vec<Vec<Value>>, QueryError> {
        budget.charge(17)?;
        budget.charge(usize::MAX)?;
        unreachable!("the failed charge must abort before any rows are allocated")
    }
}
#[test]
#[test_tag::tag(invariant)]
fn failed_provider_charge_survives_clipped_trace_without_changing_search() -> Result<(), String> {
    let original = domain()?;
    let domain = CompiledDomain::admit(
        "terminal-cost",
        original.schema().clone(),
        original.definition().clone(),
        Limits {
            diagnostics: 4096,
            ..Limits::small()
        },
    )
    .map_err(|error| error.message)?;
    let facts = domain
        .snapshot(vec![])
        .map_err(|error| format!("{error:?}"))?;
    let plain = plan_with_queries(&domain, &facts, &Exhausting, "entry", &[Value::Int(3)]);
    assert_eq!(plain.outcome, Outcome::BudgetExhausted);
    let mut prior = None;
    for cap in [0, 1, 4096] {
        let captured = htn_planner::plan_traced_with_queries(
            &domain,
            &facts,
            &Exhausting,
            "entry",
            &[Value::Int(3)],
            cap,
        );
        assert_eq!(captured.result, plain);
        let failure = captured.trace.work_failure.ok_or("missing terminal cost")?;
        assert_eq!(failure.provider_predicate, Some(0));
        assert_eq!(failure.site, Some((0, 0, Some(0))));
        assert_eq!(
            failure.requested,
            u64::try_from(usize::MAX).map_err(|error| error.to_string())?
        );
        assert!(failure.consumed_before >= 17);
        assert!(failure.requested > failure.limit - failure.consumed_before);
        assert_eq!(captured.trace.provider_work.calls, 1);
        assert_eq!(
            captured.trace.provider_work.maximum_call,
            Some((0, captured.trace.provider_work.consumed))
        );
        assert_eq!(
            captured.trace.provider_work.consumed,
            17 + failure.limit - failure.consumed_before
        );
        assert_eq!(captured.trace.final_work, plain.work);
        assert!(captured.trace.retained_units <= cap);
        if cap == 0 {
            assert_eq!(captured.trace.nodes.capacity(), 0);
        }
        if let Some(previous) = prior {
            assert_eq!(failure, previous);
        }
        prior = Some(failure);
    }
    Ok(())
}

use htn_planner::{
    CompiledDomain, Condition, Definition, DerivedQuery, Expression as E, Fact, FactSnapshot,
    Limits, Method, Schema, Signature, Step, Task, TraceControl, TraceEvent, Type, Value, plan,
    plan_traced,
};
fn fixture(
    first_success: bool,
    restrict_first: bool,
) -> Result<(CompiledDomain, FactSnapshot), String> {
    let schema = Schema {
        predicates: vec![
            Signature {
                name: "row".into(),
                arguments: vec![Type::Int],
            },
            Signature {
                name: "accepted".into(),
                arguments: vec![Type::Int],
            },
        ],
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    };
    let mut query = Condition::Fact {
        name: "row".into(),
        arguments: vec![E::Local(0)],
    };
    if restrict_first {
        query = Condition::And(vec![
            query,
            Condition::Compare {
                op: htn_planner::Comparison::Equal,
                left: E::Local(0),
                right: E::Literal(Value::Int(1)),
            },
        ]);
    }
    let children = vec![
        Condition::Derived {
            name: "selected-query".into(),
            arguments: vec![],
            outputs: vec![E::Local(0)],
        },
        Condition::Fact {
            name: "row".into(),
            arguments: vec![E::Local(0)],
        },
    ];
    let control = if first_success {
        Condition::FirstSuccess(children)
    } else {
        Condition::Or(children)
    };
    let raw = Definition {
        version: 1,
        name: "control".into(),
        queries: vec![DerivedQuery {
            name: "selected-query".into(),
            parameters: vec![],
            outputs: vec![Type::Int],
            locals: vec![Type::Int],
            condition: query,
            result: vec![E::Local(0)],
        }],
        entries: vec!["entry".into()],
        tasks: vec![Task {
            name: "entry".into(),
            parameters: vec![],
            outputs: vec![],
            methods: vec![Method {
                name: "select".into(),
                locals: vec![Type::Int],
                result: vec![],
                steps: vec![
                    Step::Condition(control),
                    Step::Require {
                        name: "accepted".into(),
                        arguments: vec![E::Local(0)],
                    },
                    Step::Action {
                        name: "emit".into(),
                        arguments: vec![E::Local(0)],
                    },
                ],
            }],
        }],
    };
    let mut limits = Limits::small();
    limits.diagnostics = 4096;
    let domain = CompiledDomain::admit("control-source", schema.clone(), raw, limits)
        .map_err(|e| e.message)?;
    let facts = FactSnapshot::new(
        &schema,
        [("row", 1), ("row", 2), ("accepted", 2)]
            .into_iter()
            .map(|(name, value)| Fact {
                predicate: name.into(),
                arguments: vec![Value::Int(value)],
            })
            .collect(),
    )?;
    Ok((domain, facts))
}
#[test_tag::tag(unit)]
#[test]
fn selected_child_trace_keeps_witnesses_without_resurrecting_sibling_alternative() {
    for restrict in [false, true] {
        let (domain, facts) = fixture(true, restrict).unwrap();
        let traced = plan_traced(&domain, &facts, "entry", &[], 4096);
        assert_eq!(traced.result, plan(&domain, &facts, "entry", &[]));
        assert!(!traced.trace.truncated);
        let boundaries = traced
            .trace
            .nodes
            .iter()
            .filter(|node| {
                matches!(
                    node.event,
                    TraceEvent::ControlEntered(TraceControl::FirstSuccess)
                )
            })
            .collect::<Vec<_>>();
        assert!(!boundaries.is_empty());
        assert!(
            traced
                .trace
                .nodes
                .iter()
                .any(|node| matches!(node.event, TraceEvent::FirstSuccessSelected))
        );
        for boundary in boundaries {
            assert!(
                !traced
                    .trace
                    .nodes
                    .iter()
                    .any(|node| node.parent == Some(boundary.id)
                        && matches!(node.event, TraceEvent::AlternativeEntered { ordinal: 1 }))
            );
        }
        assert!(traced.trace.nodes.iter().any(|node| matches!(
            node.event,
            TraceEvent::DerivedQueryEntered
        ) && node.locator.declaration
            == "selected-query"
            && node.locator.task.as_deref() == Some("entry")));
    }
}

#[test_tag::tag(unit)]
#[test]
fn negation_probe_ancestry_distinguishes_witness_rejection_from_absence() {
    let (domain, facts) = fixture(true, false).unwrap();
    let mut raw = domain.definition().clone();
    raw.queries[0].outputs.clear();
    raw.queries[0].result.clear();
    raw.tasks[0].methods[0].locals.clear();
    raw.tasks[0].methods[0].steps = vec![
        Step::Condition(Condition::Not(Box::new(Condition::Derived {
            name: "selected-query".into(),
            arguments: vec![],
            outputs: vec![],
        }))),
        Step::Action {
            name: "emit".into(),
            arguments: vec![E::Literal(Value::Int(7))],
        },
    ];
    let domain =
        CompiledDomain::admit("probe", domain.schema().clone(), raw, domain.limits()).unwrap();
    for facts in [facts, FactSnapshot::new(domain.schema(), vec![]).unwrap()] {
        let traced = plan_traced(&domain, &facts, "entry", &[], 4096);
        assert_eq!(traced.result, plan(&domain, &facts, "entry", &[]));
        let probe = traced
            .trace
            .nodes
            .iter()
            .find(|node| matches!(node.event, TraceEvent::ControlEntered(TraceControl::Not)))
            .unwrap();
        assert!(
            traced
                .trace
                .nodes
                .iter()
                .any(|node| node.parent == Some(probe.id)
                    && matches!(node.event, TraceEvent::DerivedQueryEntered))
        );
        match traced.result.outcome {
            htn_planner::Outcome::Success(_) => assert!(
                traced
                    .trace
                    .nodes
                    .iter()
                    .any(|node| node.parent == Some(probe.id)
                        && matches!(node.event, TraceEvent::Witness))
            ),
            htn_planner::Outcome::NoPlan => assert!(traced.trace.nodes.iter().any(
                |node| node.parent == Some(probe.id)
                    && matches!(
                        node.event,
                        TraceEvent::Rejected(htn_planner::TraceRejection::ProbeFoundWitness)
                    )
            )),
            other => panic!("{other:?}"),
        }
    }
}

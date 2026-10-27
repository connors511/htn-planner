use htn_planner::{
    CompiledDomain, Definition, Expression as E, Fact, FactSnapshot, Limits, Method, Schema,
    Signature, Step, Task, TraceArguments, TraceEvent, Type, Value, plan, plan_traced,
};
fn fixture() -> Result<(CompiledDomain, FactSnapshot), String> {
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
            arguments: vec![Type::Int, Type::Int],
        }],
        ..Schema::default()
    };
    let raw = Definition {
        version: 1,
        name: "nested".into(),
        queries: vec![],
        entries: vec!["entry".into()],
        tasks: vec![
            Task {
                name: "entry".into(),
                parameters: vec![],
                outputs: vec![],
                methods: vec![Method {
                    name: "parent".into(),
                    locals: vec![Type::Int, Type::Int],
                    result: vec![],
                    steps: vec![
                        Step::Call {
                            name: "child".into(),
                            arguments: vec![],
                            outputs: vec![E::Local(0)],
                        },
                        Step::Call {
                            name: "child".into(),
                            arguments: vec![],
                            outputs: vec![E::Local(1)],
                        },
                        Step::Require {
                            name: "accepted".into(),
                            arguments: vec![E::Local(0)],
                        },
                        Step::Action {
                            name: "emit".into(),
                            arguments: vec![E::Local(0), E::Local(1)],
                        },
                    ],
                }],
            },
            Task {
                name: "child".into(),
                parameters: vec![],
                outputs: vec![Type::Int],
                methods: vec![Method {
                    name: "enumerate".into(),
                    locals: vec![Type::Int],
                    result: vec![E::Local(0)],
                    steps: vec![Step::Query {
                        name: "row".into(),
                        arguments: vec![E::Local(0)],
                    }],
                }],
            },
        ],
    };
    let mut limits = Limits::small();
    limits.diagnostics = 4096;
    let domain = CompiledDomain::admit("nested-source", schema.clone(), raw, limits)
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
fn repeated_child_queries_preserve_invocation_parentage_across_backtracking() {
    let (domain, facts) = fixture().unwrap();
    let traced = plan_traced(&domain, &facts, "entry", &[], 4096);
    assert_eq!(traced.result, plan(&domain, &facts, "entry", &[]));
    assert!(!traced.trace.truncated);
    let queries = traced
        .trace
        .nodes
        .iter()
        .filter(|node| {
            matches!(node.event, TraceEvent::FactQueryEntered) && node.locator.declaration == "row"
        })
        .collect::<Vec<_>>();
    assert!(queries.len() > 1);
    for query in &queries {
        assert!(query.work.stages.queries_entered > 0);
        assert_eq!(query.locator.declaration, "row");
        assert_eq!(query.locator.task.as_deref(), Some("child"));
    }
    let witnesses = traced
        .trace
        .nodes
        .iter()
        .filter(|node| {
            matches!(node.event, TraceEvent::Witness)
                && node
                    .parent
                    .is_some_and(|parent| queries.iter().any(|query| query.id == parent))
        })
        .collect::<Vec<_>>();
    for expected in [1, 2] {
        assert!(witnesses.iter().any(|node|matches!(&node.arguments,TraceArguments::Slots(values) if values==&vec![Some(Value::Int(expected))])));
    }
    for node in &traced.trace.nodes {
        if let Some(parent) = node.parent {
            assert!(
                traced
                    .trace
                    .nodes
                    .iter()
                    .any(|ancestor| ancestor.id == parent && ancestor.id < node.id)
            );
        }
    }
    assert!(traced.result.work.stages.choices_restored > 0);
}

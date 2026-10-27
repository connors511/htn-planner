use htn_planner::{
    CompiledDomain, Condition, Definition, EffectOperation, Expression, Limits, Method,
    PlanningEffect, Schema, Signature, Step, Task, Type, Value,
};
fn definition(condition: Condition) -> Definition {
    Definition {
        version: 1,
        name: "admission".into(),
        entries: vec!["entry".into()],
        queries: vec![],
        tasks: vec![Task {
            name: "entry".into(),
            parameters: vec![],
            outputs: vec![],
            methods: vec![Method {
                name: "empty".into(),
                locals: vec![],
                result: vec![],
                steps: vec![Step::Condition(condition)],
            }],
        }],
    }
}
#[test_tag::tag(invariant)]
#[test]
fn oversized_programmatic_condition_depth_rejects_without_recursive_disposal() {
    let mut condition = Condition::And(vec![]);
    for _ in 0..10_000 {
        condition = Condition::Not(Box::new(condition));
    }
    assert!(
        CompiledDomain::admit(
            "deep",
            Schema::default(),
            definition(condition),
            Limits::small()
        )
        .is_err()
    );
}
#[test_tag::tag(invariant)]
#[test]
fn registered_effect_operations_reject_unknown_predicates_types_and_local_slots() {
    let signature = Signature {
        name: "row".into(),
        arguments: vec![Type::Int],
    };
    for (predicate, argument) in [
        ("missing", Expression::Parameter(0)),
        ("row", Expression::Literal(Value::Bool(true))),
        ("row", Expression::Local(0)),
    ] {
        let schema = Schema {
            predicates: vec![signature.clone()],
            effects: vec![PlanningEffect {
                name: "effect".into(),
                parameters: vec![Type::Int],
                operations: vec![EffectOperation::Assert {
                    predicate: predicate.into(),
                    arguments: vec![argument],
                }],
            }],
            ..Schema::default()
        };
        assert!(
            CompiledDomain::admit(
                "effect",
                schema,
                definition(Condition::And(vec![])),
                Limits::small()
            )
            .is_err()
        );
    }
}

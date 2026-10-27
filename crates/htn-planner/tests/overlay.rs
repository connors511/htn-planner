use htn_planner::{
    CompiledDomain, Condition, Definition, EffectOperation, Expression as E, Fact, FactSnapshot,
    Limits, Method, Outcome, PlanningEffect, Schema, Signature, Step, Task, Type, Value, plan,
};
fn schema() -> Schema {
    Schema {
        predicates: vec![Signature {
            name: "row".into(),
            arguments: vec![Type::Int],
        }],
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![Type::Int],
        }],
        effects: vec![
            PlanningEffect {
                name: "add".into(),
                parameters: vec![Type::Int],
                operations: vec![EffectOperation::Assert {
                    predicate: "row".into(),
                    arguments: vec![E::Parameter(0)],
                }],
            },
            PlanningEffect {
                name: "remove".into(),
                parameters: vec![Type::Int],
                operations: vec![EffectOperation::Retract {
                    predicate: "row".into(),
                    arguments: vec![E::Parameter(0)],
                }],
            },
        ],
        ..Schema::default()
    }
}
const fn lit(value: i64) -> E {
    E::Literal(Value::Int(value))
}
fn effect(name: &str, value: i64) -> Step {
    Step::Effect {
        name: name.into(),
        arguments: vec![lit(value)],
    }
}
fn method(name: &str, steps: Vec<Step>) -> Method {
    Method {
        name: name.into(),
        locals: vec![Type::Int],
        result: vec![],
        steps,
    }
}
fn definition(methods: Vec<Method>) -> Definition {
    Definition {
        version: 1,
        queries: vec![],
        name: "overlay".into(),
        entries: vec!["entry".into()],
        tasks: vec![Task {
            name: "entry".into(),
            parameters: vec![],
            outputs: vec![],
            methods,
        }],
    }
}
fn snapshot(rows: &[i64]) -> Result<FactSnapshot, String> {
    FactSnapshot::new(
        &schema(),
        rows.iter()
            .map(|value| Fact {
                predicate: "row".into(),
                arguments: vec![Value::Int(*value)],
            })
            .collect(),
    )
}
fn query_emit() -> Vec<Step> {
    vec![
        Step::Query {
            name: "row".into(),
            arguments: vec![E::Local(0)],
        },
        Step::Action {
            name: "emit".into(),
            arguments: vec![E::Local(0)],
        },
    ]
}

#[test_tag::tag(unit)]
#[test]
fn retract_hides_all_matches_and_reassert_appends_after_visible_base_rows() {
    let mut steps = vec![effect("remove", 1), effect("add", 1)];
    steps.extend(query_emit());
    let domain = CompiledDomain::admit(
        "order",
        schema(),
        definition(vec![method("select", steps)]),
        Limits::small(),
    )
    .unwrap();
    let facts = snapshot(&[1, 1, 2]).unwrap();
    let before = facts.clone();
    let result = plan(&domain, &facts, "entry", &[]);
    let Outcome::Success(entries) = result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(
        entries[0].as_action().unwrap().arguments,
        vec![Value::Int(2)]
    );
    assert_eq!(facts, before);
}
#[test_tag::tag(unit)]
#[test]
fn duplicate_assertions_are_idempotent_and_do_not_accumulate_storage() {
    let mut steps = vec![effect("add", 1), effect("add", 1)];
    steps.extend(query_emit());
    let facts = snapshot(&[1]).unwrap();
    let first = CompiledDomain::admit(
        "duplicates",
        schema(),
        definition(vec![method("same", steps.clone())]),
        Limits::small(),
    )
    .unwrap();
    steps.remove(0);
    let second = CompiledDomain::admit(
        "duplicates",
        schema(),
        definition(vec![method("same", steps)]),
        Limits::small(),
    )
    .unwrap();
    let first = plan(&first, &facts, "entry", &[]);
    let second = plan(&second, &facts, "entry", &[]);
    let Outcome::Success(first_entries) = &first.outcome else {
        panic!("{first:?}")
    };
    let Outcome::Success(second_entries) = &second.outcome else {
        panic!("{second:?}")
    };
    assert_eq!(
        first_entries[0].as_action().unwrap().arguments,
        second_entries[0].as_action().unwrap().arguments
    );
    assert_eq!(first.work.peak_storage, second.work.peak_storage);
    assert!(first.work.consumed > second.work.consumed);
}
#[test_tag::tag(unit)]
#[test]
fn failed_method_restores_overlay_and_action_prefix_before_next_method() {
    let rejected = method(
        "rejected",
        vec![
            effect("remove", 1),
            effect("add", 2),
            Step::Action {
                name: "emit".into(),
                arguments: vec![lit(99)],
            },
            Step::Require {
                name: "row".into(),
                arguments: vec![lit(1)],
            },
        ],
    );
    let mut steps = vec![Step::Condition(Condition::Not(Box::new(Condition::Fact {
        name: "row".into(),
        arguments: vec![lit(2)],
    })))];
    steps.extend(query_emit());
    let domain = CompiledDomain::admit(
        "rollback",
        schema(),
        definition(vec![rejected, method("accepted", steps)]),
        Limits::small(),
    )
    .unwrap();
    let facts = snapshot(&[1]).unwrap();
    let before = facts.clone();
    let result = plan(&domain, &facts, "entry", &[]);
    let Outcome::Success(entries) = result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].as_action().unwrap().arguments,
        vec![Value::Int(1)]
    );
    assert_eq!(facts, before);
}
#[test_tag::tag(invariant)]
#[test]
fn overlay_storage_and_work_boundaries_return_no_prefix_and_preserve_facts() {
    let mut steps = vec![effect("add", 1), effect("add", 2)];
    steps.extend(query_emit());
    let raw = definition(vec![method("select", steps)]);
    let facts = snapshot(&[]).unwrap();
    let before = facts.clone();
    let mut limits = Limits::small();
    let domain = CompiledDomain::admit("bounds", schema(), raw.clone(), limits).unwrap();
    let baseline = plan(&domain, &facts, "entry", &[]);
    assert!(matches!(baseline.outcome, Outcome::Success(_)));
    limits.storage = baseline.work.peak_storage - 1;
    let domain = CompiledDomain::admit("bounds", schema(), raw.clone(), limits).unwrap();
    assert_eq!(
        plan(&domain, &facts, "entry", &[]).outcome,
        Outcome::CapacityExceeded
    );
    limits = Limits::small();
    limits.work = baseline.work.consumed - 1;
    let domain = CompiledDomain::admit("bounds", schema(), raw, limits).unwrap();
    assert_eq!(
        plan(&domain, &facts, "entry", &[]).outcome,
        Outcome::BudgetExhausted
    );
    assert_eq!(facts, before);
}

#[test_tag::tag(invariant)]
#[test]
fn large_overlay_payload_and_retained_query_copy_obey_exact_reported_boundaries() {
    let mut schema = schema();
    schema.predicates[0].arguments = vec![Type::Symbol];
    schema.actions[0].arguments = vec![Type::Symbol];
    for effect in &mut schema.effects {
        effect.parameters = vec![Type::Symbol];
    }
    let inserted = Value::Symbol("inserted".repeat(512));
    let mut selected = method(
        "payload",
        vec![Step::Effect {
            name: "add".into(),
            arguments: vec![E::Literal(inserted)],
        }],
    );
    selected.locals = vec![Type::Symbol];
    selected.steps.extend(query_emit());
    let raw = definition(vec![selected]);
    let facts = FactSnapshot::new(
        &schema,
        vec![Fact {
            predicate: "row".into(),
            arguments: vec![Value::Symbol("base".repeat(512))],
        }],
    )
    .unwrap();
    let before = facts.clone();
    let mut limits = Limits::small();
    limits.storage = 100_000;
    limits.work = 100_000;
    let domain = CompiledDomain::admit("large", schema.clone(), raw.clone(), limits).unwrap();
    let baseline = plan(&domain, &facts, "entry", &[]);
    assert!(matches!(baseline.outcome, Outcome::Success(_)));
    limits.storage = baseline.work.peak_storage;
    limits.work = baseline.work.consumed;
    let domain = CompiledDomain::admit("large", schema.clone(), raw.clone(), limits).unwrap();
    assert_eq!(plan(&domain, &facts, "entry", &[]), baseline);
    limits.storage -= 1;
    let domain = CompiledDomain::admit("large", schema.clone(), raw.clone(), limits).unwrap();
    assert_eq!(
        plan(&domain, &facts, "entry", &[]).outcome,
        Outcome::CapacityExceeded
    );
    limits.storage += 1;
    limits.work -= 1;
    let domain = CompiledDomain::admit("large", schema, raw, limits).unwrap();
    assert_eq!(
        plan(&domain, &facts, "entry", &[]).outcome,
        Outcome::BudgetExhausted
    );
    assert_eq!(facts, before);
}

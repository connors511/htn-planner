use htn_planner::{
    CompiledDomain, Condition, Definition, Expression, Fact, FactSnapshot, Limits, Method,
    NumericOperation, Outcome, Schema, Signature, Step, Task, TraceOutcome, Type, Value, plan,
    plan_traced,
};
fn definition(steps: Vec<Step>) -> Definition {
    Definition {
        version: 1,
        name: "neutral".into(),
        queries: vec![],
        entries: vec!["entry".into()],
        tasks: vec![Task {
            name: "entry".into(),
            parameters: vec![],
            outputs: vec![],
            methods: vec![Method {
                name: "method".into(),
                locals: vec![],
                result: vec![],
                steps,
            }],
        }],
    }
}
fn neutral(
    domain: &CompiledDomain,
    facts: &FactSnapshot,
    entry: &str,
    args: &[Value],
) -> Result<htn_planner::PlanningResult, String> {
    let plain = plan(domain, facts, entry, args);
    for cap in [0, 1, 32, 4096] {
        let captured = plan_traced(domain, facts, entry, args, cap);
        if captured.result != plain
            || captured.trace.final_work != plain.work
            || captured.trace.final_outcome != TraceOutcome::from(&plain.outcome)
        {
            return Err("trace changed complete result/work".into());
        }
        if captured.trace.retained_units > captured.trace.capacity {
            return Err("trace capacity exceeded".into());
        }
        if cap == 0 && captured.trace.nodes.capacity() != 0 {
            return Err("zero cap allocated a node vector".into());
        }
    }
    Ok(plain)
}
#[test_tag::tag(invariant)]
#[test]
fn every_terminal_category_and_empty_success_preserves_result_work_across_caps() {
    let schema = Schema {
        predicates: vec![Signature {
            name: "present".into(),
            arguments: vec![],
        }],
        ..Schema::default()
    };
    let facts = FactSnapshot::new(&schema, vec![]).unwrap();
    let mut limits = Limits::small();
    limits.diagnostics = 4096;
    let domain =
        CompiledDomain::admit("empty", schema.clone(), definition(vec![]), limits).unwrap();
    assert_eq!(
        neutral(&domain, &facts, "entry", &[]).unwrap().outcome,
        Outcome::Success(vec![])
    );
    assert_eq!(
        neutral(&domain, &facts, "missing", &[]).unwrap().outcome,
        Outcome::InvalidInput
    );
    let absent = definition(vec![Step::Require {
        name: "present".into(),
        arguments: vec![],
    }]);
    let domain = CompiledDomain::admit("absent", schema.clone(), absent.clone(), limits).unwrap();
    assert_eq!(
        neutral(&domain, &facts, "entry", &[]).unwrap().outcome,
        Outcome::NoPlan
    );
    limits.work = 1;
    let domain = CompiledDomain::admit("budget", schema.clone(), absent.clone(), limits).unwrap();
    let result = neutral(&domain, &facts, "entry", &[]).unwrap();
    assert_eq!(result.outcome, Outcome::BudgetExhausted);
    assert_eq!(result.work.stages.tasks_entered, 0);
    limits = Limits::small();
    limits.storage = 1;
    let domain = CompiledDomain::admit("storage", schema, absent, limits).unwrap();
    assert_eq!(
        neutral(&domain, &facts, "entry", &[]).unwrap().outcome,
        Outcome::CapacityExceeded
    );
}
#[test_tag::tag(invariant)]
#[test]
fn overflow_inside_negation_is_neutral_invalid_input_with_originating_step() {
    let schema = Schema::default();
    let facts = FactSnapshot::new(&schema, vec![]).unwrap();
    let condition = Condition::Not(Box::new(Condition::Calculate {
        op: NumericOperation::Add,
        left: Expression::Literal(Value::Int(i64::MAX)),
        right: Expression::Literal(Value::Int(1)),
        output: Expression::Literal(Value::Int(0)),
    }));
    let mut limits = Limits::small();
    limits.diagnostics = 4096;
    let domain = CompiledDomain::admit(
        "overflow",
        schema,
        definition(vec![Step::Condition(condition)]),
        limits,
    )
    .unwrap();
    let result = neutral(&domain, &facts, "entry", &[]).unwrap();
    assert_eq!(result.outcome, Outcome::InvalidInput);
    assert_eq!(result.diagnostic.unwrap().locator.step, Some(0));
}
#[test_tag::tag(invariant)]
#[test]
fn high_fan_out_truncation_preserves_complete_search_and_reusable_capture() {
    let schema = Schema {
        predicates: vec![Signature {
            name: "row".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    };
    let mut raw = definition(vec![Step::Condition(Condition::Or(
        (0..100)
            .map(|index| Condition::Fact {
                name: "row".into(),
                arguments: vec![Expression::Literal(Value::Int(index))],
            })
            .collect(),
    ))]);
    raw.tasks[0].methods[0].locals = vec![];
    let mut limits = Limits::small();
    limits.work = 100_000;
    limits.storage = 65_536;
    limits.diagnostics = 4096;
    let domain = CompiledDomain::admit("fan-out", schema.clone(), raw, limits).unwrap();
    let facts = FactSnapshot::new(
        &schema,
        vec![Fact {
            predicate: "row".into(),
            arguments: vec![Value::Int(99)],
        }],
    )
    .unwrap();
    let before = facts.clone();
    let result = neutral(&domain, &facts, "entry", &[]).unwrap();
    assert!(matches!(result.outcome, Outcome::Success(_)));
    let first = plan_traced(&domain, &facts, "entry", &[], 128);
    assert!(first.trace.truncated);
    assert!(first.trace.dropped_emissions > 0);
    assert!(first.trace.retained_units <= 128);
    assert_eq!(first, plan_traced(&domain, &facts, "entry", &[], 128));
    assert_eq!(facts, before);
}

#[test_tag::tag(invariant)]
#[test]
fn large_or_deep_unvalidated_values_are_omitted_before_trace_payload_copy() {
    let mut raw = definition(vec![]);
    raw.tasks[0].parameters = vec![Type::Symbol];
    let mut limits = Limits::small();
    limits.diagnostics = 4096;
    let domain = CompiledDomain::admit("large", Schema::default(), raw, limits).unwrap();
    let facts = FactSnapshot::new(domain.schema(), vec![]).unwrap();
    let oversized = [Value::Symbol("private-value".repeat(10_000))];
    let result = neutral(&domain, &facts, "entry", &oversized).unwrap();
    assert_eq!(result.outcome, Outcome::BudgetExhausted);
    let captured = plan_traced(&domain, &facts, "entry", &oversized, 4096);
    assert!(
        captured
            .trace
            .nodes
            .iter()
            .all(|node| !matches!(node.arguments, htn_planner::TraceArguments::Slots(_)))
    );
    let schema = Schema {
        records: vec![htn_planner::RecordType {
            name: "node".into(),
            fields: vec![Type::Record("node".into())],
        }],
        ..Schema::default()
    };
    let mut raw = definition(vec![]);
    raw.tasks[0].parameters = vec![Type::Record("node".into())];
    let domain = CompiledDomain::admit("deep", schema.clone(), raw, limits).unwrap();
    let facts = FactSnapshot::new(&schema, vec![]).unwrap();
    let mut value = Value::Bool(false);
    for _ in 0..130 {
        value = Value::Record {
            type_name: "node".into(),
            fields: vec![value],
        };
    }
    let result = neutral(&domain, &facts, "entry", &[value]).unwrap();
    assert_eq!(result.outcome, Outcome::CapacityExceeded);
}

#[test_tag::tag(invariant)]
#[test]
fn negation_probe_exhaustion_never_becomes_evidence_of_absence() {
    let schema = Schema {
        predicates: vec![
            Signature {
                name: "absent".into(),
                arguments: vec![],
            },
            Signature {
                name: "other".into(),
                arguments: vec![Type::Int],
            },
        ],
        ..Schema::default()
    };
    let condition = Condition::Not(Box::new(Condition::Fact {
        name: "absent".into(),
        arguments: vec![],
    }));
    let raw = definition(vec![Step::Condition(condition)]);
    let facts = FactSnapshot::new(
        &schema,
        (0..1000)
            .map(|value| Fact {
                predicate: "other".into(),
                arguments: vec![Value::Int(value)],
            })
            .collect(),
    )
    .unwrap();
    let mut limits = Limits::small();
    limits.diagnostics = 4096;
    let domain =
        CompiledDomain::admit("probe-exhaustion", schema.clone(), raw.clone(), limits).unwrap();
    let complete = plan(&domain, &facts, "entry", &[]);
    assert!(matches!(complete.outcome, Outcome::Success(_)));
    limits.work = complete.work.consumed / 2;
    let domain =
        CompiledDomain::admit("probe-exhaustion", schema.clone(), raw.clone(), limits).unwrap();
    let exhausted = neutral(&domain, &facts, "entry", &[]).unwrap();
    assert_eq!(exhausted.outcome, Outcome::BudgetExhausted);
    assert!(exhausted.work.stages.queries_entered > 0);
    assert!(exhausted.work.stages.rows_inspected > 0);
    limits = Limits::small();
    limits.diagnostics = 4096;
    limits.storage = complete.work.peak_storage - 1;
    let domain = CompiledDomain::admit("probe-exhaustion", schema, raw, limits).unwrap();
    assert_eq!(
        neutral(&domain, &facts, "entry", &[]).unwrap().outcome,
        Outcome::CapacityExceeded
    );
}

use htn_planner::{
    CompiledDomain, Definition, EffectOperation, Expression as E, FactSnapshot, Limits, Method,
    PlanningEffect, Schema, Signature, Step, Task, TraceEvent, TraceStep, Type, Value, plan,
    plan_traced,
};
#[test_tag::tag(unit)]
#[test]
fn typed_steps_and_registered_effect_operations_retain_source_and_bound_arguments() {
    let schema = Schema {
        predicates: vec![Signature {
            name: "claimed".into(),
            arguments: vec![Type::Int],
        }],
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![Type::Int],
        }],
        effects: vec![PlanningEffect {
            name: "claim".into(),
            parameters: vec![Type::Int],
            operations: vec![EffectOperation::Assert {
                predicate: "claimed".into(),
                arguments: vec![E::Parameter(0)],
            }],
        }],
        ..Schema::default()
    };
    let raw = Definition {
        version: 1,
        name: "steps".into(),
        queries: vec![],
        entries: vec!["entry".into()],
        tasks: vec![Task {
            name: "entry".into(),
            parameters: vec![],
            outputs: vec![],
            methods: vec![Method {
                name: "claim-and-emit".into(),
                locals: vec![],
                result: vec![],
                steps: vec![
                    Step::Effect {
                        name: "claim".into(),
                        arguments: vec![E::Literal(Value::Int(7))],
                    },
                    Step::Require {
                        name: "claimed".into(),
                        arguments: vec![E::Literal(Value::Int(7))],
                    },
                    Step::Action {
                        name: "emit".into(),
                        arguments: vec![E::Literal(Value::Int(7))],
                    },
                ],
            }],
        }],
    };
    let mut limits = Limits::small();
    limits.diagnostics = 4096;
    let domain = CompiledDomain::admit("steps-source", schema.clone(), raw, limits).unwrap();
    let facts = FactSnapshot::new(&schema, vec![]).unwrap();
    let traced = plan_traced(&domain, &facts, "entry", &[], 4096);
    assert_eq!(traced.result, plan(&domain, &facts, "entry", &[]));
    assert!(traced.trace.nodes.iter().any(|node| matches!(
        node.event,
        TraceEvent::StepEntered(TraceStep::Action)
    ) && node.locator.step == Some(2)));
    assert!(traced.trace.nodes.iter().any(|node| matches!(
        node.event,
        TraceEvent::EffectOperationCompleted { .. }
    ) && node.locator.declaration == "claim"
        && node.locator.step == Some(0)));
    assert_eq!(traced.result.work.stages.effect_operations_completed, 1);
    assert_eq!(traced.result.work.stages.queries_entered, 1);
    assert!(traced.trace.nodes.iter().any(|node| matches!(
        node.event,
        TraceEvent::FactQueryEntered
    ) && node.locator.declaration == "claimed"
        && node.locator.step == Some(1)
        && node.work.stages.queries_entered == 1));
}

#[test_tag::tag(unit)]
#[test]
fn numeric_trace_shows_literal_operands_and_post_evaluation_output_without_changing_work() {
    let schema = Schema::default();
    let raw = Definition {
        version: 1,
        name: "numeric".into(),
        queries: vec![],
        entries: vec!["entry".into()],
        tasks: vec![Task {
            name: "entry".into(),
            parameters: vec![],
            outputs: vec![],
            methods: vec![Method {
                name: "calculate".into(),
                locals: vec![Type::Int],
                result: vec![],
                steps: vec![Step::Condition(htn_planner::Condition::Calculate {
                    op: htn_planner::NumericOperation::Add,
                    left: E::Literal(Value::Int(-4)),
                    right: E::Literal(Value::Int(6)),
                    output: E::Local(0),
                })],
            }],
        }],
    };
    let mut limits = Limits::small();
    limits.diagnostics = 4096;
    let domain = CompiledDomain::admit("numeric-source", schema.clone(), raw, limits).unwrap();
    let facts = FactSnapshot::new(&schema, vec![]).unwrap();
    let traced = plan_traced(&domain, &facts, "entry", &[], 4096);
    assert_eq!(traced.result, plan(&domain, &facts, "entry", &[]));
    let entered = traced
        .trace
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.event,
                TraceEvent::ControlEntered(htn_planner::TraceControl::Calculate)
            )
        })
        .unwrap();
    assert_eq!(
        entered.arguments,
        htn_planner::TraceArguments::Slots(vec![Some(Value::Int(-4)), Some(Value::Int(6)), None])
    );
    assert_eq!(entered.work.stages.conditions_entered, 1);
    assert!(
        traced
            .trace
            .nodes
            .iter()
            .any(|node| node.parent == Some(entered.id)
                && matches!(node.event, TraceEvent::Witness)
                && node.arguments
                    == htn_planner::TraceArguments::Slots(vec![
                        Some(Value::Int(-4)),
                        Some(Value::Int(6)),
                        Some(Value::Int(2))
                    ]))
    );
}

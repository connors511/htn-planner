use htn_planner::{
    CompiledDomain, Definition, FactSnapshot, Limits, Method, Schema, Signature, Step, Task,
    TraceEvent, Value, plan, plan_traced,
};
fn domain() -> Result<CompiledDomain, htn_planner::Error> {
    let schema = Schema {
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![],
        }],
        ..Schema::default()
    };
    let raw = Definition {
        version: 1,
        name: "trace".into(),
        queries: vec![],
        entries: vec!["entry".into()],
        tasks: vec![Task {
            name: "entry".into(),
            parameters: vec![],
            outputs: vec![],
            methods: vec![Method {
                name: "selected".into(),
                locals: vec![],
                result: vec![],
                steps: vec![Step::Action {
                    name: "emit".into(),
                    arguments: vec![],
                }],
            }],
        }],
    };
    let mut limits = Limits::small();
    limits.diagnostics = 4096;
    CompiledDomain::admit("source:trace", schema, raw, limits)
}
#[test_tag::tag(unit)]
#[test]
fn consumer_gets_source_linked_trace_without_changing_complete_result_or_work() {
    let domain = domain().unwrap();
    let facts = FactSnapshot::new(domain.schema(), vec![]).unwrap();
    let plain = plan(&domain, &facts, "entry", &[]);
    let traced = plan_traced(&domain, &facts, "entry", &[], 4096);
    assert_eq!(traced.result, plain);
    assert!(!traced.trace.truncated);
    assert!(
        traced
            .trace
            .nodes
            .iter()
            .any(|node| matches!(node.event, TraceEvent::MethodEntered)
                && node.locator.task.as_deref() == Some("entry")
                && node.locator.declaration == "selected")
    );
    for node in &traced.trace.nodes {
        if let Some(parent) = node.parent {
            assert!(
                traced
                    .trace
                    .nodes
                    .iter()
                    .any(|earlier| earlier.id == parent && earlier.id < node.id)
            );
        }
    }
    assert!(traced.trace.retained_units <= traced.trace.capacity);
}

#[test_tag::tag(invariant)]
#[test]
fn retention_capacity_changes_only_diagnostics_and_fixed_final_metadata_survives_zero() {
    let domain = domain().unwrap();
    let facts = FactSnapshot::new(domain.schema(), vec![]).unwrap();
    let plain = plan(&domain, &facts, "entry", &[]);
    for capacity in [0, 1, 64, 4096, usize::MAX] {
        let traced = plan_traced(&domain, &facts, "entry", &[], capacity);
        assert_eq!(traced.result, plain);
        assert_eq!(traced.trace.final_work, plain.work);
        assert_eq!(
            traced.trace.final_outcome,
            htn_planner::TraceOutcome::Success
        );
        assert_eq!(
            traced.trace.capacity,
            capacity.min(domain.limits().diagnostics)
        );
        assert!(traced.trace.retained_units <= traced.trace.capacity);
        if capacity == 0 {
            assert!(traced.trace.nodes.is_empty());
            assert!(traced.trace.truncated);
            assert!(traced.trace.dropped_emissions > 0);
        }
        if capacity == 4096 {
            assert!(traced.trace.nodes.iter().any(|node| matches!(
                node.event,
                TraceEvent::Finished(htn_planner::TraceOutcome::Success)
            )));
        }
    }
}

#[test_tag::tag(invariant)]
#[test]
fn malformed_input_is_not_traversed_or_echoed_for_optional_capture() {
    let domain = domain().unwrap();
    let facts = FactSnapshot::new(domain.schema(), vec![]).unwrap();
    let oversized = "sensitive".repeat(20_000);
    for (entry, arguments) in [
        (oversized.as_str(), vec![]),
        ("entry", vec![Value::Symbol(oversized.clone())]),
    ] {
        let plain = plan(&domain, &facts, entry, &arguments);
        for cap in [0, 1, 4096] {
            let traced = plan_traced(&domain, &facts, entry, &arguments, cap);
            assert_eq!(traced.result, plain);
            assert!(traced.trace.nodes.iter().all(|node| matches!(
                node.arguments,
                htn_planner::TraceArguments::UnvalidatedOmitted
                    | htn_planner::TraceArguments::NotApplicable
            )));
            assert!(
                traced
                    .trace
                    .nodes
                    .iter()
                    .all(|node| node.locator.source == "source:trace"
                        && node.locator.declaration == "trace")
            );
        }
    }
}

#[test_tag::tag(unit)]
#[test]
fn deterministic_stage_entry_counters_are_present_even_without_tracing() {
    let domain = domain().unwrap();
    let facts = FactSnapshot::new(domain.schema(), vec![]).unwrap();
    let result = plan(&domain, &facts, "entry", &[]);
    assert_eq!(result.work.stages.tasks_entered, 1);
    assert_eq!(result.work.stages.methods_entered, 1);
    assert_eq!(result.work.stages.queries_entered, 0);
    assert!(!result.work.stages.saturated);
    assert_eq!(plan_traced(&domain, &facts, "entry", &[], 0).result, result);
}

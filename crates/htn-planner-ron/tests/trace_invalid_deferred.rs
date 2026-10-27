use htn_planner::{
    CompiledDomain, DeferredTask, Definition, Expression, Fact, FactSnapshot, Limits, Locator,
    Method, Outcome, PlanEntry, Schema, Signature, Step, Task, Trace, TraceArguments, TraceEvent,
    Type, Value, expand_deferred, expand_deferred_traced, plan,
};
#[derive(serde::Serialize)]
struct TokenFields<'a> {
    definition: htn_planner::DefinitionIdentity,
    semantics: &'a str,
    task: &'a str,
    arguments: &'a [Value],
    locator: &'a Locator,
}
fn fixture() -> Result<(CompiledDomain, FactSnapshot, DeferredTask), String> {
    let schema = Schema {
        predicates: vec![Signature {
            name: "ready".into(),
            arguments: vec![Type::Symbol],
        }],
        ..Schema::default()
    };
    let raw = Definition {
        version: 1,
        name: "deferred".into(),
        queries: vec![],
        entries: vec!["entry".into()],
        tasks: vec![
            Task {
                name: "entry".into(),
                parameters: vec![Type::Symbol],
                outputs: vec![],
                methods: vec![Method {
                    name: "capture".into(),
                    locals: vec![],
                    result: vec![],
                    steps: vec![Step::Defer {
                        name: "later".into(),
                        arguments: vec![Expression::Parameter(0)],
                    }],
                }],
            },
            Task {
                name: "later".into(),
                parameters: vec![Type::Symbol],
                outputs: vec![],
                methods: vec![Method {
                    name: "check".into(),
                    locals: vec![],
                    result: vec![],
                    steps: vec![Step::Require {
                        name: "ready".into(),
                        arguments: vec![Expression::Parameter(0)],
                    }],
                }],
            },
        ],
    };
    let mut limits = Limits::small();
    limits.diagnostics = 4096;
    let domain = CompiledDomain::admit("trusted-source", schema.clone(), raw, limits)
        .map_err(|e| e.message)?;
    let facts = FactSnapshot::new(
        &schema,
        vec![Fact {
            predicate: "ready".into(),
            arguments: vec![Value::Symbol("actor".into())],
        }],
    )?;
    let Outcome::Success(mut entries) =
        plan(&domain, &facts, "entry", &[Value::Symbol("actor".into())]).outcome
    else {
        return Err("capture failed".into());
    };
    let Some(PlanEntry::Deferred(token)) = entries.pop() else {
        return Err("missing deferred token".into());
    };
    Ok((domain, facts, token))
}
#[test_tag::tag(invariant)]
#[test]
fn malformed_deserialized_tokens_never_echo_unvalidated_payload_or_change_expansion() {
    let (domain, facts, token) = fixture().unwrap();
    let oversized = "sensitive-payload".repeat(10_000);
    let locator = Locator {
        source: oversized.clone(),
        task: Some(oversized.clone()),
        declaration: oversized.clone(),
        step: Some(usize::MAX),
    };
    let huge = [Value::Symbol(oversized.clone())];
    let invalid = [Value::Bool(true)];
    for (semantics, task, arguments) in [
        (oversized.as_str(), token.task(), huge.as_slice()),
        (token.semantics(), oversized.as_str(), huge.as_slice()),
        (token.semantics(), token.task(), invalid.as_slice()),
        (token.semantics(), token.task(), huge.as_slice()),
        (token.semantics(), token.task(), token.arguments()),
    ] {
        let encoded = ron::ser::to_string(&TokenFields {
            definition: token.definition_identity(),
            semantics,
            task,
            arguments,
            locator: &locator,
        })
        .unwrap();
        let malformed: DeferredTask = ron::from_str(&encoded).unwrap();
        let plain = expand_deferred(&domain, &facts, &malformed);
        for cap in [0, 1, 32, 4096] {
            let traced = expand_deferred_traced(&domain, &facts, &malformed, cap);
            assert_eq!(traced.result, plain);
            assert!(traced.trace.retained_units <= traced.trace.capacity);
            for node in &traced.trace.nodes {
                assert_eq!(node.locator.source, "trusted-source");
                assert!(node.locator.declaration.len() < 1024);
                if let TraceArguments::Slots(values) = &node.arguments {
                    assert!(
                        values
                            .iter()
                            .flatten()
                            .all(|value| matches!(value,Value::Symbol(name) if name=="actor"))
                    );
                }
            }
        }
    }
}
#[test_tag::tag(unit)]
#[test]
fn deferred_trace_is_fresh_source_linked_and_inspection_serde_roundtrips() {
    let (domain, facts, token) = fixture().unwrap();
    let captured = expand_deferred_traced(&domain, &facts, &token, 4096);
    assert_eq!(captured.result, expand_deferred(&domain, &facts, &token));
    assert!(!captured.trace.truncated);
    assert!(captured.trace.nodes.iter().any(|node| node.parent.is_none()
        && matches!(node.event, TraceEvent::Attempt { deferred: true })));
    for node in &captured.trace.nodes {
        if let Some(parent) = node.parent {
            assert!(
                captured
                    .trace
                    .nodes
                    .iter()
                    .any(|ancestor| ancestor.id == parent && ancestor.id < node.id)
            );
        }
    }
    let text = ron::ser::to_string(&captured.trace).unwrap();
    let decoded: Trace = ron::from_str(&text).unwrap();
    assert_eq!(decoded, captured.trace);
    let absent = FactSnapshot::new(domain.schema(), vec![]).unwrap();
    for cap in [0, 1, 4096] {
        let traced = expand_deferred_traced(&domain, &absent, &token, cap);
        assert_eq!(traced.result, expand_deferred(&domain, &absent, &token));
        assert_eq!(traced.result.outcome, Outcome::NoPlan);
    }
}

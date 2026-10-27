//! Exact query-site focus can bypass earlier emissions inside the same task.
use super::*;

#[test_tag::tag(invariant)]
#[test]
fn late_query_focus_retains_actual_rows_and_witness_after_same_task_trace_clips()
-> Result<(), String> {
    let (original, facts) = fixture()?;
    let mut definition = original.definition().clone();
    let chosen = definition
        .tasks
        .iter_mut()
        .find(|task| task.name == "chosen")
        .ok_or("chosen task")?;
    let mut steps: Vec<_> = (0..64)
        .map(|_| Step::Require {
            name: "gate".into(),
            arguments: vec![],
        })
        .collect();
    steps.append(&mut chosen.methods[0].steps);
    chosen.methods[0].steps = steps;
    let domain = CompiledDomain::admit(
        "focus-source",
        original.schema().clone(),
        definition,
        original.limits(),
    )
    .map_err(|error| error.message)?;
    let provider = rows();
    let ordinary = plan_with_queries(&domain, &facts, &provider, "entry", &[]);
    let task = plan_traced_with_queries_focused(
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
    assert_eq!(task.result, ordinary);
    assert!(task.trace.truncated);
    assert!(
        task.trace
            .nodes
            .iter()
            .all(|node| node.locator.declaration != "candidate")
    );
    let query = plan_traced_with_queries_focused(
        &domain,
        &facts,
        &provider,
        "entry",
        &[],
        4096,
        TraceFocus {
            task: "chosen",
            declaration: Some("candidate"),
        },
    );
    assert_eq!(query.result, ordinary);
    assert!(!query.trace.truncated);
    assert_eq!(query.trace.focus_declaration.as_deref(), Some("candidate"));
    parents_valid(&query.trace);
    assert!(
        query
            .trace
            .nodes
            .iter()
            .all(|node| node.locator.declaration == "candidate")
    );
    assert!(
        query
            .trace
            .nodes
            .iter()
            .any(|node| node.event == TraceEvent::FactQueryEntered)
    );
    assert!(query.trace.nodes.iter().any(|node| node.event == TraceEvent::QueryCandidate
        && matches!(&node.arguments, TraceArguments::Slots(values) if values == &vec![Some(Value::Int(9))])));
    assert!(query.trace.nodes.iter().any(|node| node.event == TraceEvent::Witness
        && matches!(&node.arguments, TraceArguments::Slots(values) if values == &vec![Some(Value::Int(9))])));
    assert!(
        query.trace.nodes.iter().any(|node| node.parent.is_some()),
        "inherit must follow an actual retained query parent"
    );
    assert!(query.trace.nodes.iter().all(|node| !matches!(
        node.event,
        TraceEvent::TaskEntered | TraceEvent::HostObservation
    )));
    Ok(())
}

#[test_tag::tag(invariant)]
#[test]
fn declaration_focus_zero_cap_unknown_and_oversized_are_bounded_and_work_neutral()
-> Result<(), String> {
    let (domain, facts) = fixture()?;
    let provider = rows();
    let ordinary = plan_with_queries(&domain, &facts, &provider, "entry", &[]);
    let zero = plan_traced_with_queries_focused(
        &domain,
        &facts,
        &provider,
        "entry",
        &[],
        0,
        TraceFocus {
            task: "chosen",
            declaration: Some("candidate"),
        },
    );
    assert_eq!(zero.result, ordinary);
    assert!(zero.trace.nodes.is_empty() && zero.trace.truncated);
    for declaration in ["unknown".to_owned(), "x".repeat(1024), "x".repeat(1025)] {
        let focus = plan_traced_with_queries_focused(
            &domain,
            &facts,
            &provider,
            "entry",
            &[],
            4096,
            TraceFocus {
                task: "chosen",
                declaration: Some(&declaration),
            },
        );
        assert_eq!(focus.result, ordinary);
        assert!(focus.trace.nodes.is_empty() && !focus.trace.truncated);
        assert_eq!(focus.trace.focus_task.as_deref(), Some("chosen"));
        assert_eq!(
            focus.trace.focus_declaration.as_deref(),
            Some(if declaration.len() > 1024 {
                "<oversized-task-focus-omitted>"
            } else {
                &declaration
            })
        );
        let heap = focus.trace.heap_footprint();
        let task = focus.trace.focus_task.as_ref().unwrap();
        let declaration = focus.trace.focus_declaration.as_ref().unwrap();
        assert_eq!(heap.clone_bytes, task.len() + declaration.len());
        assert_eq!(
            heap.capacity_bytes,
            task.capacity() + declaration.capacity()
        );
    }
    Ok(())
}

#[test_tag::tag(invariant)]
#[test]
fn observation_preflight_allows_a_static_code_different_from_the_host_predicate()
-> Result<(), String> {
    let (domain, facts) = fixture()?;
    let provider = rows();
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
            declaration: Some("batch_values"),
        },
    );
    assert_eq!(focused.result, ordinary);
    assert!(
        provider.observing.get(),
        "the actual later static code remains possible"
    );
    assert!(!focused.trace.truncated);
    assert_eq!(focused.trace.nodes.len(), 1);
    let node = &focused.trace.nodes[0];
    assert_eq!(node.event, TraceEvent::HostObservation);
    assert_eq!(node.locator.declaration, "batch_values");
    assert_eq!(node.locator.task.as_deref(), Some("chosen"));
    assert!(
        node.parent.is_none(),
        "the excluded query parent must not be invented"
    );
    assert!(matches!(&node.arguments, TraceArguments::Slots(values)
        if values == &vec![Some(Value::Int(7)), Some(Value::Int(9))]));
    let unknown = plan_traced_with_queries_focused(
        &domain,
        &facts,
        &provider,
        "entry",
        &[],
        4096,
        TraceFocus {
            task: "chosen",
            declaration: Some("unknown_code"),
        },
    );
    assert_eq!(unknown.result, ordinary);
    assert!(unknown.trace.nodes.is_empty() && !unknown.trace.truncated);
    assert_eq!(
        provider.calls.get(),
        3,
        "focus never causes another host query"
    );
    Ok(())
}

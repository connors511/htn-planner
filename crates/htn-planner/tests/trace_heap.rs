use htn_planner::{
    CompiledDomain, Definition, HeapFootprint, Limits, Locator, Schema, Task, Trace,
    TraceArguments, TraceEvent, TraceNode, TraceOutcome, Value, WorkReport,
};
#[test_tag::tag(unit)]
#[test]
fn trace_owned_payload_counts_spare_capacity_and_nested_values_for_host_history() {
    let domain = CompiledDomain::admit(
        "heap",
        Schema::default(),
        Definition {
            version: 1,
            name: "heap".into(),
            queries: vec![],
            entries: vec!["entry".into()],
            tasks: vec![Task {
                name: "entry".into(),
                parameters: vec![],
                outputs: vec![],
                methods: vec![],
            }],
        },
        Limits::small(),
    )
    .unwrap();
    let mut source = String::with_capacity(64);
    source.push_str("source");
    let mut declaration = String::with_capacity(32);
    declaration.push_str("query");
    let mut symbol = String::with_capacity(128);
    symbol.push_str("payload");
    let mut fields = Vec::with_capacity(4);
    fields.push(Value::Symbol(symbol));
    let name = "record".to_owned();
    let mut slots = Vec::with_capacity(3);
    slots.push(Some(Value::Record {
        type_name: name,
        fields,
    }));
    let mut nodes = Vec::with_capacity(4);
    nodes.push(TraceNode {
        id: 0,
        parent: None,
        event: TraceEvent::Witness,
        locator: Locator {
            source,
            declaration,
            task: None,
            step: None,
        },
        arguments: TraceArguments::Slots(slots),
        work: WorkReport::default(),
    });
    let trace = Trace {
        domain: domain.identity(),
        focus_task: None,
        focus_declaration: None,
        capacity: 4096,
        retained_units: 1,
        nodes,
        truncated: false,
        dropped_emissions: 0,
        dropped_saturated: false,
        final_outcome: TraceOutcome::Success,
        final_work: WorkReport::default(),
        work_failure: None,
        provider_work: htn_planner::TraceProviderWork::default(),
    };
    let node = &trace.nodes[0];
    let TraceArguments::Slots(slots) = &node.arguments else {
        unreachable!()
    };
    let Some(Value::Record { type_name, fields }) = &slots[0] else {
        unreachable!()
    };
    let Value::Symbol(symbol) = &fields[0] else {
        unreachable!()
    };
    // Only these backing allocations are owned by this deliberately explicit graph.
    let expected = HeapFootprint {
        capacity_bytes: trace.nodes.capacity() * size_of::<TraceNode>()
            + node.locator.source.capacity()
            + node.locator.declaration.capacity()
            + slots.capacity() * size_of::<Option<Value>>()
            + type_name.capacity()
            + fields.capacity() * size_of::<Value>()
            + symbol.capacity(),
        clone_bytes: trace.nodes.len() * size_of::<TraceNode>()
            + node.locator.source.len()
            + node.locator.declaration.len()
            + slots.len() * size_of::<Option<Value>>()
            + type_name.len()
            + fields.len() * size_of::<Value>()
            + symbol.len(),
    };
    assert_eq!(trace.heap_footprint(), expected);
    assert!(expected.capacity_bytes > expected.clone_bytes);
    assert_eq!(
        trace.clone().heap_footprint().capacity_bytes,
        expected.clone_bytes
    );
}

use htn_planner::{Definition, HeapFootprint, Task};

#[test_tag::tag(unit)]
#[test]
#[expect(
    clippy::redundant_clone,
    reason = "Clone allocation payload is the contract under test"
)]
fn raw_definition_accounts_spare_string_and_vector_capacity_separately_from_clone_payload() {
    let mut name = String::with_capacity(64);
    name.push_str("warehouse");
    let mut entry = String::with_capacity(32);
    entry.push_str("ready");
    let mut entries = Vec::with_capacity(4);
    entries.push(entry);
    let definition = Definition {
        queries: vec![],
        version: 1,
        name,
        entries,
        tasks: Vec::with_capacity(2),
    };
    // These are the owned backing allocations in this deliberately shallow input;
    // inline Definition/String/Vec headers and allocator metadata are excluded.
    let expected = HeapFootprint {
        capacity_bytes: definition.name.capacity()
            + definition.entries.capacity() * size_of::<String>()
            + definition.entries[0].capacity()
            + definition.tasks.capacity() * size_of::<Task>(),
        clone_bytes: definition.name.len()
            + definition.entries.len() * size_of::<String>()
            + definition.entries[0].len(),
    };
    assert_eq!(definition.heap_footprint(), expected);
    assert!(expected.capacity_bytes > expected.clone_bytes);
    assert_eq!(
        definition.clone().heap_footprint().capacity_bytes,
        expected.clone_bytes
    );
}

#[test_tag::tag(unit)]
#[test]
fn nested_plan_arguments_and_locators_follow_current_clone_payload() {
    use htn_planner::{Action, Locator, PlanEntry, Value};
    let mut arguments = Vec::with_capacity(8);
    arguments.push(Value::Record {
        type_name: "Slot".into(),
        fields: vec![Value::Symbol("bay".into())],
    });
    let mut entry = PlanEntry::Action(Action {
        name: "dispatch".into(),
        arguments,
        locator: Locator {
            task: Some("ship".into()),
            source: "warehouse.ron".into(),
            declaration: "dispatch".into(),
            step: Some(0),
        },
    });
    let before = entry.heap_footprint();
    assert_eq!(
        entry.clone().heap_footprint().capacity_bytes,
        before.clone_bytes
    );
    let PlanEntry::Action(action) = &mut entry else {
        unreachable!()
    };
    let Value::Record { fields, .. } = &mut action.arguments[0] else {
        unreachable!()
    };
    let Value::Symbol(symbol) = &mut fields[0] else {
        unreachable!()
    };
    let extra = "-loading-dock";
    symbol.push_str(extra);
    assert_eq!(
        entry.heap_footprint().clone_bytes,
        before.clone_bytes + extra.len()
    );
}

#[test_tag::tag(invariant)]
#[test]
fn unadmitted_boxed_conditions_are_counted_iteratively_including_pointee_storage() {
    use htn_planner::{Condition, DerivedQuery};
    fn definition(condition: Condition) -> Definition {
        Definition {
            queries: vec![DerivedQuery {
                name: "available".into(),
                parameters: vec![],
                outputs: vec![],
                locals: vec![],
                condition,
                result: vec![],
            }],
            version: 1,
            name: "warehouse".into(),
            entries: vec![],
            tasks: vec![],
        }
    }
    let baseline = definition(Condition::And(vec![])).heap_footprint();
    let depth = 1024; // Deliberately exceeds admitted depth; the footprint API also accepts raw IR.
    let mut condition = Condition::And(vec![]);
    for _ in 0..depth {
        condition = Condition::Not(Box::new(condition));
    }
    let footprint = definition(condition).heap_footprint();
    let boxed_bytes = depth * size_of::<Condition>();
    assert_eq!(
        footprint.capacity_bytes,
        baseline.capacity_bytes + boxed_bytes
    );
    assert_eq!(footprint.clone_bytes, baseline.clone_bytes + boxed_bytes);
}

fn domain_with_effect(payload: &str) -> Result<htn_planner::CompiledDomain, htn_planner::Error> {
    use htn_planner::{
        CompiledDomain, EffectOperation, Expression, Limits, Method, PlanningEffect, Schema,
        Signature, Step, Type, Value,
    };
    let schema = Schema {
        predicates: vec![Signature {
            name: "reserved".into(),
            arguments: vec![Type::Symbol],
        }],
        effects: vec![PlanningEffect {
            name: "claim".into(),
            parameters: vec![],
            operations: vec![EffectOperation::Assert {
                predicate: "reserved".into(),
                arguments: vec![Expression::Literal(Value::Symbol(payload.into()))],
            }],
        }],
        ..Schema::default()
    };
    let definition = Definition {
        queries: vec![],
        version: 1,
        name: "warehouse".into(),
        entries: vec!["ship".into()],
        tasks: vec![
            Task {
                name: "ship".into(),
                parameters: vec![],
                outputs: vec![],
                methods: vec![Method {
                    name: "ship".into(),
                    locals: vec![],
                    result: vec![],
                    steps: vec![
                        Step::Effect {
                            name: "claim".into(),
                            arguments: vec![],
                        },
                        Step::Defer {
                            name: "later".into(),
                            arguments: vec![],
                        },
                    ],
                }],
            },
            Task {
                name: "later".into(),
                parameters: vec![],
                outputs: vec![],
                methods: vec![Method {
                    name: "finish".into(),
                    locals: vec![],
                    result: vec![],
                    steps: vec![],
                }],
            },
        ],
    };
    CompiledDomain::admit("warehouse.ron", schema, definition, Limits::small())
}

#[test_tag::tag(unit)]
#[test]
#[expect(
    clippy::redundant_clone,
    reason = "Clone allocation payload is the contract under test"
)]
fn compiled_domain_counts_raw_and_compiled_effect_payloads_and_deferred_entries() {
    use htn_planner::{FactSnapshot, Outcome, PlanEntry, plan};
    let small = domain_with_effect("bay").unwrap();
    let larger_text = "bay-loading-dock";
    let larger = domain_with_effect(larger_text).unwrap();
    let small_bytes = small.heap_footprint();
    let large_bytes = larger.heap_footprint();
    // One literal is retained in raw schema and a separate one in the owned compiled effect.
    assert_eq!(
        large_bytes.clone_bytes - small_bytes.clone_bytes,
        2 * (larger_text.len() - "bay".len())
    );
    let cloned_small = small.clone().heap_footprint();
    let cloned_large = larger.clone().heap_footprint();
    // Domains retain one shared admitted-schema copy in addition to the cold
    // schema and compiled effect. Clone allocates only the latter two payloads.
    assert_eq!(cloned_large.clone_bytes, large_bytes.clone_bytes);
    assert!(cloned_large.capacity_bytes > cloned_large.clone_bytes);
    assert_eq!(
        cloned_large.capacity_bytes - cloned_small.capacity_bytes,
        3 * (larger_text.len() - "bay".len())
    );
    assert!(large_bytes.clone_bytes > larger.definition().heap_footprint().clone_bytes);
    let facts = FactSnapshot::new(larger.schema(), vec![]).unwrap();
    let Outcome::Success(entries) = plan(&larger, &facts, "ship", &[]).outcome else {
        panic!("complete deferred plan")
    };
    assert!(matches!(entries[0], PlanEntry::Deferred(_)));
    let deferred = entries[0].heap_footprint();
    assert!(deferred.clone_bytes > 0);
    assert_eq!(
        entries[0].clone().heap_footprint().capacity_bytes,
        deferred.clone_bytes
    );
}

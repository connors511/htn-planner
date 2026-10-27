use htn_planner::{FactSnapshot, Limits, Outcome, Schema, Signature, Type, Value, plan};

#[test_tag::tag(unit)]
#[test]
fn ordered_fallback_returns_complete_sequence_without_mutating_facts() {
    let schema = Schema {
        predicates: vec![Signature {
            name: "available".into(),
            arguments: vec![Type::Int],
        }],
        actions: vec![Signature {
            name: "move".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    };
    let source = r#"(version:1,name:"warehouse",entries:["deliver"],tasks:[
        (name:"deliver",parameters:[Int],methods:[
            (name:"preferred",steps:[Require(name:"available",arguments:[Parameter(0)]),Action(name:"move",arguments:[Parameter(0)])]),
            (name:"fallback",steps:[Action(name:"move",arguments:[Literal(Int(2))]),Action(name:"move",arguments:[Literal(Int(3))])])
        ])])"#;
    let domain =
        htn_planner_ron::compile("fixture", source, schema.clone(), Limits::small()).unwrap();
    let facts = FactSnapshot::new(&schema, vec![]).unwrap();
    let before = facts.clone();
    let result = plan(&domain, &facts, "deliver", &[Value::Int(9)]);
    let Outcome::Success(actions) = &result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(
        actions
            .iter()
            .map(|a| a.as_action().unwrap().arguments.clone())
            .collect::<Vec<_>>(),
        vec![vec![Value::Int(2)], vec![Value::Int(3)]]
    );
    assert_eq!(facts, before);
    assert_eq!(result, plan(&domain, &facts, "deliver", &[Value::Int(9)]));
}

fn schema() -> Schema {
    Schema {
        actions: vec![Signature {
            name: "move".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    }
}
const ONE_ACTION: &str = r#"(version:1,name:"warehouse",entries:["deliver"],tasks:[
    (name:"deliver",parameters:[],methods:[(name:"direct",steps:[Action(name:"move",arguments:[Literal(Int(7))])])])])"#;

#[test_tag::tag(unit)]
#[test]
fn work_allowance_distinguishes_exact_completion_from_exhaustion() {
    let facts = FactSnapshot::new(&schema(), vec![]).unwrap();
    let complete = htn_planner_ron::compile("work", ONE_ACTION, schema(), Limits::small()).unwrap();
    let expected = plan(&complete, &facts, "deliver", &[]);
    assert!(matches!(expected.outcome, Outcome::Success(_)));
    let mut limits = Limits::small();
    limits.work = expected.work.consumed;
    let exact = htn_planner_ron::compile("work", ONE_ACTION, schema(), limits).unwrap();
    assert_eq!(plan(&exact, &facts, "deliver", &[]), expected);
    limits.work -= 1;
    let short = htn_planner_ron::compile("work", ONE_ACTION, schema(), limits).unwrap();
    assert_eq!(
        plan(&short, &facts, "deliver", &[]).outcome,
        Outcome::BudgetExhausted
    );
}

#[test_tag::tag(unit)]
#[test]
fn capacity_failure_returns_no_prefix_and_next_attempt_is_isolated() {
    let facts = FactSnapshot::new(&schema(), vec![]).unwrap();
    let mut limits = Limits::small();
    limits.storage = 1;
    let small = htn_planner_ron::compile("storage", ONE_ACTION, schema(), limits).unwrap();
    assert_eq!(
        plan(&small, &facts, "deliver", &[]).outcome,
        Outcome::CapacityExceeded
    );
    let reusable =
        htn_planner_ron::compile("storage", ONE_ACTION, schema(), Limits::small()).unwrap();
    assert!(matches!(
        plan(&reusable, &facts, "deliver", &[]).outcome,
        Outcome::Success(_)
    ));
}

#[test_tag::tag(unit)]
#[test]
fn empty_method_is_success_while_absent_methods_have_no_plan() {
    let facts = FactSnapshot::new(&Schema::default(), vec![]).unwrap();
    for (methods, expected) in [
        ("[(name:\"empty\",steps:[])]", Outcome::Success(vec![])),
        ("[]", Outcome::NoPlan),
    ] {
        let source = format!(
            "(version:1,name:\"empty\",entries:[\"entry\"],tasks:[(name:\"entry\",parameters:[],methods:{methods})])"
        );
        let domain =
            htn_planner_ron::compile("empty", &source, Schema::default(), Limits::small()).unwrap();
        assert_eq!(plan(&domain, &facts, "entry", &[]).outcome, expected);
    }
}

#[test_tag::tag(unit)]
#[test]
fn admission_rejects_unknown_actions_and_bad_arguments_with_source() {
    for step in [
        "Action(name:\"missing\",arguments:[])",
        "Action(name:\"move\",arguments:[])",
        "Action(name:\"move\",arguments:[Literal(Bool(true))])",
        "Action(name:\"move\",arguments:[Parameter(0)])",
    ] {
        let source = format!(
            "(version:1,name:\"invalid\",entries:[\"entry\"],tasks:[(name:\"entry\",parameters:[],methods:[(name:\"bad\",steps:[{step}])])])"
        );
        let error =
            htn_planner_ron::compile("bad-source", &source, schema(), Limits::small()).unwrap_err();
        assert_eq!(error.locator.source, "bad-source");
        assert_eq!(error.locator.declaration, "bad");
        assert_eq!(error.locator.step, Some(0));
    }
}

#[test_tag::tag(unit)]
#[test]
fn wrong_runtime_schema_or_entry_arguments_are_invalid_input() {
    let domain = htn_planner_ron::compile("input", ONE_ACTION, schema(), Limits::small()).unwrap();
    let facts = FactSnapshot::new(&schema(), vec![]).unwrap();
    assert_eq!(
        plan(&domain, &facts, "deliver", &[Value::Int(1)]).outcome,
        Outcome::InvalidInput
    );
    assert_eq!(
        plan(&domain, &facts, "missing", &[]).outcome,
        Outcome::InvalidInput
    );
    let different = FactSnapshot::new(&Schema::default(), vec![]).unwrap();
    assert_eq!(
        plan(&domain, &different, "deliver", &[]).outcome,
        Outcome::InvalidInput
    );
}

#[test_tag::tag(unit)]
#[test]
fn failed_nested_choices_restore_tentative_actions_before_fallback() {
    let schema = Schema {
        predicates: vec![Signature {
            name: "allowed".into(),
            arguments: vec![Type::Int],
        }],
        ..schema()
    };
    let source = r#"(version:1,name:"continuation",entries:["entry"],tasks:[
      (name:"entry",parameters:[],methods:[
        (name:"preferred",steps:[Task(name:"choose",arguments:[]),Require(name:"allowed",arguments:[Literal(Int(1))])]),
        (name:"fallback",steps:[Action(name:"move",arguments:[Literal(Int(9))])])]),
      (name:"choose",parameters:[],methods:[
        (name:"first",steps:[Action(name:"move",arguments:[Literal(Int(1))])]),
        (name:"second",steps:[Action(name:"move",arguments:[Literal(Int(2))])])])])"#;
    let domain =
        htn_planner_ron::compile("continuation", source, schema.clone(), Limits::small()).unwrap();
    let facts = FactSnapshot::new(&schema, vec![]).unwrap();
    let result = plan(&domain, &facts, "entry", &[]);
    let Outcome::Success(actions) = result.outcome else {
        panic!("expected fallback")
    };
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].as_action().unwrap().arguments, &[Value::Int(9)]);
}

#[test_tag::tag(unit)]
#[test]
fn method_choice_capacity_includes_the_current_large_value_copy() {
    let schema = Schema::default();
    let definition = htn_planner::Definition {
        queries: vec![],
        version: 1,
        name: "symbols".into(),
        entries: vec!["entry".into()],
        tasks: vec![htn_planner::Task {
            outputs: vec![],
            name: "entry".into(),
            parameters: vec![Type::Symbol],
            methods: vec![
                htn_planner::Method {
                    locals: vec![],
                    result: vec![],
                    name: "first".into(),
                    steps: vec![],
                },
                htn_planner::Method {
                    locals: vec![],
                    result: vec![],
                    name: "second".into(),
                    steps: vec![],
                },
            ],
        }],
    };
    let facts = FactSnapshot::new(&schema, vec![]).unwrap();
    let mut limits = Limits::small();
    let value = Value::Symbol("x".repeat(200));
    // A single argument fits; retaining a second complete alternative does not.
    limits.storage = 350;
    let domain = htn_planner::CompiledDomain::admit("symbols", schema, definition, limits).unwrap();
    assert_eq!(
        plan(&domain, &facts, "entry", &[value]).outcome,
        Outcome::CapacityExceeded
    );
}

#[test_tag::tag(unit)]
#[test]
fn schema_input_validation_obeys_work_before_comparing_or_rejecting_input() {
    let mut limits = Limits::small();
    limits.work = 1;
    let domain = htn_planner_ron::compile("input-work", ONE_ACTION, schema(), limits).unwrap();
    let incompatible = FactSnapshot::new(&Schema::default(), vec![]).unwrap();
    let result = plan(&domain, &incompatible, "deliver", &[]);
    assert_eq!(result.outcome, Outcome::BudgetExhausted);
    assert_eq!(result.work.consumed, limits.work);
}

#[test_tag::tag(unit)]
#[test]
fn typed_records_references_and_fixed_scale_survive_planning() {
    use htn_planner::{CompiledDomain, Definition, Expression, Method, RecordType, Step, Task};
    let types = vec![Type::Ref("worker".into()), Type::Record("bin".into())];
    let schema = Schema {
        references: vec!["worker".into()],
        records: vec![RecordType {
            name: "bin".into(),
            fields: vec![Type::Fixed(2), Type::Bool],
        }],
        actions: vec![Signature {
            name: "assign".into(),
            arguments: types.clone(),
        }],
        ..Schema::default()
    };
    let definition = Definition {
        queries: vec![],
        version: 1,
        name: "typed".into(),
        entries: vec!["entry".into()],
        tasks: vec![Task {
            outputs: vec![],
            name: "entry".into(),
            parameters: types,
            methods: vec![Method {
                locals: vec![],
                result: vec![],
                name: "assign".into(),
                steps: vec![Step::Action {
                    name: "assign".into(),
                    arguments: vec![Expression::Parameter(0), Expression::Parameter(1)],
                }],
            }],
        }],
    };
    let domain =
        CompiledDomain::admit("typed", schema.clone(), definition, Limits::small()).unwrap();
    let facts = FactSnapshot::new(&schema, vec![]).unwrap();
    let args = vec![
        Value::Ref {
            type_name: "worker".into(),
            key: 17,
        },
        Value::Record {
            type_name: "bin".into(),
            fields: vec![Value::Fixed { raw: 235, scale: 2 }, Value::Bool(true)],
        },
    ];
    let Outcome::Success(actions) = plan(&domain, &facts, "entry", &args).outcome else {
        panic!("expected typed action")
    };
    assert_eq!(actions[0].as_action().unwrap().arguments, args);
    let wrong_scale = vec![
        args[0].clone(),
        Value::Record {
            type_name: "bin".into(),
            fields: vec![Value::Fixed { raw: 235, scale: 3 }, Value::Bool(true)],
        },
    ];
    assert_eq!(
        plan(&domain, &facts, "entry", &wrong_scale).outcome,
        Outcome::InvalidInput
    );
}

#[test_tag::tag(unit)]
#[test]
fn unsupported_author_fields_are_rejected() {
    let source = r#"(version:1,name:"unknown",entries:["entry"],tasks:[(name:"entry",parameters:[],methods:[])],script:"arbitrary")"#;
    assert!(
        htn_planner_ron::compile("unknown", source, Schema::default(), Limits::small()).is_err()
    );
}

#[test_tag::tag(unit)]
#[test]
fn cyclic_tasks_and_invalid_limits_are_rejected_before_planning() {
    let source = r#"(version:1,name:"cycle",entries:["entry"],tasks:[(name:"entry",parameters:[],methods:[(name:"recurse",steps:[Task(name:"entry",arguments:[])])])])"#;
    let error =
        htn_planner_ron::compile("cycle", source, Schema::default(), Limits::small()).unwrap_err();
    assert!(error.message.contains("recursive"));
    let mut limits = Limits::small();
    limits.work = 0;
    assert!(htn_planner_ron::compile("limits", ONE_ACTION, schema(), limits).is_err());
}

#[test_tag::tag(unit)]
#[test]
fn nested_empty_task_capacity_includes_child_frame_before_expansion() {
    let source = r#"(version:1,name:"capacity",entries:["entry"],tasks:[
      (name:"entry",parameters:[],methods:[(name:"parent",steps:[Task(name:"child",arguments:[])])]),
      (name:"child",parameters:[],methods:[(name:"empty",steps:[])])])"#;
    let mut limits = Limits::small();
    limits.storage = 2;
    let domain = htn_planner_ron::compile("capacity", source, Schema::default(), limits).unwrap();
    let facts = FactSnapshot::new(&Schema::default(), vec![]).unwrap();
    let result = plan(&domain, &facts, "entry", &[]);
    assert_eq!(result.outcome, Outcome::CapacityExceeded);
    assert!(result.work.peak_storage <= limits.storage);
}

#[test_tag::tag(unit)]
#[test]
fn oversized_source_identity_is_rejected_before_parse_diagnostics() {
    let identity = "s".repeat(1025);
    let error =
        htn_planner_ron::compile(&identity, "invalid RON", Schema::default(), Limits::small())
            .unwrap_err();
    assert!(error.locator.source.len() <= 1024);
    assert!(error.message.contains("capacity"));
}

#[test_tag::tag(unit)]
#[test]
fn large_structured_input_obeys_exact_work_and_storage_boundaries() {
    use htn_planner::{CompiledDomain, Definition, Method, RecordType, Task};
    let schema = Schema {
        records: vec![RecordType {
            name: "payload".into(),
            fields: vec![Type::Symbol],
        }],
        ..Schema::default()
    };
    let definition = Definition {
        queries: vec![],
        version: 1,
        name: "payload".into(),
        entries: vec!["entry".into()],
        tasks: vec![Task {
            outputs: vec![],
            name: "entry".into(),
            parameters: vec![Type::Record("payload".into())],
            methods: vec![Method {
                locals: vec![],
                result: vec![],
                name: "empty".into(),
                steps: vec![],
            }],
        }],
    };
    let arguments = vec![Value::Record {
        type_name: "payload".into(),
        fields: vec![Value::Symbol("large".repeat(80))],
    }];
    let before = arguments.clone();
    let facts = FactSnapshot::new(&schema, vec![]).unwrap();
    let domain = CompiledDomain::admit(
        "payload",
        schema.clone(),
        definition.clone(),
        Limits::small(),
    )
    .unwrap();
    let expected = plan(&domain, &facts, "entry", &arguments);
    assert_eq!(expected.outcome, Outcome::Success(vec![]));
    let mut exact = Limits::small();
    exact.work = expected.work.consumed;
    exact.storage = expected.work.peak_storage;
    let domain =
        CompiledDomain::admit("payload", schema.clone(), definition.clone(), exact).unwrap();
    assert_eq!(plan(&domain, &facts, "entry", &arguments), expected);
    let mut short = exact;
    short.work -= 1;
    let domain =
        CompiledDomain::admit("payload", schema.clone(), definition.clone(), short).unwrap();
    assert_eq!(
        plan(&domain, &facts, "entry", &arguments).outcome,
        Outcome::BudgetExhausted
    );
    short = exact;
    short.storage -= 1;
    let domain = CompiledDomain::admit("payload", schema, definition, short).unwrap();
    assert_eq!(
        plan(&domain, &facts, "entry", &arguments).outcome,
        Outcome::CapacityExceeded
    );
    assert_eq!(arguments, before);
}

#[test_tag::tag(unit)]
#[test]
fn oversized_declaration_diagnostics_do_not_retain_unbounded_names() {
    let name = "n".repeat(1025);
    let definition = htn_planner::Definition {
        queries: vec![],
        version: 1,
        name: "bounded".into(),
        entries: vec![name.clone()],
        tasks: vec![htn_planner::Task {
            outputs: vec![],
            name,
            parameters: vec![],
            methods: vec![],
        }],
    };
    let error = htn_planner::CompiledDomain::admit(
        "bounded",
        Schema::default(),
        definition,
        Limits::small(),
    )
    .unwrap_err();
    assert!(error.locator.declaration.len() <= 1024);
    assert!(error.message.len() <= 1024);
}

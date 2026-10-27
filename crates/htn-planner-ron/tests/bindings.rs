use htn_planner::{Fact, FactSnapshot, Limits, Outcome, Schema, Signature, Type, Value, plan};

fn schema() -> Schema {
    Schema {
        predicates: vec![Signature {
            name: "allowed".into(),
            arguments: vec![Type::Int],
        }],
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    }
}

#[test_tag::tag(unit)]
#[test]
fn later_sibling_reopens_successful_child_method_and_rolls_back_actions() {
    let schema = schema();
    let source = r#"(version:1,name:"returns",entries:["entry"],tasks:[
      (name:"entry",parameters:[],methods:[
        (name:"preferred",locals:[Int],steps:[Call(name:"choose",arguments:[],outputs:[Local(0)]),Require(name:"allowed",arguments:[Local(0)])]),
        (name:"fallback",steps:[Action(name:"emit",arguments:[Literal(Int(99))])])]),
      (name:"choose",parameters:[],outputs:[Int],methods:[
        (name:"first",steps:[Action(name:"emit",arguments:[Literal(Int(1))])],result:[Literal(Int(1))]),
        (name:"second",steps:[Action(name:"emit",arguments:[Literal(Int(2))])],result:[Literal(Int(2))])])])"#;
    let domain =
        htn_planner_ron::compile("returns", source, schema.clone(), Limits::small()).unwrap();
    let facts = FactSnapshot::new(
        &schema,
        vec![Fact {
            predicate: "allowed".into(),
            arguments: vec![Value::Int(2)],
        }],
    )
    .unwrap();
    let before = facts.clone();
    let result = plan(&domain, &facts, "entry", &[]);
    let Outcome::Success(actions) = &result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0].as_action().unwrap().arguments,
        vec![Value::Int(2)]
    );
    assert_eq!(facts, before);
    assert_eq!(result, plan(&domain, &facts, "entry", &[]));
}

const HEALER: &str = r#"(version:1,name:"healer",entries:["choose"],tasks:[
  (name:"choose",parameters:[],methods:[(name:"heal",locals:[Int,Int],steps:[
    Query(name:"ally",arguments:[Local(0)]),
    Query(name:"position",arguments:[Local(0),Local(1)]),
    Require(name:"usable",arguments:[Local(1)]),
    Action(name:"heal",arguments:[Local(0),Local(1)])])])])"#;
fn healer_schema() -> Schema {
    Schema {
        predicates: vec![
            Signature {
                name: "ally".into(),
                arguments: vec![Type::Int],
            },
            Signature {
                name: "position".into(),
                arguments: vec![Type::Int, Type::Int],
            },
            Signature {
                name: "usable".into(),
                arguments: vec![Type::Int],
            },
        ],
        actions: vec![Signature {
            name: "heal".into(),
            arguments: vec![Type::Int, Type::Int],
        }],
        ..Schema::default()
    }
}
fn row(predicate: &str, values: &[i64]) -> Fact {
    Fact {
        predicate: predicate.into(),
        arguments: values.iter().copied().map(Value::Int).collect(),
    }
}
fn healer_rows() -> Vec<Fact> {
    vec![
        row("ally", &[1]),
        row("ally", &[2]),
        row("position", &[1, 10]),
        row("position", &[2, 20]),
        row("usable", &[20]),
    ]
}

#[test_tag::tag(unit)]
#[test]
fn healer_revisits_ally_after_exhausting_inaccessible_positions() {
    let schema = healer_schema();
    let domain =
        htn_planner_ron::compile("healer", HEALER, schema.clone(), Limits::small()).unwrap();
    let facts = FactSnapshot::new(&schema, healer_rows()).unwrap();
    let before = facts.clone();
    let result = plan(&domain, &facts, "choose", &[]);
    let Outcome::Success(actions) = &result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0].as_action().unwrap().arguments,
        vec![Value::Int(2), Value::Int(20)]
    );
    assert_eq!(facts, before);
    assert_eq!(result, plan(&domain, &facts, "choose", &[]));
}

#[test_tag::tag(unit)]
#[test]
fn healer_tries_later_position_for_preferred_ally_before_next_ally() {
    let schema = healer_schema();
    let domain =
        htn_planner_ron::compile("healer", HEALER, schema.clone(), Limits::small()).unwrap();
    let mut rows = healer_rows();
    rows.insert(3, row("position", &[1, 11]));
    rows.push(row("usable", &[11]));
    let facts = FactSnapshot::new(&schema, rows).unwrap();
    let result = plan(&domain, &facts, "choose", &[]);
    let Outcome::Success(actions) = result.outcome else {
        panic!("expected complete healer plan")
    };
    assert_eq!(
        actions[0].as_action().unwrap().arguments,
        vec![Value::Int(1), Value::Int(11)]
    );
}

#[test_tag::tag(unit)]
#[test]
fn repeated_variable_and_later_queries_constrain_instead_of_rebinding() {
    let schema = Schema {
        predicates: vec![
            Signature {
                name: "pair".into(),
                arguments: vec![Type::Int, Type::Int],
            },
            Signature {
                name: "allowed".into(),
                arguments: vec![Type::Int],
            },
        ],
        ..schema()
    };
    let source = r#"(version:1,name:"repeats",entries:["entry"],tasks:[(name:"entry",parameters:[],methods:[
      (name:"same",locals:[Int],steps:[Query(name:"pair",arguments:[Local(0),Local(0)]),Query(name:"allowed",arguments:[Local(0)]),Action(name:"emit",arguments:[Local(0)])])])])"#;
    let domain =
        htn_planner_ron::compile("repeats", source, schema.clone(), Limits::small()).unwrap();
    let facts = FactSnapshot::new(
        &schema,
        vec![
            row("pair", &[1, 2]),
            row("pair", &[3, 3]),
            row("allowed", &[4]),
            row("allowed", &[3]),
        ],
    )
    .unwrap();
    let result = plan(&domain, &facts, "entry", &[]);
    let Outcome::Success(actions) = result.outcome else {
        panic!("expected compatible tuple")
    };
    assert_eq!(
        actions[0].as_action().unwrap().arguments,
        vec![Value::Int(3)]
    );
}

#[test_tag::tag(unit)]
#[test]
fn task_invocations_have_independent_local_scopes() {
    let schema = Schema {
        predicates: vec![Signature {
            name: "echo".into(),
            arguments: vec![Type::Int, Type::Int],
        }],
        actions: vec![Signature {
            name: "pair".into(),
            arguments: vec![Type::Int, Type::Int],
        }],
        ..Schema::default()
    };
    let source = r#"(version:1,name:"scope",entries:["entry"],tasks:[
      (name:"entry",parameters:[],methods:[(name:"both",locals:[Int,Int],steps:[
        Call(name:"echo",arguments:[Literal(Int(1))],outputs:[Local(0)]),
        Call(name:"echo",arguments:[Literal(Int(2))],outputs:[Local(1)]),
        Action(name:"pair",arguments:[Local(0),Local(1)])])]),
      (name:"echo",parameters:[Int],outputs:[Int],methods:[(name:"lookup",locals:[Int],steps:[Query(name:"echo",arguments:[Parameter(0),Local(0)])],result:[Local(0)])])])"#;
    let domain =
        htn_planner_ron::compile("scope", source, schema.clone(), Limits::small()).unwrap();
    let facts =
        FactSnapshot::new(&schema, vec![row("echo", &[1, 11]), row("echo", &[2, 22])]).unwrap();
    let result = plan(&domain, &facts, "entry", &[]);
    let Outcome::Success(actions) = result.outcome else {
        panic!("expected independent scopes")
    };
    assert_eq!(
        actions[0].as_action().unwrap().arguments,
        vec![Value::Int(11), Value::Int(22)]
    );
}

#[test_tag::tag(unit)]
#[test]
fn call_return_conflict_is_no_plan_without_overwriting_a_bound_local() {
    let schema = schema();
    let source = r#"(version:1,name:"conflict",entries:["entry"],tasks:[
      (name:"entry",parameters:[],methods:[(name:"bounded",locals:[Int],steps:[Query(name:"allowed",arguments:[Local(0)]),Call(name:"one",arguments:[],outputs:[Local(0)])])]),
      (name:"one",parameters:[],outputs:[Int],methods:[(name:"return",steps:[Action(name:"emit",arguments:[Literal(Int(1))])],result:[Literal(Int(1))])])])"#;
    let domain =
        htn_planner_ron::compile("conflict", source, schema.clone(), Limits::small()).unwrap();
    let facts = FactSnapshot::new(&schema, vec![row("allowed", &[2])]).unwrap();
    let result = plan(&domain, &facts, "entry", &[]);
    assert_eq!(result.outcome, Outcome::NoPlan);
    let diagnostic = result.diagnostic.unwrap();
    assert_eq!(diagnostic.locator.source, "conflict");
    assert!(diagnostic.message.contains("return conflicts"));
}

#[test_tag::tag(unit)]
#[test]
fn admission_rejects_unbound_local_reads_and_task_output_misuse() {
    for steps in [
        "Action(name:\"emit\",arguments:[Local(0)])",
        "Task(name:\"result\",arguments:[])",
        "Call(name:\"result\",arguments:[],outputs:[Local(1)])",
        "Call(name:\"result\",arguments:[Local(0)],outputs:[Local(0)])",
    ] {
        let source = format!(
            "(version:1,name:\"invalid\",entries:[\"entry\"],tasks:[(name:\"entry\",parameters:[],methods:[(name:\"bad\",locals:[Int],steps:[{steps}])]),(name:\"result\",parameters:[],outputs:[Int],methods:[(name:\"one\",steps:[],result:[Literal(Int(1))])])])"
        );
        let error =
            htn_planner_ron::compile("invalid", &source, schema(), Limits::small()).unwrap_err();
        assert_eq!(error.locator.declaration, "bad");
        assert_eq!(error.locator.step, Some(0));
    }
}

#[test_tag::tag(unit)]
#[test]
fn method_results_require_bound_values_of_declared_output_types() {
    for result in ["[]", "[Local(0)]", "[Literal(Bool(true))]"] {
        let source = format!(
            "(version:1,name:\"invalid-return\",entries:[\"entry\"],tasks:[(name:\"entry\",parameters:[],outputs:[Int],methods:[(name:\"bad-return\",locals:[Int],steps:[],result:{result})])])"
        );
        let error = htn_planner_ron::compile(
            "invalid-return",
            &source,
            Schema::default(),
            Limits::small(),
        )
        .unwrap_err();
        assert_eq!(error.locator.declaration, "bad-return");
        assert_eq!(error.locator.step, None);
    }
}

#[test_tag::tag(unit)]
#[test]
fn later_sibling_reopens_child_query_row_after_successful_return() {
    let schema = Schema {
        predicates: vec![
            Signature {
                name: "candidate".into(),
                arguments: vec![Type::Int],
            },
            Signature {
                name: "allowed".into(),
                arguments: vec![Type::Int],
            },
        ],
        ..schema()
    };
    let source = r#"(version:1,name:"query-return",entries:["entry"],tasks:[
      (name:"entry",parameters:[],methods:[
        (name:"preferred",locals:[Int],steps:[Call(name:"choose",arguments:[],outputs:[Local(0)]),Require(name:"allowed",arguments:[Local(0)])]),
        (name:"fallback",steps:[Action(name:"emit",arguments:[Literal(Int(99))])])]),
      (name:"choose",parameters:[],outputs:[Int],methods:[(name:"rows",locals:[Int],steps:[
        Query(name:"candidate",arguments:[Local(0)]),Action(name:"emit",arguments:[Local(0)])],result:[Local(0)])])])"#;
    let domain =
        htn_planner_ron::compile("query-return", source, schema.clone(), Limits::small()).unwrap();
    let facts = FactSnapshot::new(
        &schema,
        vec![
            row("candidate", &[1]),
            row("candidate", &[2]),
            row("allowed", &[2]),
        ],
    )
    .unwrap();
    let result = plan(&domain, &facts, "entry", &[]);
    let Outcome::Success(actions) = result.outcome else {
        panic!("expected second returned row")
    };
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0].as_action().unwrap().arguments,
        vec![Value::Int(2)]
    );
    assert_eq!(actions[0].as_action().unwrap().locator.declaration, "rows");
}

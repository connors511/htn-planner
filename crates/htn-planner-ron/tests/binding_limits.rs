use htn_planner::{Fact, FactSnapshot, Limits, Outcome, Schema, Signature, Type, Value, plan};
const DOMAIN: &str = r#"(version:1,name:"bounded-query",entries:["entry"],tasks:[
  (name:"entry",parameters:[],methods:[(name:"candidate",locals:[Int],steps:[
    Query(name:"candidate",arguments:[Local(0)]),Require(name:"allowed",arguments:[Local(0)])])])])"#;
fn schema() -> Schema {
    Schema {
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
        ..Schema::default()
    }
}
fn rows(count: i64) -> Vec<Fact> {
    (0..count)
        .map(|value| Fact {
            predicate: "candidate".into(),
            arguments: vec![Value::Int(value)],
        })
        .collect()
}

#[test_tag::tag(unit)]
#[test]
fn high_fanout_distinguishes_absence_work_exhaustion_and_capacity() {
    let schema = schema();
    let facts = FactSnapshot::new(&schema, rows(80)).unwrap();
    let mut limits = Limits::small();
    limits.work = 100_000;
    let complete = htn_planner_ron::compile("bounded", DOMAIN, schema.clone(), limits).unwrap();
    let no_plan = plan(&complete, &facts, "entry", &[]);
    assert_eq!(no_plan.outcome, Outcome::NoPlan);
    let mut exhausted = limits;
    exhausted.work = 200;
    let domain = htn_planner_ron::compile("bounded", DOMAIN, schema.clone(), exhausted).unwrap();
    assert_eq!(
        plan(&domain, &facts, "entry", &[]).outcome,
        Outcome::BudgetExhausted
    );
    let mut full = limits;
    full.storage = 8;
    let domain = htn_planner_ron::compile("bounded", DOMAIN, schema, full).unwrap();
    let result = plan(&domain, &facts, "entry", &[]);
    assert_eq!(result.outcome, Outcome::CapacityExceeded);
    assert!(result.work.peak_storage <= full.storage);
    assert_eq!(plan(&complete, &facts, "entry", &[]), no_plan);
}

#[test_tag::tag(unit)]
#[test]
fn query_backtracking_obeys_exact_work_and_storage_allowances() {
    let schema = schema();
    let mut rows = rows(4);
    rows.push(Fact {
        predicate: "allowed".into(),
        arguments: vec![Value::Int(3)],
    });
    let facts = FactSnapshot::new(&schema, rows).unwrap();
    let domain =
        htn_planner_ron::compile("boundaries", DOMAIN, schema.clone(), Limits::small()).unwrap();
    let expected = plan(&domain, &facts, "entry", &[]);
    assert_eq!(expected.outcome, Outcome::Success(vec![]));
    let mut limits = Limits::small();
    limits.work = expected.work.consumed;
    limits.storage = expected.work.peak_storage;
    let exact = htn_planner_ron::compile("boundaries", DOMAIN, schema.clone(), limits).unwrap();
    assert_eq!(plan(&exact, &facts, "entry", &[]), expected);
    limits.work -= 1;
    let short = htn_planner_ron::compile("boundaries", DOMAIN, schema.clone(), limits).unwrap();
    assert_eq!(
        plan(&short, &facts, "entry", &[]).outcome,
        Outcome::BudgetExhausted
    );
    limits.work = expected.work.consumed;
    limits.storage -= 1;
    let short = htn_planner_ron::compile("boundaries", DOMAIN, schema, limits).unwrap();
    assert_eq!(
        plan(&short, &facts, "entry", &[]).outcome,
        Outcome::CapacityExceeded
    );
}

#[test_tag::tag(unit)]
#[test]
fn deep_task_scopes_use_bounded_explicit_storage() {
    use htn_planner::{CompiledDomain, Definition, Method, Step, Task};
    let depth = 40usize;
    let tasks = (0..depth)
        .map(|index| Task {
            name: format!("task{index}"),
            parameters: vec![],
            outputs: vec![],
            methods: vec![Method {
                name: format!("method{index}"),
                locals: vec![],
                result: vec![],
                steps: if index + 1 == depth {
                    vec![]
                } else {
                    vec![Step::Task {
                        name: format!("task{}", index + 1),
                        arguments: vec![],
                    }]
                },
            }],
        })
        .collect();
    let definition = Definition {
        queries: vec![],
        version: 1,
        name: "deep".into(),
        entries: vec!["task0".into()],
        tasks,
    };
    let schema = Schema::default();
    let facts = FactSnapshot::new(&schema, vec![]).unwrap();
    let mut limits = Limits::small();
    limits.depth = depth;
    let domain = CompiledDomain::admit("deep", schema.clone(), definition.clone(), limits).unwrap();
    assert_eq!(
        plan(&domain, &facts, "task0", &[]).outcome,
        Outcome::Success(vec![])
    );
    limits.storage = depth / 2;
    let domain = CompiledDomain::admit("deep", schema, definition, limits).unwrap();
    assert_eq!(
        plan(&domain, &facts, "task0", &[]).outcome,
        Outcome::CapacityExceeded
    );
}

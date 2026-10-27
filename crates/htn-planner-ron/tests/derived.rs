use htn_planner::{Fact, FactSnapshot, Limits, Outcome, Schema, Signature, Type, Value, plan};
fn schema() -> Schema {
    Schema {
        predicates: vec![
            Signature {
                name: "pair".into(),
                arguments: vec![Type::Int, Type::Int],
            },
            Signature {
                name: "accepted".into(),
                arguments: vec![Type::Int, Type::Int],
            },
        ],
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![Type::Int, Type::Int],
        }],
        ..Schema::default()
    }
}
const DERIVED: &str = r#"(version:1,name:"nested",entries:["entry"],queries:[
(name:"inner",parameters:[Int],outputs:[Int],locals:[Int],condition:Fact(name:"pair",arguments:[Parameter(0),Local(0)]),result:[Local(0)]),
(name:"outer",parameters:[Int],outputs:[Int],locals:[Int],condition:Derived(name:"inner",arguments:[Parameter(0)],outputs:[Local(0)]),result:[Local(0)])
],tasks:[(name:"entry",parameters:[],methods:[(name:"select",locals:[Int,Int],steps:[
Condition(Derived(name:"outer",arguments:[Literal(Int(1))],outputs:[Local(0)])),
Condition(Derived(name:"outer",arguments:[Literal(Int(2))],outputs:[Local(1)])),
Require(name:"accepted",arguments:[Local(0),Local(1)]),Action(name:"emit",arguments:[Local(0),Local(1)])])])])"#;
#[test_tag::tag(unit)]
#[test]
fn nested_scopes_remain_distinct_and_reopen_earlier_derived_results() {
    let schema = schema();
    let mut limits = Limits::small();
    limits.storage = 65536;
    let domain = htn_planner_ron::compile("nested", DERIVED, schema.clone(), limits).unwrap();
    let rows = [
        ("pair", 1, 10),
        ("pair", 1, 11),
        ("pair", 2, 20),
        ("accepted", 11, 20),
    ]
    .into_iter()
    .map(|(name, a, b)| Fact {
        predicate: name.into(),
        arguments: vec![Value::Int(a), Value::Int(b)],
    })
    .collect();
    let facts = FactSnapshot::new(&schema, rows).unwrap();
    let result = plan(&domain, &facts, "entry", &[]);
    let Outcome::Success(entries) = result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(
        entries[0].as_action().unwrap().arguments,
        vec![Value::Int(11), Value::Int(20)]
    );
}
#[test_tag::tag(invariant)]
#[test]
fn derived_dependency_cycles_through_negation_are_rejected() {
    let source = r#"(version:1,name:"cycle",entries:["entry"],queries:[(name:"cycle",parameters:[],outputs:[],locals:[],condition:Not(Derived(name:"cycle",arguments:[],outputs:[])),result:[])],tasks:[(name:"entry",parameters:[],methods:[])])"#;
    assert!(htn_planner_ron::compile("cycle", source, Schema::default(), Limits::small()).is_err());
}
#[test_tag::tag(unit)]
#[test]
fn empty_candidate_relation_returns_no_plan() {
    let schema = schema();
    let domain =
        htn_planner_ron::compile("empty", DERIVED, schema.clone(), Limits::small()).unwrap();
    let facts = FactSnapshot::new(&schema, vec![]).unwrap();
    assert_eq!(plan(&domain, &facts, "entry", &[]).outcome, Outcome::NoPlan);
}

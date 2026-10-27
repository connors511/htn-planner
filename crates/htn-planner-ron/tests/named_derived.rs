//! Derived scopes, nested controls and task composition use the existing runtime semantics.
use htn_planner::{
    EffectOperation, Expression, Fact, Limits, Outcome, PlanningEffect, Schema, Signature, Type,
    Value, plan,
};
const NAMED: &str = r#"(version:2,name:"nested",entries:["entry"],queries:[
(name:"increment",parameters:[Int("input")],outputs:[Int],locals:[Int("next")],
condition:And([
Calculate(op:Add,left:Var("input"),right:Literal(Int(1)),output:Var("next")),
Or([Fact(name:"allowed",arguments:[Var("next")]),Compare(op:Equal,left:Var("next"),right:Literal(Int(5)))]),
FirstSuccess([Fact(name:"allowed",arguments:[Var("next")]),Compare(op:Equal,left:Var("next"),right:Literal(Int(6)))]),
Not(Fact(name:"blocked",arguments:[Var("next")]))]),result:[Var("next")])],tasks:[
(name:"entry",parameters:[],methods:[(name:"compose",locals:[Int("first"),Int("second")],steps:[
Call(name:"seed",arguments:[],outputs:[Var("first")]),
Condition(Derived(name:"increment",arguments:[Var("first")],outputs:[Var("second")])),
Effect(name:"mark",arguments:[Var("second")]),
Require(name:"seen",arguments:[Var("second")]),
Task(name:"relay",arguments:[Var("second")]),
Defer(name:"relay",arguments:[Var("second")])])]),
(name:"seed",parameters:[],outputs:[Int],methods:[(name:"give",steps:[],result:[Literal(Int(4))])]),
(name:"relay",parameters:[Int("value")],methods:[(name:"emit",steps:[Action(name:"emit",arguments:[Var("value")])])])])"#;
const NUMERIC: &str = r#"(version:1,name:"nested",entries:["entry"],queries:[
(name:"increment",parameters:[Int],outputs:[Int],locals:[Int],
condition:And([
Calculate(op:Add,left:Parameter(0),right:Literal(Int(1)),output:Local(0)),
Or([Fact(name:"allowed",arguments:[Local(0)]),Compare(op:Equal,left:Local(0),right:Literal(Int(5)))]),
FirstSuccess([Fact(name:"allowed",arguments:[Local(0)]),Compare(op:Equal,left:Local(0),right:Literal(Int(6)))]),
Not(Fact(name:"blocked",arguments:[Local(0)]))]),result:[Local(0)])],tasks:[
(name:"entry",parameters:[],methods:[(name:"compose",locals:[Int,Int],steps:[
Call(name:"seed",arguments:[],outputs:[Local(0)]),
Condition(Derived(name:"increment",arguments:[Local(0)],outputs:[Local(1)])),
Effect(name:"mark",arguments:[Local(1)]),Require(name:"seen",arguments:[Local(1)]),
Task(name:"relay",arguments:[Local(1)]),Defer(name:"relay",arguments:[Local(1)])])]),
(name:"seed",parameters:[],outputs:[Int],methods:[(name:"give",steps:[],result:[Literal(Int(4))])]),
(name:"relay",parameters:[Int],methods:[(name:"emit",steps:[Action(name:"emit",arguments:[Parameter(0)])])])])"#;
fn schema() -> Schema {
    Schema {
        predicates: ["allowed", "blocked", "seen"]
            .into_iter()
            .map(|name| Signature {
                name: name.into(),
                arguments: vec![Type::Int],
            })
            .collect(),
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![Type::Int],
        }],
        effects: vec![PlanningEffect {
            name: "mark".into(),
            parameters: vec![Type::Int],
            operations: vec![EffectOperation::Assert {
                predicate: "seen".into(),
                arguments: vec![Expression::Parameter(0)],
            }],
        }],
        ..Schema::default()
    }
}
#[test]
#[test_tag::tag(compatibility)]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "Both synthetic programs must admit and produce a complete composed plan"
)]
fn named_nested_conditions_and_derived_returns_preserve_complete_numeric_result() {
    let named = htn_planner_ron::compile("nested", NAMED, schema(), Limits::small()).unwrap();
    let numeric = htn_planner_ron::compile("nested", NUMERIC, schema(), Limits::small()).unwrap();
    assert_eq!(named.definition(), numeric.definition());
    assert_eq!(named.identity(), numeric.identity());
    let facts = named
        .snapshot(vec![Fact {
            predicate: "allowed".into(),
            arguments: vec![Value::Int(5)],
        }])
        .unwrap();
    let before = facts.clone();
    let actual = plan(&named, &facts, "entry", &[]);
    assert_eq!(actual, plan(&numeric, &facts, "entry", &[]));
    let Outcome::Success(entries) = actual.outcome else {
        panic!("expected complete composed plan")
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0].as_action().unwrap().arguments,
        vec![Value::Int(5)]
    );
    assert!(matches!(entries[1], htn_planner::PlanEntry::Deferred(_)));
    assert_eq!(facts, before);
}
#[test]
#[test_tag::tag(invariant)]
#[expect(
    clippy::unwrap_used,
    reason = "The parsed synthetic query must reject its deliberately unknown return"
)]
fn derived_query_names_are_local_and_unknown_returns_retain_the_query_context() {
    use htn_planner_ron::authoring::{Definition, Expression};
    let mut source: Definition = ron::from_str(NAMED).unwrap();
    source.queries[0].result[0] = Expression::Var("not_in_query_scope".into());
    let error = source.lower("derived").unwrap_err();
    assert_eq!(error.locator.source, "derived");
    assert_eq!(error.locator.task.as_deref(), Some("increment"));
    assert_eq!(error.locator.declaration, "increment");
    assert!(error.message.contains("not_in_query_scope"));
}

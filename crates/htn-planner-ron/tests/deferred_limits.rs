use htn_planner::{
    CompiledDomain, DeferredTask, FactSnapshot, Limits, Outcome, PlanEntry, Schema, Signature,
    Type, expand_deferred, plan,
};
fn fixture(limits: Limits) -> Result<(CompiledDomain, FactSnapshot, DeferredTask), String> {
    let steps = (0..8)
        .map(|value| format!("Action(name:\"mark\",arguments:[Literal(Int({value}))])"))
        .collect::<Vec<_>>()
        .join(",");
    let source = format!(
        "(version:1,name:\"limits\",entries:[\"entry\"],tasks:[(name:\"entry\",parameters:[],methods:[(name:\"capture\",steps:[Defer(name:\"later\",arguments:[])])]),(name:\"later\",parameters:[],methods:[(name:\"many\",steps:[{steps}])])])"
    );
    let schema = Schema {
        actions: vec![Signature {
            name: "mark".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    };
    let facts = FactSnapshot::new(&schema, vec![])?;
    let domain = htn_planner_ron::compile("limits", &source, schema, limits)
        .map_err(|error| format!("{error:?}"))?;
    let Outcome::Success(mut entries) = plan(&domain, &facts, "entry", &[]).outcome else {
        return Err("token capture must fit tested profile".into());
    };
    let Some(PlanEntry::Deferred(token)) = entries.pop() else {
        return Err("expected token".into());
    };
    Ok((domain, facts, token))
}

#[test_tag::tag(unit)]
#[test]
fn deferred_expansion_obeys_one_exact_work_and_storage_allowance() {
    let (domain, facts, token) = fixture(Limits::small()).unwrap();
    let expected = expand_deferred(&domain, &facts, &token);
    assert!(matches!(&expected.outcome,Outcome::Success(entries) if entries.len() == 8));
    let mut exact = Limits::small();
    exact.work = expected.work.consumed;
    exact.storage = expected.work.peak_storage;
    let (domain, facts, token) = fixture(exact).unwrap();
    assert_eq!(expand_deferred(&domain, &facts, &token), expected);
    let mut short = exact;
    short.work -= 1;
    let (domain, facts, token) = fixture(short).unwrap();
    assert_eq!(
        expand_deferred(&domain, &facts, &token).outcome,
        Outcome::BudgetExhausted
    );
    short = exact;
    short.storage -= 1;
    let (domain, facts, token) = fixture(short).unwrap();
    assert_eq!(
        expand_deferred(&domain, &facts, &token).outcome,
        Outcome::CapacityExceeded
    );
}

#[test_tag::tag(unit)]
#[test]
fn deferred_plan_capacity_never_exposes_a_prefix() {
    let mut limits = Limits::small();
    limits.plan = 1;
    let (domain, facts, token) = fixture(limits).unwrap();
    assert_eq!(
        expand_deferred(&domain, &facts, &token).outcome,
        Outcome::CapacityExceeded
    );
}

#[test_tag::tag(unit)]
#[test]
fn deferred_empty_success_is_explicit_completion() {
    let source = r#"(version:1,name:"empty",entries:["entry"],tasks:[
      (name:"entry",parameters:[],methods:[(name:"capture",steps:[Defer(name:"later",arguments:[])])]),
      (name:"later",parameters:[],methods:[(name:"empty",steps:[])])])"#;
    let schema = Schema::default();
    let facts = FactSnapshot::new(&schema, vec![]).unwrap();
    let domain = htn_planner_ron::compile("empty", source, schema, Limits::small()).unwrap();
    let Outcome::Success(mut entries) = plan(&domain, &facts, "entry", &[]).outcome else {
        panic!("expected capture")
    };
    let Some(PlanEntry::Deferred(token)) = entries.pop() else {
        panic!("expected token")
    };
    let result = expand_deferred(&domain, &facts, &token);
    assert_eq!(result.outcome, Outcome::Success(vec![]));
    assert_eq!(result, expand_deferred(&domain, &facts, &token));
}

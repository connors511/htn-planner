use htn_planner::{
    Fact, FactSnapshot, Limits, Outcome, PlanEntry, Schema, Signature, Type, Value,
    expand_deferred, plan,
};

fn schema() -> Schema {
    Schema {
        predicates: vec![Signature {
            name: "ready".into(),
            arguments: vec![Type::Int, Type::Int],
        }],
        actions: vec![
            Signature {
                name: "prepare".into(),
                arguments: vec![Type::Int],
            },
            Signature {
                name: "work".into(),
                arguments: vec![Type::Int, Type::Int],
            },
        ],
        ..Schema::default()
    }
}
const DOMAIN: &str = r#"(version:1,name:"deferred",entries:["entry"],tasks:[
  (name:"entry",parameters:[Int],methods:[(name:"start",steps:[Action(name:"prepare",arguments:[Parameter(0)]),Defer(name:"follow",arguments:[Parameter(0)])])]),
  (name:"follow",parameters:[Int],methods:[(name:"fresh",locals:[Int],steps:[Query(name:"ready",arguments:[Parameter(0),Local(0)]),Action(name:"work",arguments:[Parameter(0),Local(0)])])])])"#;
fn ready(id: i64, position: i64) -> Fact {
    Fact {
        predicate: "ready".into(),
        arguments: vec![Value::Int(id), Value::Int(position)],
    }
}

#[test_tag::tag(unit)]
#[test]
fn deferred_private_task_uses_fresh_facts_and_retains_captured_inputs() {
    let schema = schema();
    let domain =
        htn_planner_ron::compile("deferred", DOMAIN, schema.clone(), Limits::small()).unwrap();
    let old = FactSnapshot::new(&schema, vec![ready(7, 1)]).unwrap();
    let Outcome::Success(entries) = plan(&domain, &old, "entry", &[Value::Int(7)]).outcome else {
        panic!("expected accepted plan")
    };
    assert!(matches!(&entries[0],PlanEntry::Action(action) if action.name == "prepare"));
    let PlanEntry::Deferred(token) = &entries[1] else {
        panic!("expected deferred entry")
    };
    assert_eq!(token.arguments(), &[Value::Int(7)]);
    let fresh = FactSnapshot::new(&schema, vec![ready(9, 9), ready(7, 2)]).unwrap();
    let result = expand_deferred(&domain, &fresh, token);
    let Outcome::Success(expansion) = result.outcome else {
        panic!("expected fresh expansion")
    };
    assert!(
        matches!(&expansion[0],PlanEntry::Action(action) if action.arguments == vec![Value::Int(7),Value::Int(2)])
    );
    assert_eq!(token.arguments(), &[Value::Int(7)]);
}

fn captured(
    domain: &htn_planner::CompiledDomain,
    facts: &FactSnapshot,
    arguments: &[Value],
) -> Result<htn_planner::DeferredTask, String> {
    let Outcome::Success(mut entries) = plan(domain, facts, "entry", arguments).outcome else {
        return Err("expected capture".into());
    };
    let Some(PlanEntry::Deferred(token)) = entries.pop() else {
        return Err("expected token".into());
    };
    Ok(token)
}
#[derive(serde::Serialize)]
struct TokenFields<'a> {
    definition: htn_planner::DefinitionIdentity,
    semantics: &'a str,
    task: &'a str,
    arguments: &'a [Value],
    locator: &'a htn_planner::Locator,
}
fn modified(
    token: &htn_planner::DeferredTask,
    semantics: &str,
    task: &str,
    arguments: &[Value],
) -> Result<htn_planner::DeferredTask, String> {
    // Exercise the explicitly untrusted serde seam with field names from its contract.
    let fields = TokenFields {
        definition: token.definition_identity(),
        semantics,
        task,
        arguments,
        locator: token.locator(),
    };
    let text = ron::ser::to_string(&fields).map_err(|error| error.to_string())?;
    ron::from_str(&text).map_err(|error| error.to_string())
}

#[test_tag::tag(compatibility)]
#[test]
fn deferred_expansion_rejects_identity_semantics_and_undeclared_targets() {
    let schema = schema();
    let domain =
        htn_planner_ron::compile("guards", DOMAIN, schema.clone(), Limits::small()).unwrap();
    let facts = FactSnapshot::new(&schema, vec![ready(7, 1)]).unwrap();
    let token = captured(&domain, &facts, &[Value::Int(7)]).unwrap();
    let mut changed = Limits::small();
    changed.work += 1;
    let other = htn_planner_ron::compile("guards", DOMAIN, schema, changed).unwrap();
    let result = expand_deferred(&other, &facts, &token);
    assert_eq!(result.outcome, Outcome::InvalidInput);
    assert!(
        result
            .diagnostic
            .unwrap()
            .message
            .contains("definition identity")
    );
    let old = modified(&token, "old semantics", token.task(), token.arguments()).unwrap();
    let result = expand_deferred(&domain, &facts, &old);
    assert_eq!(result.outcome, Outcome::InvalidInput);
    assert!(result.diagnostic.unwrap().message.contains("semantics"));
    let undeclared = modified(&token, token.semantics(), "entry", token.arguments()).unwrap();
    let result = expand_deferred(&domain, &facts, &undeclared);
    assert_eq!(result.outcome, Outcome::InvalidInput);
    assert!(result.diagnostic.unwrap().message.contains("not declared"));
    assert_eq!(
        plan(&domain, &facts, "follow", token.arguments()).outcome,
        Outcome::InvalidInput
    );
}

#[test_tag::tag(unit)]
#[test]
fn unavailable_reference_has_no_plan_while_malformed_input_is_invalid() {
    let schema = schema();
    let domain =
        htn_planner_ron::compile("stale", DOMAIN, schema.clone(), Limits::small()).unwrap();
    let facts = FactSnapshot::new(&schema, vec![ready(7, 1)]).unwrap();
    let token = captured(&domain, &facts, &[Value::Int(7)]).unwrap();
    let removed = FactSnapshot::new(&schema, vec![]).unwrap();
    assert_eq!(
        expand_deferred(&domain, &removed, &token).outcome,
        Outcome::NoPlan
    );
    let malformed = modified(
        &token,
        token.semantics(),
        token.task(),
        &[Value::Bool(true)],
    )
    .unwrap();
    assert_eq!(
        expand_deferred(&domain, &facts, &malformed).outcome,
        Outcome::InvalidInput
    );
    assert!(matches!(
        expand_deferred(&domain, &facts, &token).outcome,
        Outcome::Success(_)
    ));
}

#[test_tag::tag(unit)]
#[test]
fn failed_deferred_expansion_never_exposes_a_tentative_action_prefix() {
    let source = r#"(version:1,name:"failed",entries:["entry"],tasks:[
      (name:"entry",parameters:[],methods:[(name:"capture",steps:[Defer(name:"later",arguments:[])])]),
      (name:"later",parameters:[],methods:[(name:"fail",steps:[Action(name:"prepare",arguments:[Literal(Int(7))]),Require(name:"ready",arguments:[Literal(Int(7)),Literal(Int(1))])])])])"#;
    let schema = schema();
    let domain =
        htn_planner_ron::compile("failed", source, schema.clone(), Limits::small()).unwrap();
    let facts = FactSnapshot::new(&schema, vec![]).unwrap();
    let token = captured(&domain, &facts, &[]).unwrap();
    assert_eq!(
        expand_deferred(&domain, &facts, &token).outcome,
        Outcome::NoPlan
    );
}

#[test_tag::tag(unit)]
#[test]
fn deferred_inputs_must_be_bound_and_targets_must_be_acyclic_with_no_outputs() {
    for step in [
        "Defer(name:\"entry\",arguments:[])",
        "Defer(name:\"later\",arguments:[Local(0)])",
    ] {
        let source = format!(
            "(version:1,name:\"bad\",entries:[\"entry\"],tasks:[(name:\"entry\",parameters:[],methods:[(name:\"bad\",locals:[Int],steps:[{step}])]),(name:\"later\",parameters:[Int],methods:[(name:\"empty\",steps:[])])])"
        );
        assert!(
            htn_planner_ron::compile("bad", &source, Schema::default(), Limits::small()).is_err()
        );
    }
    let source = r#"(version:1,name:"bad-output",entries:["entry"],tasks:[
      (name:"entry",parameters:[],methods:[(name:"defer",steps:[Defer(name:"later",arguments:[])])]),
      (name:"later",parameters:[],outputs:[Int],methods:[(name:"one",steps:[],result:[Literal(Int(1))])])])"#;
    let error = htn_planner_ron::compile("bad-output", source, Schema::default(), Limits::small())
        .unwrap_err();
    assert!(error.message.contains("cannot return outputs"));
}

#[test_tag::tag(compatibility)]
#[test]
fn executable_provenance_disambiguates_equal_method_names_and_legacy_locators_default_task() {
    let source = r#"(version:1,name:"provenance",entries:["entry"],tasks:[
      (name:"entry",parameters:[Int],methods:[(name:"same",steps:[Defer(name:"follow",arguments:[Parameter(0)])])]),
      (name:"follow",parameters:[Int],methods:[(name:"same",steps:[Action(name:"prepare",arguments:[Parameter(0)])])])])"#;
    let schema = schema();
    let facts = FactSnapshot::new(&schema, vec![]).unwrap();
    let domain = htn_planner_ron::compile("provenance", source, schema, Limits::small()).unwrap();
    let token = captured(&domain, &facts, &[Value::Int(7)]).unwrap();
    assert_eq!(token.locator().task.as_deref(), Some("entry"));
    assert_eq!(token.locator().declaration, "same");
    let Outcome::Success(entries) = expand_deferred(&domain, &facts, &token).outcome else {
        panic!("expected expansion")
    };
    let action = entries[0].as_action().unwrap();
    assert_eq!(action.locator.task.as_deref(), Some("follow"));
    assert_eq!(action.locator.declaration, token.locator().declaration);
    // This fixed legacy locator shape is the explicit serde-default contract.
    let legacy: htn_planner::Locator =
        ron::from_str(r#"(source:"legacy",declaration:"same",step:Some(0))"#).unwrap();
    assert_eq!(legacy.task, None);
}

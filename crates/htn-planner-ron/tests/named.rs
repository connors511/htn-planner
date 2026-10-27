//! Named authoring lowers to the same executable contract as numeric definitions.
use htn_planner::{Fact, Limits, Schema, Signature, Type, Value, plan};

const NAMED: &str = r#"(version:2,name:"selection",entries:["choose"],tasks:[
(name:"choose",parameters:[Int("minimum")],methods:[
(name:"select",locals:[Int("target")],steps:[
Query(name:"candidate",arguments:[Var("target")]),
Condition(Compare(op:Greater,left:Var("target"),right:Var("minimum"))),
Action(name:"emit",arguments:[Var("target")])])])])"#;
const NUMERIC: &str = r#"(version:1,name:"selection",entries:["choose"],tasks:[
(name:"choose",parameters:[Int],methods:[
(name:"select",locals:[Int],steps:[
Query(name:"candidate",arguments:[Local(0)]),
Condition(Compare(op:Greater,left:Local(0),right:Parameter(0))),
Action(name:"emit",arguments:[Local(0)])])])])"#;
fn schema() -> Schema {
    Schema {
        predicates: vec![Signature {
            name: "candidate".into(),
            arguments: vec![Type::Int],
        }],
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    }
}
#[test]
#[test_tag::tag(compatibility)]
#[expect(
    clippy::unwrap_used,
    reason = "Both valid compatibility fixtures and their synthetic snapshot must admit"
)]
fn named_and_numeric_sources_have_identical_lowered_identity_and_results() {
    let named = htn_planner_ron::compile("selection", NAMED, schema(), Limits::small()).unwrap();
    let numeric =
        htn_planner_ron::compile("selection", NUMERIC, schema(), Limits::small()).unwrap();
    assert_eq!(named.definition(), numeric.definition());
    assert_eq!(named.identity(), numeric.identity());
    let facts = named
        .snapshot(vec![Fact {
            predicate: "candidate".into(),
            arguments: vec![Value::Int(7)],
        }])
        .unwrap();
    assert_eq!(
        plan(&named, &facts, "choose", &[Value::Int(3)]),
        plan(&numeric, &facts, "choose", &[Value::Int(3)])
    );
}

#[expect(
    clippy::unwrap_used,
    reason = "The reusable synthetic named fixture must deserialize"
)]
fn definition() -> htn_planner_ron::authoring::Definition {
    ron::from_str(NAMED).unwrap()
}
#[test]
#[test_tag::tag(invariant)]
#[expect(
    clippy::unwrap_used,
    reason = "Every deliberately duplicated binding must reject"
)]
fn duplicate_parameters_locals_and_parameter_shadowing_reject_in_the_named_scope() {
    use htn_planner_ron::authoring::Binding;
    for duplicate in ["parameter", "local", "shadow"] {
        let mut definition = definition();
        let task = &mut definition.tasks[0];
        match duplicate {
            "parameter" => task.parameters.push(Binding::Bool("minimum".into())),
            "local" => task.methods[0].locals.push(Binding::Bool("target".into())),
            _ => task.methods[0].locals.push(Binding::Bool("minimum".into())),
        }
        let error = definition.lower("named-input").unwrap_err();
        assert_eq!(error.locator.source, "named-input");
        assert_eq!(error.locator.task.as_deref(), Some("choose"));
        assert!(error.message.contains("duplicate or shadowed"));
    }
}
#[test]
#[test_tag::tag(invariant)]
#[expect(
    clippy::unwrap_used,
    reason = "The deliberately unknown variable must reject"
)]
fn unknown_variable_keeps_source_task_method_step_and_human_name() {
    use htn_planner_ron::authoring::{Expression, Step};
    let mut definition = definition();
    definition.tasks[0].methods[0].steps[2] = Step::Action {
        name: "emit".into(),
        arguments: vec![Expression::Var("missing_target".into())],
    };
    let error = definition.lower("named-input").unwrap_err();
    assert_eq!(error.locator.source, "named-input");
    assert_eq!(error.locator.task.as_deref(), Some("choose"));
    assert_eq!(error.locator.declaration, "select");
    assert_eq!(error.locator.step, Some(2));
    assert!(error.message.contains("missing_target"));
}
#[test]
#[test_tag::tag(invariant)]
fn invalid_names_reject_but_the_explicit_ascii_length_boundary_is_supported() {
    use htn_planner_ron::authoring::Binding;
    for name in [
        String::new(),
        "3target".into(),
        "target-name".into(),
        "tårget".into(),
        "x".repeat(129),
    ] {
        let mut definition = definition();
        definition.tasks[0].parameters[0] = Binding::Int(name);
        assert!(definition.lower("names").is_err());
    }
    let mut definition = definition();
    definition.tasks[0]
        .parameters
        .push(Binding::Int("x".repeat(128)));
    assert!(definition.lower("names").is_ok());
}
#[test]
#[test_tag::tag(compatibility)]
#[expect(
    clippy::unwrap_used,
    reason = "The complete synthetic declaration vocabulary must lower"
)]
fn all_named_declaration_types_lower_to_unchanged_ordered_runtime_types() {
    use htn_planner_ron::authoring::{Binding, Task};
    let mut definition = definition();
    definition.tasks = vec![Task {
        name: "types".into(),
        parameters: vec![
            Binding::Ref("actor".into(), "Actor".into()),
            Binding::Record("attack".into(), "AttackChoice".into()),
            Binding::Int("distance".into()),
            Binding::Symbol("kind".into()),
            Binding::Bool("flag".into()),
            Binding::Fixed("weight".into(), 3),
        ],
        methods: vec![],
        outputs: vec![Type::Int],
    }];
    let lowered = definition.lower("types").unwrap();
    assert_eq!(
        lowered.tasks[0].parameters,
        vec![
            Type::Ref("Actor".into()),
            Type::Record("AttackChoice".into()),
            Type::Int,
            Type::Symbol,
            Type::Bool,
            Type::Fixed(3)
        ]
    );
    assert_eq!(lowered.tasks[0].outputs, vec![Type::Int]);
}
#[test]
#[test_tag::tag(invariant)]
#[expect(
    clippy::unwrap_used,
    reason = "Synthetic invalid bindings must serialize and then reject at admission"
)]
fn named_lowering_does_not_bypass_runtime_type_or_definite_binding_admission() {
    use htn_planner_ron::authoring::{Binding, Expression, Step};
    for type_mismatch in [false, true] {
        let mut definition = definition();
        definition.tasks[0].methods[0].steps = vec![Step::Action {
            name: "emit".into(),
            arguments: vec![Expression::Var("target".into())],
        }];
        if type_mismatch {
            definition.tasks[0].methods[0].locals[0] = Binding::Bool("target".into());
        }
        let text = ron::to_string(&definition).unwrap();
        let error =
            htn_planner_ron::compile("admission", &text, schema(), Limits::small()).unwrap_err();
        assert_eq!(error.locator.source, "admission");
        assert_eq!(error.locator.declaration, "select");
        assert_eq!(error.locator.step, Some(0));
        assert!(
            error
                .message
                .contains(if type_mismatch { "type" } else { "unbound" })
        );
    }
}
#[test]
#[test_tag::tag(invariant)]
fn explicit_versions_reject_malformed_named_or_unknown_input_without_numeric_fallback() {
    for source in [
        r#"(version:2,name:"bad",entries:[],tasks:[(name:"entry",parameters:[Int],methods:[])])"#,
        r#"(version:3,name:"bad",entries:[],tasks:[])"#,
        r#"(version:2,name:"bad",entries:[],tasks:[(name:"entry",parameters:[],methods:[(name:"bad",steps:[Action(name:"emit",arguments:[Local(0)])])])])"#,
    ] {
        assert!(htn_planner_ron::compile("version", source, schema(), Limits::small()).is_err());
    }
}

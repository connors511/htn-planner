//! Predictive setup facts support complete-continuation planning.
use htn_planner::{
    Fact, FactSnapshot, Limits, Outcome, RecordType, Schema, Signature, Type, Value, plan,
};

fn actor() -> Value {
    Value::Ref {
        type_name: "Actor".into(),
        key: 7,
    }
}
fn position(x: i64, y: i64) -> Value {
    Value::Record {
        type_name: "Position".into(),
        fields: vec![Value::Int(x), Value::Int(y)],
    }
}
fn schema() -> Schema {
    let actor = Type::Ref("Actor".into());
    let position = Type::Record("Position".into());
    Schema {
        references: vec!["Actor".into()],
        records: vec![RecordType {
            name: "Position".into(),
            fields: vec![Type::Int, Type::Int],
        }],
        predicates: ["setup", "finish_possible"]
            .into_iter()
            .map(|name| Signature {
                name: name.into(),
                arguments: vec![actor.clone(), position.clone()],
            })
            .collect(),
        actions: ["setup", "primary", "finish"]
            .into_iter()
            .map(|name| Signature {
                name: name.into(),
                arguments: vec![actor.clone(), position.clone()],
            })
            .chain([Signature {
                name: "fallback".into(),
                arguments: vec![actor.clone()],
            }])
            .collect(),
        ..Schema::default()
    }
}
fn fact(name: &str, position: &Value) -> Fact {
    Fact {
        predicate: name.into(),
        arguments: vec![actor(), position.clone()],
    }
}
fn actions(outcome: Outcome) -> Result<Vec<(String, Vec<Value>)>, String> {
    let Outcome::Success(entries) = outcome else {
        return Err(format!("expected complete synthetic plan, got {outcome:?}"));
    };
    entries
        .into_iter()
        .map(|entry| {
            let action = entry
                .as_action()
                .ok_or("fixture must emit only inert actions")?;
            Ok((action.name.clone(), action.arguments.clone()))
        })
        .collect()
}

// The host supplies immutable setup feasibility facts.
const REQUIRED: &str = r#"(
    version: 1,
    name: "required-combo",
    entries: ["engage"],
    tasks: [
        (
            name: "engage",
            parameters: [Ref("Actor")],
            methods: [
                (
                    name: "required-combo",
                    locals: [Record("Position")],
                    steps: [
                        Call(name: "prepare", arguments: [Parameter(0)], outputs: [Local(0)]),
                        Action(name: "primary", arguments: [Parameter(0), Local(0)]),
                        Require(name: "finish_possible", arguments: [Parameter(0), Local(0)]),
                        Action(name: "finish", arguments: [Parameter(0), Local(0)]),
                    ],
                ),
                (
                    name: "fallback",
                    steps: [Action(name: "fallback", arguments: [Parameter(0)])],
                ),
            ],
        ),
        (
            name: "prepare",
            parameters: [Ref("Actor")],
            outputs: [Record("Position")],
            methods: [(
                name: "ordered-setups",
                locals: [Record("Position")],
                steps: [
                    Query(name: "setup", arguments: [Parameter(0), Local(0)]),
                    Action(name: "setup", arguments: [Parameter(0), Local(0)]),
                ],
                result: [Local(0)],
            )],
        ),
    ],
)"#;

#[test_tag::tag(unit)]
#[test]
fn required_combo_revisits_setup_after_later_finish_fails() {
    let schema = schema();
    let domain = htn_planner_ron::compile(
        "synthetic-required-combo",
        REQUIRED,
        schema.clone(),
        Limits::small(),
    )
    .unwrap();
    let preferred = position(100, 0);
    let alternative = position(200, 50);
    let facts = FactSnapshot::new(
        &schema,
        vec![
            fact("setup", &preferred),
            fact("setup", &alternative),
            fact("finish_possible", &alternative),
        ],
    )
    .unwrap();
    let before = facts.clone();
    let result = plan(&domain, &facts, "engage", &[actor()]);
    assert_eq!(facts, before);
    assert_eq!(result, plan(&domain, &facts, "engage", &[actor()]));
    assert_eq!(
        actions(result.outcome).unwrap(),
        ["setup", "primary", "finish"]
            .into_iter()
            .map(|name| (name.into(), vec![actor(), alternative.clone()]))
            .collect::<Vec<_>>()
    );
}

#[test_tag::tag(unit)]
#[test]
fn required_combo_falls_back_without_setup_or_attack_prefix_when_all_finishes_fail() {
    let schema = schema();
    let domain = htn_planner_ron::compile(
        "synthetic-required-combo",
        REQUIRED,
        schema.clone(),
        Limits::small(),
    )
    .unwrap();
    let facts = FactSnapshot::new(
        &schema,
        vec![
            fact("setup", &position(100, 0)),
            fact("setup", &position(200, 50)),
        ],
    )
    .unwrap();
    let before = facts.clone();
    assert_eq!(
        actions(plan(&domain, &facts, "engage", &[actor()]).outcome).unwrap(),
        vec![("fallback".into(), vec![actor()])]
    );
    assert_eq!(facts, before);
}

// Optional followup feasibility must not turn the valid primary into a required combo.
const OPTIONAL: &str = r#"(
    version: 1,
    name: "optional-followup",
    entries: ["engage"],
    tasks: [
        (
            name: "engage",
            parameters: [Ref("Actor")],
            methods: [(
                name: "primary-and-optional-followup",
                locals: [Record("Position")],
                steps: [
                    Query(name: "setup", arguments: [Parameter(0), Local(0)]),
                    Action(name: "primary", arguments: [Parameter(0), Local(0)]),
                    Task(name: "optional-followup", arguments: [Parameter(0), Local(0)]),
                ],
            )],
        ),
        (
            name: "optional-followup",
            parameters: [Ref("Actor"), Record("Position")],
            methods: [
                (
                    name: "when-feasible",
                    steps: [
                        Require(name: "finish_possible", arguments: [Parameter(0), Parameter(1)]),
                        Action(name: "finish", arguments: [Parameter(0), Parameter(1)]),
                    ],
                ),
                (name: "otherwise-complete-primary", steps: []),
            ],
        ),
    ],
)"#;

#[test_tag::tag(unit)]
#[test]
fn optional_followup_keeps_primary_setup_when_only_a_different_setup_can_finish() {
    let schema = schema();
    let domain = htn_planner_ron::compile(
        "synthetic-optional-followup",
        OPTIONAL,
        schema.clone(),
        Limits::small(),
    )
    .unwrap();
    let preferred = position(100, 0);
    let alternative = position(200, 50);
    let facts = FactSnapshot::new(
        &schema,
        vec![
            fact("setup", &preferred),
            fact("setup", &alternative),
            fact("finish_possible", &alternative),
        ],
    )
    .unwrap();
    let before = facts.clone();
    let result = plan(&domain, &facts, "engage", &[actor()]);
    assert_eq!(facts, before);
    assert_eq!(result, plan(&domain, &facts, "engage", &[actor()]));
    assert_eq!(
        actions(result.outcome).unwrap(),
        vec![("primary".into(), vec![actor(), preferred])]
    );
}

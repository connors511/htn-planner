//! Owned public admission must reject arbitrary programmatic trees without recursive disposal.
use htn_planner::{
    CompiledDomain, Condition, Definition, DerivedQuery, EffectOperation, Expression, Fact,
    FactSnapshot, Limits, Method, PlanningEffect, Schema, Step, Task, Value,
};

// Deliberately beyond the128-level input contract. Construction is iterative;
// no enlarged/tiny custom stack or production content hides rejection cleanup.
fn overdeep() -> Value {
    let mut value = Value::Int(0);
    for _ in 0..20_000 {
        value = Value::Record {
            type_name: "R".into(),
            fields: vec![value],
        };
    }
    value
}
fn literal() -> Expression {
    Expression::Literal(overdeep())
}
fn effect() -> PlanningEffect {
    PlanningEffect {
        name: "effect".into(),
        parameters: vec![],
        operations: vec![
            EffectOperation::Assert {
                predicate: "unknown".into(),
                arguments: vec![literal()],
            },
            EffectOperation::Retract {
                predicate: "unknown".into(),
                arguments: vec![literal()],
            },
        ],
    }
}
fn definition() -> Definition {
    Definition {
        version: 1,
        name: "rejected".into(),
        entries: vec!["entry".into()],
        queries: vec![DerivedQuery {
            name: "query".into(),
            parameters: vec![],
            outputs: vec![],
            locals: vec![],
            condition: Condition::Not(Box::new(Condition::Calculate {
                op: htn_planner::NumericOperation::Add,
                left: literal(),
                right: literal(),
                output: literal(),
            })),
            result: vec![literal()],
        }],
        tasks: vec![Task {
            name: "entry".into(),
            parameters: vec![],
            outputs: vec![],
            methods: vec![Method {
                name: "method".into(),
                locals: vec![],
                result: vec![literal()],
                steps: vec![
                    Step::Action {
                        name: "unknown".into(),
                        arguments: vec![literal()],
                    },
                    Step::Call {
                        name: "unknown".into(),
                        arguments: vec![literal()],
                        outputs: vec![literal()],
                    },
                    Step::Condition(Condition::And(vec![
                        Condition::Compare {
                            op: htn_planner::Comparison::Equal,
                            left: literal(),
                            right: literal(),
                        },
                        Condition::Derived {
                            name: "unknown".into(),
                            arguments: vec![literal()],
                            outputs: vec![literal()],
                        },
                    ])),
                ],
            }],
        }],
    }
}
#[test]
#[test_tag::tag(invariant)]
fn unknown_predicate_rejection_disposes_owned_fact_values_iteratively() {
    let facts = vec![Fact {
        predicate: "unknown".into(),
        arguments: vec![overdeep()],
    }];
    assert!(FactSnapshot::new(&Schema::default(), facts).is_err());
}
#[test]
#[test_tag::tag(invariant)]
fn invalid_borrowed_schema_rejection_still_disposes_owned_fact_values() {
    let schema = Schema {
        references: vec![String::new()],
        ..Schema::default()
    };
    assert!(
        FactSnapshot::new(
            &schema,
            vec![Fact {
                predicate: "unknown".into(),
                arguments: vec![overdeep()]
            }]
        )
        .is_err()
    );
}
#[test]
#[test_tag::tag(invariant)]
fn source_rejection_disposes_owned_schema_and_every_definition_literal_location() {
    let schema = Schema {
        effects: vec![effect()],
        ..Schema::default()
    };
    assert!(
        CompiledDomain::admit("x".repeat(1025), schema, definition(), Limits::small()).is_err()
    );
}
#[test]
#[test_tag::tag(invariant)]
fn invalid_version_rejection_disposes_method_and_derived_query_literals() {
    let mut definition = definition();
    definition.version = 0;
    assert!(
        CompiledDomain::admit("version", Schema::default(), definition, Limits::small()).is_err()
    );
}
#[test]
#[test_tag::tag(invariant)]
fn invalid_schema_rejection_disposes_unvisited_definition_literals() {
    let schema = Schema {
        effects: vec![effect()],
        ..Schema::default()
    };
    assert!(CompiledDomain::admit("schema", schema, definition(), Limits::small()).is_err());
}

#[test]
#[test_tag::tag(invariant)]
fn known_predicate_depth_rejection_disposes_values_after_partial_validation() {
    let schema = Schema {
        records: vec![htn_planner::RecordType {
            name: "R".into(),
            fields: vec![htn_planner::Type::Record("R".into())],
        }],
        predicates: vec![htn_planner::Signature {
            name: "row".into(),
            arguments: vec![htn_planner::Type::Record("R".into())],
        }],
        ..Schema::default()
    };
    assert!(
        FactSnapshot::new(
            &schema,
            vec![Fact {
                predicate: "row".into(),
                arguments: vec![overdeep()]
            }]
        )
        .is_err()
    );
}
#[test]
#[test_tag::tag(invariant)]
fn consuming_schema_validation_safely_rejects_owned_effect_literals() {
    let schema = Schema {
        effects: vec![effect()],
        ..Schema::default()
    };
    assert!(schema.validate().is_err());
}
#[test]
#[test_tag::tag(compatibility)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "Controlled compatibility fixture variants and fields must remain consumable"
)]
fn valid_schema_validation_preserves_data_and_consuming_authoring_field_patterns() {
    use htn_planner::{RecordType, Signature, Type};
    let expected_fields = vec![Value::Int(-9)];
    let fact = Fact {
        predicate: "row".into(),
        arguments: vec![Value::Record {
            type_name: "R".into(),
            fields: expected_fields.clone(),
        }],
    };
    let Fact { arguments, .. } = fact;
    let expressions = arguments
        .into_iter()
        .map(Expression::Literal)
        .collect::<Vec<_>>();
    let schema = Schema {
        records: vec![RecordType {
            name: "R".into(),
            fields: vec![Type::Int],
        }],
        predicates: vec![Signature {
            name: "row".into(),
            arguments: vec![Type::Record("R".into())],
        }],
        effects: vec![PlanningEffect {
            name: "assert".into(),
            parameters: vec![],
            operations: vec![EffectOperation::Assert {
                predicate: "row".into(),
                arguments: expressions,
            }],
        }],
        ..Schema::default()
    };
    let expected = schema.clone();
    let validated = schema.validate().expect("valid closed schema");
    assert_eq!(validated, expected);
    let Schema { effects, .. } = validated;
    let EffectOperation::Assert { arguments, .. } = effects
        .into_iter()
        .next()
        .expect("one effect")
        .operations
        .into_iter()
        .next()
        .expect("one operation")
    else {
        panic!("controlled authoring variant")
    };
    let raw = Definition {
        version: 1,
        name: "moveable".into(),
        entries: vec![],
        queries: vec![],
        tasks: vec![Task {
            name: "task".into(),
            parameters: vec![],
            outputs: vec![],
            methods: vec![Method {
                name: "method".into(),
                locals: vec![],
                result: vec![],
                steps: vec![Step::Action {
                    name: "row".into(),
                    arguments,
                }],
            }],
        }],
    };
    let Definition { tasks, .. } = raw;
    let Step::Action { arguments, .. } = tasks
        .into_iter()
        .next()
        .expect("one task")
        .methods
        .into_iter()
        .next()
        .expect("one method")
        .steps
        .into_iter()
        .next()
        .expect("one step")
    else {
        panic!("controlled authoring variant")
    };
    let Expression::Literal(Value::Record { fields, .. }) =
        arguments.into_iter().next().expect("one literal")
    else {
        panic!("controlled authoring variant")
    };
    assert_eq!(fields, expected_fields);
}

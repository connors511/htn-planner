//! Parsing failures must also safely dispose the schema transferred by a caller.
use htn_planner::{EffectOperation, Expression, Limits, PlanningEffect, Schema, Value};
fn overdeep_schema() -> Schema {
    let mut value = Value::Int(0);
    for _ in 0..20_000 {
        value = Value::Record {
            type_name: "R".into(),
            fields: vec![value],
        };
    }
    Schema {
        effects: vec![PlanningEffect {
            name: "effect".into(),
            parameters: vec![],
            operations: vec![EffectOperation::Assert {
                predicate: "missing".into(),
                arguments: vec![Expression::Literal(value)],
            }],
        }],
        ..Schema::default()
    }
}
#[test]
#[test_tag::tag(invariant)]
fn malformed_text_rejection_disposes_owned_untrusted_schema() {
    assert!(
        htn_planner_ron::compile("invalid-text", "(", overdeep_schema(), Limits::small()).is_err()
    );
}
#[test]
#[test_tag::tag(invariant)]
fn oversized_source_rejection_disposes_owned_untrusted_schema() {
    assert!(
        htn_planner_ron::compile(&"x".repeat(1025), "(", overdeep_schema(), Limits::small())
            .is_err()
    );
}

#[test]
#[test_tag::tag(invariant)]
#[expect(clippy::expect_used, reason = "Synthetic invalid schema must fail")]
fn invalid_schema_error_keeps_oversized_source_locator_bounded() {
    let error = htn_planner_ron::compile(
        &"x".repeat(1025),
        "(",
        Schema {
            references: vec![String::new()],
            ..Schema::default()
        },
        Limits::small(),
    )
    .expect_err("invalid schema");
    assert_eq!(error.locator.source, "<oversized source>");
    assert_eq!(error.locator.declaration, "<schema>");
}

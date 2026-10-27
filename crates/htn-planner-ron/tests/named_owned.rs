//! Rejected caller-owned trees need bounded traversal and stack-safe cleanup.
use htn_planner::{Comparison, Value};
use htn_planner_ron::authoring::{Condition, Definition, Expression, Method, Step, Task};
fn definition(step: Step) -> Definition {
    Definition {
        version: 2,
        name: "owned".into(),
        entries: vec!["entry".into()],
        queries: vec![],
        tasks: vec![Task {
            name: "entry".into(),
            parameters: vec![],
            outputs: vec![],
            methods: vec![Method {
                name: "method".into(),
                locals: vec![],
                result: vec![],
                steps: vec![step],
            }],
        }],
    }
}
#[test]
#[test_tag::tag(invariant)]
fn excessive_programmatic_condition_depth_rejects_and_disposes_owned_tree() {
    let mut condition = Condition::Compare {
        op: Comparison::Equal,
        left: Expression::Literal(Value::Int(1)),
        right: Expression::Literal(Value::Int(1)),
    };
    for _ in 0..20_000 {
        condition = Condition::Not(Box::new(condition));
    }
    assert!(
        definition(Step::Condition(condition))
            .lower("owned")
            .is_err()
    );
}
#[test]
#[test_tag::tag(invariant)]
fn excessive_programmatic_literal_depth_rejects_and_disposes_owned_tree() {
    let mut value = Value::Int(1);
    for _ in 0..20_000 {
        value = Value::Record {
            type_name: "R".into(),
            fields: vec![value],
        };
    }
    assert!(
        definition(Step::Action {
            name: "emit".into(),
            arguments: vec![Expression::Literal(value)]
        })
        .lower("owned")
        .is_err()
    );
}

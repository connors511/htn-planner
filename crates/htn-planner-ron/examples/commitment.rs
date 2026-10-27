use htn_planner::{
    CompiledDomain, Condition, Definition, Expression, Fact, FactSnapshot, Limits, Method, Schema,
    Signature, Step, Task, Type, Value, plan,
};
use std::io::Write;
fn run(first_success: bool) -> Result<(), String> {
    let schema = Schema {
        predicates: vec![
            Signature {
                name: "candidate".into(),
                arguments: vec![Type::Int],
            },
            Signature {
                name: "accepted".into(),
                arguments: vec![Type::Int],
            },
        ],
        actions: vec![Signature {
            name: "select".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    };
    let child = |actor| {
        Condition::And(vec![
            Condition::Fact {
                name: "candidate".into(),
                arguments: vec![Expression::Literal(Value::Int(actor))],
            },
            Condition::Fact {
                name: "accepted".into(),
                arguments: vec![Expression::Local(0)],
            },
        ])
    };
    // The first child yields actor1. Only actor2 satisfies the later caller step.
    let child = |actor| {
        Condition::And(vec![
            child(actor),
            Condition::Compare {
                op: htn_planner::Comparison::Equal,
                left: Expression::Local(0),
                right: Expression::Literal(Value::Int(actor)),
            },
        ])
    };
    let children = vec![child(1), child(2)];
    let condition = if first_success {
        Condition::FirstSuccess(children)
    } else {
        Condition::Or(children)
    };
    let definition = Definition {
        version: 1,
        name: "commitment".into(),
        queries: vec![],
        entries: vec!["entry".into()],
        tasks: vec![Task {
            name: "entry".into(),
            parameters: vec![],
            outputs: vec![],
            methods: vec![Method {
                name: "select".into(),
                locals: vec![Type::Int],
                result: vec![],
                steps: vec![
                    Step::Condition(condition),
                    Step::Condition(Condition::Compare {
                        op: htn_planner::Comparison::Equal,
                        left: Expression::Local(0),
                        right: Expression::Literal(Value::Int(2)),
                    }),
                    Step::Action {
                        name: "select".into(),
                        arguments: vec![Expression::Local(0)],
                    },
                ],
            }],
        }],
    };
    let domain = CompiledDomain::admit("commitment", schema.clone(), definition, Limits::small())
        .map_err(|e| e.message)?;
    let facts = FactSnapshot::new(
        &schema,
        [
            ("candidate", 1),
            ("candidate", 2),
            ("accepted", 1),
            ("accepted", 2),
        ]
        .into_iter()
        .map(|(name, value)| Fact {
            predicate: name.into(),
            arguments: vec![Value::Int(value)],
        })
        .collect(),
    )?;
    let result = plan(&domain, &facts, "entry", &[]);
    writeln!(
        std::io::stdout(),
        "{}: {:?}",
        if first_success { "FirstSuccess" } else { "Or" },
        result.outcome
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}
fn main() -> Result<(), String> {
    run(false)?;
    run(true)
}

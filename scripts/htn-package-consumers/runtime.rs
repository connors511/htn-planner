//! Standalone typed runtime caller and shared native/WASM identity fixture.
use htn_planner::{
    CompiledDomain, Condition, Definition, DerivedQuery, EffectOperation, Expression, Fact,
    FactSnapshot, Limits, Method, Outcome, PlanEntry, PlanningEffect, RecordType, Schema,
    Signature, Step, Task, Type, Value, expand_deferred, plan,
};
use std::io::Write;

pub fn identity() -> Result<[u8; 32], String> {
    let parcel = Type::Ref("Parcel".into());
    let bay = Type::Record("Bay".into());
    let signature = |name: &str, arguments| Signature {
        name: name.into(),
        arguments,
    };
    let schema = Schema {
        query_inputs: vec![],
        references: vec!["Parcel".into()],
        records: vec![RecordType {
            name: "Bay".into(),
            fields: vec![Type::Int, Type::Bool],
        }],
        predicates: vec![
            signature("ready", vec![parcel.clone(), bay.clone()]),
            signature("remembered", vec![Type::Fixed(18), Type::Int, Type::Symbol]),
        ],
        actions: vec![
            signature("dispatch", vec![parcel.clone(), bay.clone()]),
            signature("finish", vec![Type::Int]),
        ],
        effects: vec![PlanningEffect {
            name: "remember".into(),
            parameters: vec![Type::Fixed(18)],
            operations: vec![EffectOperation::Assert {
                predicate: "remembered".into(),
                arguments: vec![
                    Expression::Parameter(0),
                    Expression::Literal(Value::Int(i64::MIN)),
                    Expression::Literal(Value::Symbol("bay\0λ".into())),
                ],
            }],
        }],
    };
    let definition = Definition {
        version: 1,
        name: "warehouse".into(),
        entries: vec!["ship".into()],
        queries: vec![DerivedQuery {
            name: "available".into(),
            parameters: vec![parcel.clone()],
            outputs: vec![bay.clone()],
            locals: vec![bay.clone()],
            condition: Condition::And(vec![Condition::Fact {
                name: "ready".into(),
                arguments: vec![Expression::Parameter(0), Expression::Local(0)],
            }]),
            result: vec![Expression::Local(0)],
        }],
        tasks: vec![
            Task {
                name: "ship".into(),
                parameters: vec![parcel, bay.clone()],
                outputs: vec![Type::Int],
                methods: vec![Method {
                    name: "dispatch".into(),
                    locals: vec![bay],
                    result: vec![Expression::Literal(Value::Int(i64::MIN))],
                    steps: vec![
                        Step::Condition(Condition::Derived {
                            name: "available".into(),
                            arguments: vec![Expression::Parameter(0)],
                            outputs: vec![Expression::Local(0)],
                        }),
                        Step::Require {
                            name: "ready".into(),
                            arguments: vec![Expression::Parameter(0), Expression::Parameter(1)],
                        },
                        Step::Effect {
                            name: "remember".into(),
                            arguments: vec![Expression::Literal(Value::Fixed {
                                raw: i64::MAX,
                                scale: 18,
                            })],
                        },
                        Step::Action {
                            name: "dispatch".into(),
                            arguments: vec![Expression::Parameter(0), Expression::Local(0)],
                        },
                        Step::Defer {
                            name: "finish".into(),
                            arguments: vec![Expression::Literal(Value::Int(i64::MIN))],
                        },
                    ],
                }],
            },
            Task {
                name: "finish".into(),
                parameters: vec![Type::Int],
                outputs: vec![],
                methods: vec![Method {
                    name: "complete".into(),
                    locals: vec![],
                    result: vec![],
                    steps: vec![Step::Action {
                        name: "finish".into(),
                        arguments: vec![Expression::Parameter(0)],
                    }],
                }],
            },
        ],
    };
    let domain = CompiledDomain::admit(
        "supplied-source",
        schema.clone(),
        definition.clone(),
        Limits::small(),
    )
    .map_err(|e| e.message)?;
    let equivalent = CompiledDomain::admit(
        "relocated-source",
        schema.clone(),
        definition,
        Limits::small(),
    )
    .map_err(|e| e.message)?;
    if domain.identity() != equivalent.identity() {
        return Err("source relocation changed semantic identity".into());
    }
    let arguments = vec![
        Value::Ref {
            type_name: "Parcel".into(),
            key: u64::MAX,
        },
        Value::Record {
            type_name: "Bay".into(),
            fields: vec![Value::Int(i64::MIN), Value::Bool(true)],
        },
    ];
    let facts = FactSnapshot::new(
        &schema,
        vec![Fact {
            predicate: "ready".into(),
            arguments: arguments.clone(),
        }],
    )?;
    let before = facts.clone();
    let first = plan(&domain, &facts, "ship", &arguments);
    if first != plan(&domain, &facts, "ship", &arguments) {
        return Err("nondeterministic repeated plan".into());
    }
    let Outcome::Success(entries) = first.outcome else {
        return Err("typed runtime plan failed".into());
    };
    if entries.len() != 2
        || !entries[0]
            .as_action()
            .is_some_and(|a| a.arguments == arguments)
        || facts != before
    {
        return Err("typed values or speculative isolation changed".into());
    }
    let PlanEntry::Deferred(token) = &entries[1] else {
        return Err("missing captured deferred input".into());
    };
    let Outcome::Success(expanded) = expand_deferred(&domain, &facts, token).outcome else {
        return Err("runtime deferred expansion failed".into());
    };
    if expanded.len() != 1
        || !expanded[0]
            .as_action()
            .is_some_and(|a| a.arguments == vec![Value::Int(i64::MIN)])
    {
        return Err("deferred extreme input changed".into());
    }
    Ok(*domain.identity().as_bytes())
}

fn main() -> Result<(), String> {
    let hex = identity()?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    writeln!(std::io::stdout(), "{hex}").map_err(|e| e.to_string())
}

use htn_planner::{
    EffectOperation, Expression, Fact, Limits, PlanningEffect, RecordType, Schema, Signature, Type,
    Value,
};
pub fn reference(kind: &str, key: u64) -> Value {
    Value::Ref {
        type_name: kind.into(),
        key,
    }
}
pub fn role(name: &str) -> Value {
    Value::Record {
        type_name: "Role".into(),
        fields: vec![Value::Symbol(name.into())],
    }
}
pub fn bay(number: i64) -> Value {
    Value::Record {
        type_name: "Bay".into(),
        fields: vec![Value::Int(number)],
    }
}
pub const fn limits() -> Limits {
    Limits {
        work: 200_000,
        storage: 131_072,
        depth: 32,
        plan: 16,
        bindings: 256,
        diagnostics: 16,
    }
}
pub fn schema() -> Schema {
    let worker = Type::Ref("Worker".into());
    let role = Type::Record("Role".into());
    let parcel = Type::Ref("Parcel".into());
    let bay = Type::Record("Bay".into());
    let signature = |name: &str, arguments| Signature {
        name: name.into(),
        arguments,
    };
    let operations = || vec![Expression::Parameter(0), Expression::Parameter(1)];
    Schema {
        query_inputs: vec![],
        references: vec!["Parcel".into(), "Worker".into()],
        records: vec![
            RecordType {
                name: "Role".into(),
                fields: vec![Type::Symbol],
            },
            RecordType {
                name: "Bay".into(),
                fields: vec![Type::Int],
            },
        ],
        predicates: vec![
            signature("eligible", vec![worker.clone(), role.clone()]),
            signature("assigned", vec![worker.clone(), role.clone()]),
            signature("ready", vec![parcel.clone(), bay.clone()]),
        ],
        actions: vec![
            signature("assign", vec![parcel.clone(), worker.clone(), role.clone()]),
            signature("deliver", vec![parcel, bay]),
        ],
        effects: vec![
            PlanningEffect {
                name: "claim".into(),
                parameters: vec![worker.clone(), role.clone()],
                operations: vec![EffectOperation::Assert {
                    predicate: "assigned".into(),
                    arguments: operations(),
                }],
            },
            PlanningEffect {
                name: "release".into(),
                parameters: vec![worker, role],
                operations: vec![EffectOperation::Retract {
                    predicate: "assigned".into(),
                    arguments: operations(),
                }],
            },
        ],
    }
}
pub fn facts(parcel: &Value) -> Vec<Fact> {
    let mut rows = [(1, "front"), (2, "front"), (3, "left"), (1, "right")]
        .into_iter()
        .map(|(worker, name)| Fact {
            predicate: "eligible".into(),
            arguments: vec![reference("Worker", worker), role(name)],
        })
        .collect::<Vec<_>>();
    rows.push(Fact {
        predicate: "assigned".into(),
        arguments: vec![reference("Worker", 9), role("bench")],
    });
    rows.push(Fact {
        predicate: "ready".into(),
        arguments: vec![parcel.clone(), bay(23)],
    });
    rows
}

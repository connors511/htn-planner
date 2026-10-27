//! Small independent public contract cases, deliberately separate from allocation content.
use super::fixture;
use htn_planner::{
    Comparison, CompiledDomain, Condition, Definition, Expression, Fact, FactSnapshot, Limits,
    Method, NumericOperation, Outcome, Schema, Signature, Step, Task, Type, Value, plan,
};

fn admit(schema: Schema, definition: Definition, limits: Limits) -> Result<CompiledDomain, String> {
    CompiledDomain::admit("contract", schema, definition, limits).map_err(|e| e.message)
}
fn definition(steps: Vec<Step>, locals: Vec<Type>) -> Definition {
    Definition {
        version: 1,
        name: "contract".into(),
        queries: vec![],
        entries: vec!["entry".into()],
        tasks: vec![Task {
            name: "entry".into(),
            parameters: vec![],
            outputs: vec![],
            methods: vec![Method {
                name: "run".into(),
                locals,
                result: vec![],
                steps,
            }],
        }],
    }
}
fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

pub fn run(domain: &CompiledDomain, facts: &FactSnapshot) -> Result<(), String> {
    let input = [fixture::reference("Parcel", 17)];
    let empty = FactSnapshot::new(domain.schema(), vec![])?;
    require(
        plan(domain, &empty, "ship", &input).outcome == Outcome::NoPlan,
        "absence did not yield NoPlan",
    )?;
    require(
        plan(domain, facts, "nothing", &[]).outcome == Outcome::Success(vec![]),
        "empty completion failed",
    )?;
    let other = FactSnapshot::new(&Schema::default(), vec![])?;
    require(
        plan(domain, &other, "ship", &input).outcome == Outcome::InvalidInput,
        "schema mismatch adapted silently",
    )?;
    require(
        plan(domain, facts, "ship", &[Value::Int(17)]).outcome == Outcome::InvalidInput,
        "wrong input type adapted silently",
    )?;
    for (limits, expected) in [
        (
            {
                let mut l = domain.limits();
                l.work = 1;
                l
            },
            Outcome::BudgetExhausted,
        ),
        (
            {
                let mut l = domain.limits();
                l.plan = 1;
                l
            },
            Outcome::CapacityExceeded,
        ),
    ] {
        let bounded = admit(domain.schema().clone(), domain.definition().clone(), limits)?;
        require(
            plan(&bounded, facts, "ship", &input).outcome == expected,
            "bounded failure category or prefix changed",
        )?;
    }
    let mut malformed = domain.schema().clone();
    malformed.references.push(malformed.references[0].clone());
    require(
        admit(malformed, domain.definition().clone(), domain.limits()).is_err(),
        "duplicate host schema admitted",
    )?;
    let mut unresolved = domain.definition().clone();
    unresolved.tasks[0].parameters = vec![Type::Ref("Unregistered".into())];
    require(
        admit(domain.schema().clone(), unresolved, domain.limits()).is_err(),
        "unresolved authored type admitted",
    )?;
    commitment(false, false)?;
    commitment(true, false)?;
    commitment(true, true)?;
    let overflow = definition(
        vec![Step::Condition(Condition::Calculate {
            op: NumericOperation::Add,
            left: Expression::Literal(Value::Int(i64::MAX)),
            right: Expression::Literal(Value::Int(1)),
            output: Expression::Local(0),
        })],
        vec![Type::Int],
    );
    let numeric = admit(Schema::default(), overflow, Limits::small())?;
    let facts = FactSnapshot::new(numeric.schema(), vec![])?;
    require(
        plan(&numeric, &facts, "entry", &[]).outcome == Outcome::InvalidInput,
        "numeric overflow was treated as absence",
    )
}

fn commitment(first_success: bool, multiple_witnesses: bool) -> Result<(), String> {
    let signature = |name: &str| Signature {
        name: name.into(),
        arguments: vec![Type::Int],
    };
    let schema = Schema {
        predicates: vec![signature("first"), signature("second")],
        ..Schema::default()
    };
    let child = |name: &str| Condition::Fact {
        name: name.into(),
        arguments: vec![Expression::Local(0)],
    };
    let children = vec![child("first"), child("second")];
    let condition = if first_success {
        Condition::FirstSuccess(children)
    } else {
        Condition::Or(children)
    };
    let definition = definition(
        vec![
            Step::Condition(condition),
            Step::Condition(Condition::Compare {
                op: Comparison::Equal,
                left: Expression::Local(0),
                right: Expression::Literal(Value::Int(2)),
            }),
        ],
        vec![Type::Int],
    );
    let domain = admit(schema.clone(), definition, Limits::small())?;
    let mut rows = vec![
        Fact {
            predicate: "first".into(),
            arguments: vec![Value::Int(1)],
        },
        Fact {
            predicate: "second".into(),
            arguments: vec![Value::Int(2)],
        },
    ];
    if multiple_witnesses {
        rows.push(Fact {
            predicate: "first".into(),
            arguments: vec![Value::Int(2)],
        });
    }
    let facts = FactSnapshot::new(&schema, rows)?;
    let expected = if first_success && !multiple_witnesses {
        Outcome::NoPlan
    } else {
        Outcome::Success(vec![])
    };
    require(
        plan(&domain, &facts, "entry", &[]).outcome == expected,
        "Or/selected-child witness commitment changed",
    )
}

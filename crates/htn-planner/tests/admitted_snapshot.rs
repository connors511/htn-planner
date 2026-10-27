//! Schema reuse preserves the public static planning and compatibility boundary.
use htn_planner::{
    CompiledDomain, Definition, Expression, Fact, FactSnapshot, Limits, Method, Schema, Signature,
    Step, Task, Type, Value, plan,
};

fn domain() -> Result<CompiledDomain, String> {
    let schema = Schema {
        predicates: vec![Signature {
            name: "ready".into(),
            arguments: vec![Type::Int],
        }],
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    };
    CompiledDomain::admit(
        "snapshot",
        schema,
        Definition {
            version: 1,
            name: "snapshot".into(),
            entries: vec!["entry".into()],
            queries: vec![],
            tasks: vec![Task {
                name: "entry".into(),
                parameters: vec![Type::Int],
                outputs: vec![],
                methods: vec![Method {
                    name: "ready".into(),
                    locals: vec![],
                    result: vec![],
                    steps: vec![
                        Step::Require {
                            name: "ready".into(),
                            arguments: vec![Expression::Parameter(0)],
                        },
                        Step::Action {
                            name: "emit".into(),
                            arguments: vec![Expression::Parameter(0)],
                        },
                    ],
                }],
            }],
        },
        Limits::small(),
    )
    .map_err(|error| error.message)
}

#[test]
#[test_tag::tag(compatibility)]
fn admitted_snapshot_preserves_equivalent_raw_schema_results_and_work() -> Result<(), String> {
    let domain = domain()?;
    let rows = vec![Fact {
        predicate: "ready".into(),
        arguments: vec![Value::Int(7)],
    }];
    let raw =
        FactSnapshot::new(domain.schema(), rows.clone()).map_err(|error| format!("{error:?}"))?;
    let reused = domain
        .snapshot(rows)
        .map_err(|error| format!("{error:?}"))?;
    let request = [Value::Int(7)];
    let expected = plan(&domain, &raw, "entry", &request);
    assert!(matches!(expected.outcome, htn_planner::Outcome::Success(_)));
    assert_eq!(plan(&domain, &reused, "entry", &request), expected);
    assert!(
        domain
            .snapshot(vec![Fact {
                predicate: "ready".into(),
                arguments: vec![Value::Bool(true)]
            }])
            .is_err()
    );
    Ok(())
}

#[test]
#[test_tag::tag(compatibility)]
fn independent_changed_schema_is_rejected_while_admitted_snapshots_remain_immutable()
-> Result<(), String> {
    let domain = domain()?;
    let mut different = domain.schema().clone();
    different.actions[0].arguments[0] = Type::Bool;
    let rows = vec![Fact {
        predicate: "ready".into(),
        arguments: vec![Value::Int(7)],
    }];
    let foreign =
        FactSnapshot::new(&different, rows.clone()).map_err(|error| format!("{error:?}"))?;
    assert!(matches!(
        plan(&domain, &foreign, "entry", &[Value::Int(7)]).outcome,
        htn_planner::Outcome::InvalidInput
    ));
    let own = domain
        .snapshot(rows)
        .map_err(|error| format!("{error:?}"))?;
    assert!(matches!(
        plan(&domain, &own, "entry", &[Value::Int(7)]).outcome,
        htn_planner::Outcome::Success(_)
    ));
    Ok(())
}

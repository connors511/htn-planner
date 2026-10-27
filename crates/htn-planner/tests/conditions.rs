use htn_planner::{
    Comparison, CompiledDomain, Condition, Definition, Expression as E, Fact, FactSnapshot, Limits,
    Method, NumericOperation, Outcome, Schema, Signature, Step, Task, Type, Value, plan,
};
fn fact(name: &str, arg: E) -> Condition {
    Condition::Fact {
        name: name.into(),
        arguments: vec![arg],
    }
}
const fn literal(value: i64) -> E {
    E::Literal(Value::Int(value))
}
fn schema() -> Schema {
    Schema {
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
            name: "emit".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    }
}
fn definition(condition: Condition) -> Definition {
    Definition {
        version: 1,
        queries: vec![],
        name: "conditions".into(),
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
                    Step::Require {
                        name: "accepted".into(),
                        arguments: vec![E::Local(0)],
                    },
                    Step::Action {
                        name: "emit".into(),
                        arguments: vec![E::Local(0)],
                    },
                ],
            }],
        }],
    }
}
fn snapshot(rows: &[(&str, i64)]) -> Result<FactSnapshot, String> {
    FactSnapshot::new(
        &schema(),
        rows.iter()
            .map(|(name, value)| Fact {
                predicate: (*name).into(),
                arguments: vec![Value::Int(*value)],
            })
            .collect(),
    )
}
fn run(condition: Condition, rows: &[(&str, i64)]) -> Result<htn_planner::PlanningResult, String> {
    let domain = CompiledDomain::admit(
        "conditions",
        schema(),
        definition(condition),
        Limits::small(),
    )
    .map_err(|e| e.message)?;
    Ok(plan(&domain, &snapshot(rows)?, "entry", &[]))
}

#[test_tag::tag(unit)]
#[test]
fn enumerating_or_reopens_later_child_but_first_success_commits_child() {
    let choices = || vec![fact("candidate", literal(1)), fact("candidate", literal(2))];
    // Bind the output through each branch; later Require accepts only second branch.
    let branches = || {
        choices()
            .into_iter()
            .enumerate()
            .map(|(index, condition)| {
                Condition::And(vec![
                    condition,
                    Condition::Calculate {
                        op: NumericOperation::Add,
                        left: literal(i64::try_from(index).unwrap() + 1),
                        right: literal(0),
                        output: E::Local(0),
                    },
                ])
            })
            .collect()
    };
    let rows = [("candidate", 1), ("candidate", 2), ("accepted", 2)];
    assert!(matches!(
        run(Condition::Or(branches()), &rows).unwrap().outcome,
        Outcome::Success(_)
    ));
    assert_eq!(
        run(Condition::FirstSuccess(branches()), &rows)
            .unwrap()
            .outcome,
        Outcome::NoPlan
    );
}
#[test_tag::tag(unit)]
#[test]
fn first_success_preserves_all_witnesses_inside_selected_child() {
    let condition = Condition::FirstSuccess(vec![
        fact("candidate", E::Local(0)),
        Condition::Calculate {
            op: NumericOperation::Add,
            left: literal(3),
            right: literal(0),
            output: E::Local(0),
        },
    ]);
    let result = run(
        condition,
        &[("candidate", 1), ("candidate", 2), ("accepted", 2)],
    )
    .unwrap();
    let Outcome::Success(entries) = result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(
        entries[0].as_action().unwrap().arguments,
        vec![Value::Int(2)]
    );
}
#[test_tag::tag(unit)]
#[test]
fn first_success_advances_past_child_with_no_witness() {
    let condition = Condition::FirstSuccess(vec![
        Condition::And(vec![
            fact("candidate", literal(9)),
            Condition::Calculate {
                op: NumericOperation::Add,
                left: literal(9),
                right: literal(0),
                output: E::Local(0),
            },
        ]),
        fact("candidate", E::Local(0)),
    ]);
    assert!(matches!(
        run(condition, &[("candidate", 2), ("accepted", 2)])
            .unwrap()
            .outcome,
        Outcome::Success(_)
    ));
}
#[test_tag::tag(invariant)]
#[test]
fn unsafe_negation_and_non_definite_disjunction_binding_are_rejected() {
    assert!(
        CompiledDomain::admit(
            "unsafe",
            schema(),
            definition(Condition::Not(Box::new(fact("candidate", E::Local(0))))),
            Limits::small()
        )
        .is_err()
    );
    assert!(
        CompiledDomain::admit(
            "unsafe",
            schema(),
            definition(Condition::Or(vec![
                fact("candidate", E::Local(0)),
                Condition::And(vec![])
            ])),
            Limits::small()
        )
        .is_err()
    );
}
#[test_tag::tag(unit)]
#[test]
fn checked_arithmetic_overflow_is_fatal_even_inside_negation_or_alternatives() {
    let overflow = Condition::Calculate {
        op: NumericOperation::Add,
        left: literal(i64::MAX),
        right: literal(1),
        output: literal(0),
    };
    let condition = Condition::And(vec![
        fact("candidate", E::Local(0)),
        Condition::Or(vec![
            Condition::Not(Box::new(overflow)),
            Condition::And(vec![]),
        ]),
    ]);
    let result = run(condition, &[("candidate", 2), ("accepted", 2)]).unwrap();
    assert_eq!(result.outcome, Outcome::InvalidInput);
    assert!(
        result
            .diagnostic
            .iter()
            .any(|d| d.message.contains("overflow"))
    );
}
#[test_tag::tag(unit)]
#[test]
fn signed_comparison_and_subtraction_bind_grounded_outputs() {
    let condition = Condition::And(vec![
        Condition::Compare {
            op: Comparison::Less,
            left: literal(-4),
            right: literal(-1),
        },
        Condition::Calculate {
            op: NumericOperation::Subtract,
            left: literal(-4),
            right: literal(-6),
            output: E::Local(0),
        },
    ]);
    assert!(matches!(
        run(condition, &[("accepted", 2)]).unwrap().outcome,
        Outcome::Success(_)
    ));
}
#[test_tag::tag(invariant)]
#[test]
fn negation_propagates_work_and_storage_exhaustion_without_a_prefix() {
    let condition = Condition::And(vec![
        fact("candidate", E::Local(0)),
        Condition::Not(Box::new(fact("accepted", literal(999)))),
    ]);
    let raw = definition(condition);
    let facts = snapshot(&[("candidate", 2), ("accepted", 2)]).unwrap();
    let mut limits = Limits::small();
    let baseline = CompiledDomain::admit("limits", schema(), raw.clone(), limits).unwrap();
    let result = plan(&baseline, &facts, "entry", &[]);
    assert!(matches!(result.outcome, Outcome::Success(_)));
    limits.work = result.work.consumed - 1;
    let domain = CompiledDomain::admit("limits", schema(), raw.clone(), limits).unwrap();
    assert_eq!(
        plan(&domain, &facts, "entry", &[]).outcome,
        Outcome::BudgetExhausted
    );
    limits = Limits::small();
    limits.storage = result.work.peak_storage - 1;
    let domain = CompiledDomain::admit("limits", schema(), raw, limits).unwrap();
    assert_eq!(
        plan(&domain, &facts, "entry", &[]).outcome,
        Outcome::CapacityExceeded
    );
}

#[test_tag::tag(unit)]
#[test]
fn first_success_commit_does_not_cut_outer_fact_choices() {
    let condition = Condition::And(vec![
        fact("candidate", E::Local(0)),
        Condition::FirstSuccess(vec![Condition::And(vec![]), Condition::And(vec![])]),
    ]);
    let result = run(
        condition,
        &[("candidate", 1), ("candidate", 2), ("accepted", 2)],
    )
    .unwrap();
    let Outcome::Success(entries) = result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(
        entries[0].as_action().unwrap().arguments,
        vec![Value::Int(2)]
    );
}
#[test_tag::tag(invariant)]
#[test]
fn numeric_scale_and_type_mismatches_reject_admission() {
    for right in [
        E::Literal(Value::Fixed { raw: 1, scale: 3 }),
        literal(1),
        E::Literal(Value::Bool(true)),
    ] {
        let condition = Condition::And(vec![
            fact("candidate", E::Local(0)),
            Condition::Compare {
                op: Comparison::Equal,
                left: E::Literal(Value::Fixed { raw: 1, scale: 2 }),
                right,
            },
        ]);
        assert!(
            CompiledDomain::admit("numeric", schema(), definition(condition), Limits::small())
                .is_err()
        );
    }
}
#[test_tag::tag(unit)]
#[test]
fn fixed_arithmetic_preserves_scale_and_bound_output_conflict_backtracks() {
    let condition = Condition::And(vec![
        fact("candidate", E::Local(0)),
        Condition::Calculate {
            op: NumericOperation::Subtract,
            left: E::Literal(Value::Fixed { raw: -8, scale: 2 }),
            right: E::Literal(Value::Fixed { raw: -10, scale: 2 }),
            output: E::Literal(Value::Fixed { raw: 2, scale: 2 }),
        },
        Condition::Or(vec![
            Condition::Calculate {
                op: NumericOperation::Add,
                left: literal(1),
                right: literal(0),
                output: E::Local(0),
            },
            Condition::Calculate {
                op: NumericOperation::Add,
                left: literal(2),
                right: literal(0),
                output: E::Local(0),
            },
        ]),
    ]);
    assert!(matches!(
        run(condition, &[("candidate", 2), ("accepted", 2)])
            .unwrap()
            .outcome,
        Outcome::Success(_)
    ));
}

#[test_tag::tag(unit)]
#[test]
fn control_diagnostic_locates_originating_condition_instead_of_next_action() {
    let condition = Condition::And(vec![
        fact("candidate", E::Local(0)),
        Condition::Calculate {
            op: NumericOperation::Add,
            left: literal(i64::MAX),
            right: literal(1),
            output: literal(0),
        },
    ]);
    let mut raw = definition(condition);
    raw.tasks[0].methods[0].steps.remove(1);
    let domain = CompiledDomain::admit("source-location", schema(), raw, Limits::small()).unwrap();
    let result = plan(
        &domain,
        &snapshot(&[("candidate", 1)]).unwrap(),
        "entry",
        &[],
    );
    assert_eq!(result.outcome, Outcome::InvalidInput);
    let diagnostic = result.diagnostic.unwrap();
    assert_eq!(diagnostic.locator.task.as_deref(), Some("entry"));
    assert_eq!(diagnostic.locator.declaration, "select");
    assert_eq!(diagnostic.locator.step, Some(0));
}

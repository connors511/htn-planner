use super::*;

#[test]
#[test_tag::tag(invariant)]
fn failed_effect_branch_restores_lazy_rows_without_rerunning_provider() -> Result<(), String> {
    let original = domain()?;
    let mut schema = original.schema().clone();
    schema.effects.push(htn_planner::PlanningEffect {
        name: "remove".into(),
        parameters: vec![Type::Int, Type::Int],
        operations: vec![htn_planner::EffectOperation::Retract {
            predicate: "candidate".into(),
            arguments: vec![E::Parameter(0), E::Parameter(1)],
        }],
    });
    let mut definition = original.definition().clone();
    definition.tasks[0].methods[0].steps = vec![
        Step::Effect {
            name: "remove".into(),
            arguments: vec![E::Parameter(0), E::Literal(Value::Int(5))],
        },
        Step::Require {
            name: "usable".into(),
            arguments: vec![E::Literal(Value::Int(999))],
        },
    ];
    definition.tasks[0].methods.push(Method {
        name: "fallback".into(),
        locals: vec![Type::Int],
        result: vec![],
        steps: vec![
            Step::Query {
                name: "candidate".into(),
                arguments: vec![E::Parameter(0), E::Local(0)],
            },
            Step::Action {
                name: "emit".into(),
                arguments: vec![E::Local(0)],
            },
        ],
    });
    let domain = CompiledDomain::admit("rollback", schema, definition, Limits::small())
        .map_err(|error| format!("{error:?}"))?;
    let facts = domain
        .snapshot(vec![])
        .map_err(|error| format!("{error:?}"))?;
    let provider = Candidates(Cell::new(0));
    let result = plan_with_queries(&domain, &facts, &provider, "entry", &[Value::Int(3)]);
    let Outcome::Success(entries) = result.outcome else {
        return Err(format!("fallback must see restored first row: {result:?}"));
    };
    assert_eq!(
        entries[0]
            .as_action()
            .ok_or("expected concrete action")?
            .arguments,
        vec![Value::Int(5)]
    );
    assert_eq!(provider.0.get(), 1);
    assert_eq!(
        facts,
        domain
            .snapshot(vec![])
            .map_err(|error| format!("{error:?}"))?
    );
    Ok(())
}

#[test]
#[test_tag::tag(invariant)]
fn another_prefix_materialization_does_not_reorder_parent_candidates() -> Result<(), String> {
    struct Tree(std::cell::RefCell<Vec<i64>>);
    impl QueryProvider for Tree {
        fn query(
            &self,
            _: &str,
            inputs: &[Value],
            budget: &mut QueryBudget<'_>,
        ) -> Result<Vec<Vec<Value>>, QueryError> {
            let Value::Int(input) = inputs[0] else {
                return Err(QueryError::InvalidInput);
            };
            budget.charge(16)?;
            budget.reserve(32)?;
            self.0.borrow_mut().push(input);
            Ok(match input {
                3 => vec![
                    vec![Value::Int(3), Value::Int(5)],
                    vec![Value::Int(3), Value::Int(7)],
                ],
                7 => vec![vec![Value::Int(7), Value::Int(9)]],
                _ => vec![],
            })
        }
    }
    let original = domain()?;
    let mut definition = original.definition().clone();
    definition.tasks[0].methods[0].locals.push(Type::Int);
    definition.tasks[0].methods[0].steps.insert(
        1,
        Step::Query {
            name: "candidate".into(),
            arguments: vec![E::Local(0), E::Local(1)],
        },
    );
    definition.tasks[0].methods[0].steps[2] = Step::Require {
        name: "usable".into(),
        arguments: vec![E::Local(1)],
    };
    definition.tasks[0].methods[0].steps[3] = Step::Action {
        name: "emit".into(),
        arguments: vec![E::Local(1)],
    };
    let domain = CompiledDomain::admit(
        "prefix-order",
        original.schema().clone(),
        definition,
        Limits::small(),
    )
    .map_err(|error| format!("{error:?}"))?;
    let facts = domain
        .snapshot(vec![Fact {
            predicate: "usable".into(),
            arguments: vec![Value::Int(9)],
        }])
        .map_err(|error| format!("{error:?}"))?;
    let provider = Tree(std::cell::RefCell::new(vec![]));
    let result = plan_with_queries(&domain, &facts, &provider, "entry", &[Value::Int(3)]);
    let Outcome::Success(entries) = result.outcome else {
        return Err(format!(
            "second parent candidate must remain available: {result:?}"
        ));
    };
    assert_eq!(
        entries[0]
            .as_action()
            .ok_or("expected concrete action")?
            .arguments,
        vec![Value::Int(9)]
    );
    assert_eq!(*provider.0.borrow(), vec![3, 5, 7]);
    Ok(())
}

#[test]
#[test_tag::tag(invariant)]
fn retained_prefix_cache_is_included_in_the_shared_storage_allowance() -> Result<(), String> {
    struct Prefix;
    impl QueryProvider for Prefix {
        fn query(
            &self,
            _: &str,
            inputs: &[Value],
            budget: &mut QueryBudget<'_>,
        ) -> Result<Vec<Vec<Value>>, QueryError> {
            budget.charge(8)?;
            budget.reserve(8)?;
            Ok(vec![vec![inputs[0].clone(), Value::Int(0)]])
        }
    }
    let original = domain()?;
    let mut definition = original.definition().clone();
    let require = |n| Step::Require {
        name: "candidate".into(),
        arguments: vec![E::Literal(Value::Int(n)), E::Literal(Value::Int(0))],
    };
    definition.tasks[0].methods[0].steps = vec![
        require(0),
        Step::Action {
            name: "emit".into(),
            arguments: vec![E::Literal(Value::Int(0))],
        },
    ];
    let first = CompiledDomain::admit(
        "cache",
        original.schema().clone(),
        definition.clone(),
        Limits::small(),
    )
    .map_err(|error| format!("{error:?}"))?;
    let report = plan_with_queries(
        &first,
        &first
            .snapshot(vec![])
            .map_err(|error| format!("{error:?}"))?,
        &Prefix,
        "entry",
        &[Value::Int(3)],
    );
    assert!(matches!(report.outcome, Outcome::Success(_)));
    definition.tasks[0].methods[0]
        .steps
        .splice(0..1, (0..32).map(require));
    let limited = CompiledDomain::admit(
        "cache",
        original.schema().clone(),
        definition,
        Limits {
            storage: report.work.peak_storage + 1,
            ..Limits::small()
        },
    )
    .map_err(|error| format!("{error:?}"))?;
    let exhausted = plan_with_queries(
        &limited,
        &limited
            .snapshot(vec![])
            .map_err(|error| format!("{error:?}"))?,
        &Prefix,
        "entry",
        &[Value::Int(3)],
    );
    assert!(
        matches!(exhausted.outcome, Outcome::CapacityExceeded),
        "retained prefixes must not become invisible storage: {exhausted:?}"
    );
    Ok(())
}

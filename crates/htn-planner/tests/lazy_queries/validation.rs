use super::*;

struct Rows(Vec<Vec<Value>>);
impl QueryProvider for Rows {
    fn query(
        &self,
        _: &str,
        _: &[Value],
        budget: &mut QueryBudget<'_>,
    ) -> Result<Vec<Vec<Value>>, QueryError> {
        budget.charge(256)?;
        budget.reserve(256)?;
        Ok(self.0.clone())
    }
}
#[test]
#[test_tag::tag(invariant)]
fn malformed_rows_abort_the_whole_batch_before_any_usable_prefix() -> Result<(), String> {
    let domain = domain()?;
    let facts = domain
        .snapshot(vec![Fact {
            predicate: "usable".into(),
            arguments: vec![Value::Int(7)],
        }])
        .map_err(|error| format!("{error:?}"))?;
    for malformed in [
        vec![Value::Int(4), Value::Int(7)],
        vec![Value::Int(3), Value::Bool(true)],
        vec![Value::Int(3)],
    ] {
        let provider = Rows(vec![vec![Value::Int(3), Value::Int(7)], malformed]);
        let result = plan_with_queries(&domain, &facts, &provider, "entry", &[Value::Int(3)]);
        assert!(
            matches!(result.outcome, Outcome::InvalidInput),
            "{result:?}"
        );
    }
    assert!(matches!(
        plan_with_queries(&domain, &facts, &Rows(vec![]), "entry", &[Value::Int(3)]).outcome,
        Outcome::NoPlan
    ));
    Ok(())
}
struct IgnoresFailure(bool);
impl QueryProvider for IgnoresFailure {
    fn query(
        &self,
        _: &str,
        _: &[Value],
        budget: &mut QueryBudget<'_>,
    ) -> Result<Vec<Vec<Value>>, QueryError> {
        if self.0 {
            assert!(budget.charge(usize::MAX).is_err());
        } else {
            assert!(budget.reserve(usize::MAX).is_err());
        }
        Ok(vec![])
    }
}
#[test]
#[test_tag::tag(invariant)]
fn ignored_provider_budget_errors_are_sticky_and_not_ordinary_absence() -> Result<(), String> {
    let domain = domain()?;
    let facts = domain
        .snapshot(vec![])
        .map_err(|error| format!("{error:?}"))?;
    let work = plan_with_queries(
        &domain,
        &facts,
        &IgnoresFailure(true),
        "entry",
        &[Value::Int(3)],
    );
    assert!(matches!(work.outcome, Outcome::BudgetExhausted));
    let storage = plan_with_queries(
        &domain,
        &facts,
        &IgnoresFailure(false),
        "entry",
        &[Value::Int(3)],
    );
    assert!(matches!(storage.outcome, Outcome::CapacityExceeded));
    Ok(())
}
#[test]
#[test_tag::tag(invariant)]
fn query_prefix_declarations_and_unbound_calls_are_rejected_at_admission() -> Result<(), String> {
    let domain = domain()?;
    let inputs = domain.schema().query_inputs.clone();
    for bad in [
        vec![QueryInputs {
            predicate: "unknown".into(),
            bound: 0,
        }],
        vec![QueryInputs {
            predicate: "candidate".into(),
            bound: 3,
        }],
        vec![inputs[0].clone(), inputs[0].clone()],
    ] {
        let mut schema = domain.schema().clone();
        schema.query_inputs = bad;
        assert!(
            CompiledDomain::admit(
                "bad-prefix",
                schema,
                domain.definition().clone(),
                Limits::small()
            )
            .is_err()
        );
    }
    let mut definition = domain.definition().clone();
    definition.tasks[0].methods[0].steps[0] = Step::Query {
        name: "candidate".into(),
        arguments: vec![E::Local(0), E::Parameter(0)],
    };
    assert!(
        CompiledDomain::admit(
            "unbound",
            domain.schema().clone(),
            definition.clone(),
            Limits::small()
        )
        .is_err()
    );
    definition.tasks[0].methods[0].steps[0] = Step::Condition(htn_planner::Condition::Fact {
        name: "candidate".into(),
        arguments: vec![E::Local(0), E::Parameter(0)],
    });
    assert!(
        CompiledDomain::admit(
            "unbound-condition",
            domain.schema().clone(),
            definition,
            Limits::small()
        )
        .is_err()
    );
    let mut schema = domain.schema().clone();
    schema.query_inputs[0].bound = 0;
    let whole = CompiledDomain::admit(
        "whole",
        schema,
        domain.definition().clone(),
        Limits::small(),
    )
    .map_err(|error| format!("{error:?}"))?;
    assert_ne!(
        whole.identity(),
        domain.identity(),
        "required input prefix changes admitted query semantics"
    );
    Ok(())
}
#[test]
#[test_tag::tag(invariant)]
fn deeply_malformed_provider_payload_is_disposed_at_the_owned_boundary() -> Result<(), String> {
    struct Deep;
    impl QueryProvider for Deep {
        fn query(
            &self,
            _: &str,
            _: &[Value],
            budget: &mut QueryBudget<'_>,
        ) -> Result<Vec<Vec<Value>>, QueryError> {
            // Deliberately violate the provider contract to test fail-closed owned cleanup.
            budget.reserve(32)?;
            let mut value = Value::Int(0);
            for _ in 0..10_000 {
                value = Value::Record {
                    type_name: "invalid".into(),
                    fields: vec![value],
                };
            }
            Ok(vec![vec![Value::Int(3), value]])
        }
    }
    let domain = domain()?;
    let facts = domain
        .snapshot(vec![])
        .map_err(|error| format!("{error:?}"))?;
    assert!(matches!(
        plan_with_queries(&domain, &facts, &Deep, "entry", &[Value::Int(3)]).outcome,
        Outcome::InvalidInput
    ));
    Ok(())
}

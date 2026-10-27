use super::*;

struct Fresh {
    value: i64,
    calls: Cell<usize>,
}
impl QueryProvider for Fresh {
    fn query(
        &self,
        _: &str,
        inputs: &[Value],
        budget: &mut QueryBudget<'_>,
    ) -> Result<Vec<Vec<Value>>, QueryError> {
        budget.charge(8)?;
        budget.reserve(32)?;
        self.calls.set(self.calls.get() + 1);
        Ok(vec![vec![inputs[0].clone(), Value::Int(self.value)]])
    }
}
#[test]
#[test_tag::tag(invariant)]
fn deferred_expansion_uses_fresh_provider_and_tracing_never_demands_early_rows()
-> Result<(), String> {
    let original = domain()?;
    let mut definition = original.definition().clone();
    let mut later = definition.tasks[0].clone();
    later.name = "later".into();
    definition.tasks[0].methods[0].steps = vec![Step::Defer {
        name: "later".into(),
        arguments: vec![E::Parameter(0)],
    }];
    definition.tasks.push(later);
    let domain = CompiledDomain::admit(
        "deferred",
        original.schema().clone(),
        definition,
        Limits {
            diagnostics: 4096,
            ..Limits::small()
        },
    )
    .map_err(|error| format!("{error:?}"))?;
    let empty = domain
        .snapshot(vec![])
        .map_err(|error| format!("{error:?}"))?;
    let provider = Fresh {
        value: 5,
        calls: Cell::new(0),
    };
    let root = htn_planner::plan_traced_with_queries(
        &domain,
        &empty,
        &provider,
        "entry",
        &[Value::Int(3)],
        4096,
    );
    assert_eq!(provider.calls.get(), 0);
    assert!(
        !root
            .trace
            .nodes
            .iter()
            .any(|n| n.event == htn_planner::TraceEvent::QueryCandidate)
    );
    let Outcome::Success(entries) = root.result.outcome else {
        return Err("defer must be inert".into());
    };
    let [htn_planner::PlanEntry::Deferred(token)] = entries.as_slice() else {
        return Err("expected deferred boundary".into());
    };
    for value in [5, 7] {
        let facts = domain
            .snapshot(vec![Fact {
                predicate: "usable".into(),
                arguments: vec![Value::Int(value)],
            }])
            .map_err(|error| format!("{error:?}"))?;
        let fresh = Fresh {
            value,
            calls: Cell::new(0),
        };
        let expanded = htn_planner::expand_deferred_with_queries(&domain, &facts, &fresh, token);
        let traced =
            htn_planner::expand_deferred_traced_with_queries(&domain, &facts, &fresh, token, 4096);
        assert_eq!(expanded, traced.result);
        let Outcome::Success(entries) = expanded.outcome else {
            return Err("fresh value must be usable".into());
        };
        assert_eq!(
            entries[0]
                .as_action()
                .ok_or("expected concrete action")?
                .arguments,
            vec![Value::Int(value)]
        );
        assert_eq!(
            fresh.calls.get(),
            2,
            "each expansion is a fresh attempt, traced or untraced"
        );
    }
    Ok(())
}

#[test]
#[test_tag::tag(invariant)]
fn truncated_trace_keeps_lazy_result_work_and_provider_calls_unchanged() -> Result<(), String> {
    let domain = domain()?;
    let facts = domain
        .snapshot(vec![Fact {
            predicate: "usable".into(),
            arguments: vec![Value::Int(7)],
        }])
        .map_err(|error| format!("{error:?}"))?;
    let provider = Candidates(Cell::new(0));
    let expected = plan_with_queries(&domain, &facts, &provider, "entry", &[Value::Int(3)]);
    assert!(matches!(expected.outcome, Outcome::Success(_)));
    for capacity in [0, 1, 16] {
        let provider = Candidates(Cell::new(0));
        let traced = htn_planner::plan_traced_with_queries(
            &domain,
            &facts,
            &provider,
            "entry",
            &[Value::Int(3)],
            capacity,
        );
        assert_eq!(traced.result, expected);
        assert_eq!(provider.0.get(), 1);
        assert!(traced.trace.truncated);
        assert!(traced.trace.retained_units <= capacity);
        if capacity == 0 {
            assert!(traced.trace.nodes.is_empty());
        }
    }
    Ok(())
}

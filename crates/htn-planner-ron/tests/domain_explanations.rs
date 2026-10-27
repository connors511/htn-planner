//! Causal summaries describe actual deferred exploration, independently of trace prefixes.
use htn_planner::{
    CompiledDomain, DeferredTask, ExplainedPlanningResult, Fact, FactSnapshot, Limits, Outcome,
    PlanEntry, QueryBudget, QueryError, QueryInputs, QueryProvider, Schema, SearchResolution,
    Signature, TraceRejection, Type, Value, expand_deferred_explained_with_queries,
    expand_deferred_with_queries, plan,
};
use htn_planner_ron::domain::{Source, compile};
use std::cell::Cell;

struct EmptyHost;
impl QueryProvider for EmptyHost {
    fn query(
        &self,
        _: &str,
        _: &[Value],
        budget: &mut QueryBudget<'_>,
    ) -> Result<Vec<Vec<Value>>, QueryError> {
        budget.charge(1)?;
        Ok(Vec::new())
    }
}

fn schema() -> Schema {
    Schema {
        predicates: ["candidate", "allowed", "missing", "untried"]
            .into_iter()
            .map(|name| Signature {
                name: name.into(),
                arguments: vec![Type::Int],
            })
            .collect(),
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    }
}
#[expect(
    clippy::unwrap_used,
    reason = "Synthetic domain must admit before causal assertions"
)]
fn domain(follow: &str, extra: &str, schema: Schema) -> CompiledDomain {
    let text = format!(
        "(:domain Evidence top_level_domain
        (:method (start) top_level_method (capture () ((&follow))))
        (:method (follow) {follow}) {extra})"
    );
    let mut limits = Limits::small();
    limits.diagnostics = 4096;
    compile(
        "Evidence.domain",
        &[Source {
            id: "Evidence.domain",
            text: &text,
        }],
        schema,
        &[],
        limits,
    )
    .unwrap()
}
fn fact(predicate: &str, value: i64) -> Fact {
    Fact {
        predicate: predicate.into(),
        arguments: vec![Value::Int(value)],
    }
}
#[expect(
    clippy::panic,
    reason = "Synthetic entry must capture a deferred invocation"
)]
fn token(domain: &CompiledDomain, facts: &FactSnapshot) -> DeferredTask {
    let result = plan(domain, facts, "start", &[]);
    let Outcome::Success(mut entries) = result.outcome else {
        panic!("capture failed: {result:?}")
    };
    let Some(PlanEntry::Deferred(token)) = entries.pop() else {
        panic!("capture must return an inert deferred invocation")
    };
    token
}
#[expect(
    clippy::unwrap_used,
    reason = "Tiny typed facts must admit before deferred evaluation"
)]
fn explain(domain: &CompiledDomain, rows: Vec<Fact>) -> ExplainedPlanningResult {
    let facts = domain.snapshot(rows).unwrap();
    expand_deferred_explained_with_queries(domain, &facts, &EmptyHost, &token(domain, &facts), None)
}

#[test_tag::tag(unit)]
#[test]
fn a_later_success_recovers_earlier_candidate_rejections_without_a_failure_cause() {
    let domain = domain(
        "(choose (and (candidate ?choice) (allowed ?choice)) ((!emit ?choice)))",
        "",
        schema(),
    );
    let result = explain(
        &domain,
        vec![
            fact("candidate", 1),
            fact("candidate", 2),
            fact("allowed", 2),
        ],
    );
    let Outcome::Success(entries) = &result.result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(
        entries[0].as_action().unwrap().arguments,
        vec![Value::Int(2)]
    );
    assert_eq!(result.explanation.resolution, SearchResolution::Planned);
    assert!(result.explanation.failures.is_empty());
    assert!(result.explanation.recovered_rejections > 0);
    assert!(result.explanation.selected_empty.is_none());
}

#[test_tag::tag(unit)]
#[test]
fn explicit_empty_is_distinct_from_exhausting_every_method() {
    let empty = explain(&domain("(decline () ())", "", schema()), vec![]);
    assert!(matches!(&empty.result.outcome, Outcome::Success(entries) if entries.is_empty()));
    assert_eq!(
        empty.explanation.resolution,
        SearchResolution::AuthoredEmpty
    );
    assert_eq!(
        empty.explanation.selected_empty.unwrap().declaration,
        "decline"
    );
    assert!(empty.explanation.failures.is_empty());
    assert_eq!(empty.explanation.explored_rejections, 0);

    let exhausted = explain(&domain("(needs_row (missing 1) ())", "", schema()), vec![]);
    assert_eq!(exhausted.result.outcome, Outcome::NoPlan);
    assert_eq!(
        exhausted.explanation.resolution,
        SearchResolution::Exhausted
    );
    assert!(exhausted.explanation.selected_empty.is_none());
    assert!(exhausted.explanation.failures.iter().any(|failure| {
        failure.predicate == "missing"
            && failure.locator.task.as_deref() == Some("follow")
            && failure.locator.declaration == "needs_row"
    }));
}

#[test_tag::tag(unit)]
#[test]
fn an_empty_fallback_retains_only_its_own_invocation_exploration() {
    let result = explain(
        &domain("(try (missing 1) ()) (decline () ())", "", schema()),
        vec![],
    );
    assert_eq!(
        result.explanation.resolution,
        SearchResolution::AuthoredEmpty
    );
    assert_eq!(
        result.explanation.selected_empty.unwrap().declaration,
        "decline"
    );
    assert!(
        result.explanation.failures.iter().any(|failure| {
            failure.predicate == "missing" && failure.locator.declaration == "try"
        })
    );
}

#[test_tag::tag(unit)]
#[test]
fn a_different_empty_child_does_not_inherit_the_failed_childs_cause() {
    let result = explain(
        &domain(
            "(attempt () ((failed_child))) (fallback () ((empty_child)))",
            "(:method (failed_child) (unavailable (missing 1) ((!emit 1))))
             (:method (empty_child) (decline () ()))",
            schema(),
        ),
        vec![],
    );
    let selected = result.explanation.selected_empty.unwrap();
    assert_eq!(selected.task.as_deref(), Some("empty_child"));
    assert_eq!(selected.declaration, "decline");
    assert!(result.explanation.failures.is_empty());
    assert!(result.explanation.recovered_rejections > 0);
}

#[test_tag::tag(unit)]
#[test]
fn negation_absence_succeeds_without_recording_its_failed_probe_as_a_blocker() {
    let result = explain(
        &domain("(absent (not (missing 1)) ())", "", schema()),
        vec![],
    );
    assert_eq!(
        result.explanation.resolution,
        SearchResolution::AuthoredEmpty
    );
    assert!(result.explanation.failures.is_empty());
    assert_eq!(result.explanation.explored_rejections, 0);
}

#[test_tag::tag(unit)]
#[test]
fn negation_witness_reports_the_failed_negation_instead_of_an_absent_child() {
    let result = explain(
        &domain("(absent (not (allowed 1)) ())", "", schema()),
        vec![fact("allowed", 1)],
    );
    assert_eq!(result.explanation.resolution, SearchResolution::Exhausted);
    assert!(result.explanation.failures.iter().any(|failure| {
        failure.predicate == "not" && failure.rejection == TraceRejection::ProbeFoundWitness
    }));
    assert!(!result.explanation.failures.iter().any(|failure| {
        failure.predicate == "allowed" && failure.rejection == TraceRejection::NoMatchingTuple
    }));
}

struct PrefixHost {
    calls: Cell<usize>,
}
impl QueryProvider for PrefixHost {
    fn query(
        &self,
        _: &str,
        inputs: &[Value],
        budget: &mut QueryBudget<'_>,
    ) -> Result<Vec<Vec<Value>>, QueryError> {
        budget.charge(1)?;
        self.calls.set(self.calls.get() + 1);
        let [Value::Int(prefix)] = inputs else {
            return Err(QueryError::InvalidInput);
        };
        budget.explain_absence(
            if *prefix == 1 {
                "prefix_one_absent"
            } else {
                "prefix_two_absent"
            },
            &[("prefix", *prefix)],
        );
        Ok(Vec::new())
    }
}

#[test_tag::tag(unit)]
#[test]
fn cached_absence_explanations_stay_with_the_rejected_bound_prefix() {
    let mut schema = schema();
    schema.predicates.push(Signature {
        name: "check".into(),
        arguments: vec![Type::Int, Type::Int],
    });
    schema.query_inputs.push(QueryInputs {
        predicate: "check".into(),
        bound: 1,
    });
    let domain = domain(
        "(first (check 1 ?a) ()) (cached (check 1 ?b) ()) (different (check 2 ?c) ()) (decline () ())",
        "",
        schema,
    );
    let facts = domain.snapshot(vec![]).unwrap();
    let host = PrefixHost {
        calls: Cell::new(0),
    };
    let result = expand_deferred_explained_with_queries(
        &domain,
        &facts,
        &host,
        &token(&domain, &facts),
        None,
    );
    assert_eq!(
        host.calls.get(),
        2,
        "one materialization per demanded prefix"
    );
    let failures = &result.explanation.failures;
    for (branch, code, prefix) in [
        ("first", "prefix_one_absent", 1),
        ("cached", "prefix_one_absent", 1),
        ("different", "prefix_two_absent", 2),
    ] {
        let failure = failures
            .iter()
            .find(|failure| failure.locator.declaration == branch)
            .unwrap();
        let note = failure.absence.as_ref().unwrap();
        assert_eq!(note.code, code);
        assert_eq!(note.values, vec![("prefix".into(), prefix)]);
    }
}

#[test_tag::tag(invariant)]
#[test]
fn explanation_is_independent_of_optional_trace_capacity_and_search_work() {
    let domain = domain("(try (missing 1) ()) (decline () ())", "", schema());
    let facts = domain.snapshot(vec![]).unwrap();
    let token = token(&domain, &facts);
    let ordinary = expand_deferred_with_queries(&domain, &facts, &EmptyHost, &token);
    let reference =
        expand_deferred_explained_with_queries(&domain, &facts, &EmptyHost, &token, None);
    assert_eq!(reference.result, ordinary);
    assert!(reference.trace.is_none());
    for cap in [0, 16, 4096] {
        let result =
            expand_deferred_explained_with_queries(&domain, &facts, &EmptyHost, &token, Some(cap));
        assert_eq!(result.result, reference.result);
        assert_eq!(result.explanation, reference.explanation);
        let trace = result.trace.unwrap();
        assert!(trace.retained_units <= cap);
        assert_eq!(trace.final_work, ordinary.work);
    }
}

#[test_tag::tag(unit)]
#[test]
fn rejected_alternative_samples_are_bounded_and_omissions_are_explicit() {
    let result = explain(
        &domain(
            "(a (missing 1) ()) (b (missing 2) ()) (c (missing 3) ())
             (d (missing 4) ()) (e (missing 5) ()) (f (missing 6) ()) (decline () ())",
            "",
            schema(),
        ),
        vec![],
    );
    // Four retained samples is the public diagnostic cardinality contract.
    assert_eq!(result.explanation.failures.len(), 4);
    assert!(result.explanation.omitted_rejections > 0);
    assert!(result.explanation.truncated);
    assert!(result.explanation.explored_rejections > result.explanation.failures.len() as u64);
}

struct UnexpectedSibling(Cell<usize>);
impl QueryProvider for UnexpectedSibling {
    fn query(
        &self,
        _: &str,
        _: &[Value],
        budget: &mut QueryBudget<'_>,
    ) -> Result<Vec<Vec<Value>>, QueryError> {
        budget.charge(1)?;
        self.0.set(self.0.get() + 1);
        Ok(Vec::new())
    }
}

#[test_tag::tag(unit)]
#[test]
fn first_success_cut_does_not_report_an_untried_sibling_as_exhausted() {
    // FirstSuccess belongs to typed/numeric RON; the deterministic .domain profile
    // intentionally has only enumerating alt, so this uses the legacy public frontend.
    let source = r#"(version:1,name:"cut",entries:["start"],tasks:[
        (name:"start",parameters:[],methods:[(name:"capture",steps:[Defer(name:"follow",arguments:[])])]),
        (name:"follow",parameters:[],methods:[(name:"selected",locals:[Int],steps:[
            Condition(FirstSuccess([
                Fact(name:"candidate",arguments:[Local(0)]),
                Fact(name:"untried",arguments:[Local(0)])])),
            Require(name:"allowed",arguments:[Local(0)]),
            Action(name:"emit",arguments:[Local(0)])])])])"#;
    let mut schema = schema();
    schema.query_inputs.push(QueryInputs {
        predicate: "untried".into(),
        bound: 0,
    });
    let domain = htn_planner_ron::compile("Cut.ron", source, schema, Limits::small()).unwrap();
    let facts = domain
        .snapshot(vec![fact("candidate", 1), fact("allowed", 2)])
        .unwrap();
    let host = UnexpectedSibling(Cell::new(0));
    let result = expand_deferred_explained_with_queries(
        &domain,
        &facts,
        &host,
        &token(&domain, &facts),
        None,
    );
    assert_eq!(result.result.outcome, Outcome::NoPlan);
    assert!(result.explanation.first_success_cuts > 0);
    assert_eq!(host.0.get(), 0, "cut sibling must never be evaluated");
    assert!(
        !result
            .explanation
            .failures
            .iter()
            .any(|failure| failure.predicate == "untried")
    );
    assert!(
        result
            .explanation
            .failures
            .iter()
            .any(|failure| failure.predicate == "allowed")
    );
}

#[test_tag::tag(invariant)]
#[test]
fn long_source_labels_and_resolved_unicode_values_are_clipped_at_utf8_boundaries() {
    let source_id = format!("{}.domain", "界".repeat(40));
    let predicate = format!("missing_{}", "x".repeat(70));
    let branch = format!("blocked_{}", "y".repeat(70));
    let text = format!(
        "(:domain Unicode top_level_domain
          (:method (start) top_level_method (capture (seed ?label) ((&follow ?label))))
          (:method (follow ?inp_label) ({branch} ({predicate} ?inp_label) ())))"
    );
    let schema = Schema {
        predicates: ["seed", predicate.as_str()]
            .into_iter()
            .map(|name| Signature {
                name: name.into(),
                arguments: vec![Type::Symbol],
            })
            .collect(),
        ..Schema::default()
    };
    let domain = compile(
        &source_id,
        &[Source {
            id: &source_id,
            text: &text,
        }],
        schema,
        &[],
        Limits::small(),
    )
    .unwrap();
    let facts = domain
        .snapshot(vec![Fact {
            predicate: "seed".into(),
            arguments: vec![Value::Symbol("界".repeat(128))],
        }])
        .unwrap();
    let result = expand_deferred_explained_with_queries(
        &domain,
        &facts,
        &EmptyHost,
        &token(&domain, &facts),
        None,
    );
    assert_eq!(result.explanation.resolution, SearchResolution::Exhausted);
    let failure = result.explanation.failures.first().unwrap();
    assert!(failure.locator.source.len() <= 32);
    assert!(failure.locator.source.starts_with('界'));
    assert!(failure.locator.source.ends_with('…'));
    assert!(failure.locator.declaration.len() <= 32);
    assert!(failure.predicate.len() <= 48);
    assert!(failure.arguments.len() <= 96);
    assert!(failure.arguments.contains('界'));
    assert!(result.explanation.truncated);
}

#[test_tag::tag(invariant)]
#[test]
fn a_successful_empty_witness_recovers_earlier_failures_in_the_same_method() {
    let domain = domain(
        "(choose (and (candidate ?choice) (allowed ?choice)) ())",
        "",
        schema(),
    );
    let facts = domain
        .snapshot(vec![
            fact("candidate", 1),
            fact("candidate", 2),
            fact("allowed", 2),
        ])
        .unwrap();
    let token = token(&domain, &facts);
    let ordinary = expand_deferred_with_queries(&domain, &facts, &EmptyHost, &token);
    let reference =
        expand_deferred_explained_with_queries(&domain, &facts, &EmptyHost, &token, None);
    assert!(matches!(&ordinary.outcome, Outcome::Success(entries) if entries.is_empty()));
    assert_eq!(reference.result, ordinary);
    assert_eq!(
        reference.explanation.resolution,
        SearchResolution::AuthoredEmpty
    );
    assert_eq!(
        reference
            .explanation
            .selected_empty
            .as_ref()
            .unwrap()
            .declaration,
        "choose"
    );
    assert!(reference.explanation.failures.is_empty());
    assert!(reference.explanation.recovered_rejections > 0);
    for cap in [0, 16, 4096] {
        let explained =
            expand_deferred_explained_with_queries(&domain, &facts, &EmptyHost, &token, Some(cap));
        assert_eq!(explained.result, ordinary);
        assert_eq!(explained.explanation, reference.explanation);
    }
}

#[path = "domain_explanations/failure_boundaries.rs"]
mod failure_boundaries;

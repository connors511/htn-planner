//! Fatal probe errors and derived-return conflicts are actual evaluation evidence.
use super::*;

#[expect(
    clippy::unwrap_used,
    reason = "Synthetic numeric domain must admit before boundary assertions"
)]
fn legacy(source: &str) -> CompiledDomain {
    let mut limits = Limits::small();
    limits.diagnostics = 4096;
    htn_planner_ron::compile("FailureBoundaries.ron", source, Schema::default(), limits).unwrap()
}
#[expect(
    clippy::unwrap_used,
    reason = "Empty facts must admit for the synthetic failure boundaries"
)]
fn parity(domain: &CompiledDomain) -> ExplainedPlanningResult {
    let facts = domain.snapshot(vec![]).unwrap();
    let token = token(domain, &facts);
    let ordinary = expand_deferred_with_queries(domain, &facts, &EmptyHost, &token);
    let reference =
        expand_deferred_explained_with_queries(domain, &facts, &EmptyHost, &token, None);
    assert_eq!(reference.result, ordinary);
    for cap in [0, 16, 4096] {
        let explained =
            expand_deferred_explained_with_queries(domain, &facts, &EmptyHost, &token, Some(cap));
        assert_eq!(explained.result, ordinary);
        assert_eq!(explained.explanation, reference.explanation);
    }
    reference
}

#[test_tag::tag(unit)]
#[test]
fn fatal_overflow_inside_not_is_not_discarded_as_an_ordinary_failed_probe() {
    // MAX+1 is the normative checked-i64 overflow boundary, not fixture tuning.
    // Numeric RON deliberately exercises a runtime construct absent from .domain.
    let source = r#"(version:1,name:"fatal",entries:["start"],tasks:[
        (name:"start",parameters:[],methods:[(name:"capture",steps:[Defer(name:"follow",arguments:[])])]),
        (name:"follow",parameters:[],methods:[(name:"negated",steps:[Condition(Not(Calculate(
            op:Add,left:Literal(Int(9223372036854775807)),right:Literal(Int(1)),output:Literal(Int(0))
        )))])])])"#;
    let result = parity(&legacy(source));
    assert_eq!(result.result.outcome, Outcome::InvalidInput);
    assert_eq!(
        result.explanation.resolution,
        SearchResolution::Interrupted(htn_planner::TraceOutcome::InvalidInput)
    );
    let failure = result
        .explanation
        .failures
        .iter()
        .find(|failure| failure.rejection == TraceRejection::ArithmeticOverflow)
        .expect("fatal overflow is evidence even inside a negated probe");
    assert!(failure.arguments.contains(&i64::MAX.to_string()));
    assert!(failure.arguments.ends_with(",1"));
    assert_eq!(failure.locator.task.as_deref(), Some("follow"));
    assert_eq!(failure.locator.declaration, "negated");
}

#[test_tag::tag(unit)]
#[test]
fn derived_output_conflict_is_reported_at_the_actual_caller_boundary() {
    let source = r#"(version:1,name:"output-conflict",entries:["start"],queries:[
        (name:"two",parameters:[],outputs:[Int],locals:[],condition:And([]),result:[Literal(Int(2))])
        ],tasks:[
        (name:"start",parameters:[],methods:[(name:"capture",steps:[Defer(name:"follow",arguments:[])])]),
        (name:"follow",parameters:[],methods:[(name:"needs_one",steps:[
            Condition(Derived(name:"two",arguments:[],outputs:[Literal(Int(1))]))
        ])])])"#;
    let result = parity(&legacy(source));
    assert_eq!(result.result.outcome, Outcome::NoPlan);
    assert_eq!(result.explanation.resolution, SearchResolution::Exhausted);
    let failure = result
        .explanation
        .failures
        .iter()
        .find(|failure| failure.rejection == TraceRejection::UnificationConflict)
        .expect("returned two cannot unify with the caller's required one");
    assert_eq!(failure.locator.task.as_deref(), Some("follow"));
    assert_eq!(failure.locator.declaration, "needs_one");
    assert_eq!(failure.predicate, "two");
    assert_eq!(
        failure.arguments, "2",
        "retain the actual conflicting return"
    );
    assert!(
        failure.absence.is_none(),
        "a successful derived query is not host absence"
    );
}

#[path = "../examples/support/lifecycle.rs"]
mod lifecycle;
use htn_planner::Value;
use lifecycle::{Event, Host, Observation};

#[test_tag::tag(unit)]
#[test]
fn checkpoint_and_equivalent_plan_preserve_active_progress_and_tick_guard() {
    let domain = lifecycle::domain().unwrap();
    let observation = Observation::ready(7, 1);
    let mut host = Host::new(&domain);
    assert_eq!(
        host.decide(&domain, &observation, 0, &[Value::Int(7)]),
        Event::Accepted
    );
    assert_eq!(host.advance(&domain, &observation, 0), Event::Started(7));
    assert_eq!(
        host.advance(&domain, &observation, 1),
        Event::Progress(7, 1)
    );
    let checkpoint = ron::ser::to_string(&host.checkpoint()).unwrap();
    let relocated = htn_planner::CompiledDomain::admit(
        "relocated",
        domain.schema().clone(),
        domain.definition().clone(),
        domain.limits(),
    )
    .unwrap();
    let mut resumed = Host::resume(&relocated, ron::from_str(&checkpoint).unwrap()).unwrap();
    assert_eq!(
        resumed.decide(&relocated, &observation, 2, &[Value::Int(7)]),
        Event::Retained
    );
    assert_eq!(
        resumed.advance(&relocated, &observation, 2),
        Event::Completed(7)
    );
    assert_eq!(
        resumed.advance(&relocated, &observation, 2),
        Event::Suppressed
    );
    assert_eq!(
        resumed.advance(&relocated, &Observation::ready(7, 2), 3),
        Event::Expanded
    );
}

#[test_tag::tag(unit)]
#[test]
fn host_revalidates_rejected_starts_and_distinguishes_actual_execution_failure() {
    use lifecycle::Rejection;
    let domain = lifecycle::domain().unwrap();
    let observation = Observation::ready(7, 1);
    let mut host = Host::new(&domain);
    assert_eq!(
        host.decide(&domain, &observation, 0, &[Value::Int(7)]),
        Event::Accepted
    );
    let mut blocked = observation.clone();
    blocked.can_start = false;
    assert_eq!(
        host.advance(&domain, &blocked, 0),
        Event::RejectedStart(Rejection::Preconditions)
    );
    assert_eq!(
        host.decide(&domain, &observation, 0, &[Value::Int(7)]),
        Event::Suppressed
    );
    assert_eq!(
        host.decide(&domain, &observation, 1, &[Value::Int(7)]),
        Event::Accepted
    );
    assert_eq!(host.advance(&domain, &observation, 1), Event::Started(7));
    let mut failed = observation.clone();
    failed.fail_execution = true;
    assert_eq!(
        host.advance(&domain, &failed, 2),
        Event::FailedExecution(Rejection::ExecutionFailed)
    );
    assert_eq!(host.advance(&domain, &observation, 3), Event::Finished);
}

#[test_tag::tag(unit)]
#[test]
fn removed_target_fails_owned_execution_and_stale_position_rejects_new_start() {
    use lifecycle::Rejection;
    let domain = lifecycle::domain().unwrap();
    let observation = Observation::ready(7, 1);
    let mut host = Host::new(&domain);
    assert_eq!(
        host.decide(&domain, &observation, 0, &[Value::Int(7)]),
        Event::Accepted
    );
    assert_eq!(host.advance(&domain, &observation, 0), Event::Started(7));
    let mut missing = observation.clone();
    missing.available.clear();
    assert_eq!(
        host.advance(&domain, &missing, 1),
        Event::FailedExecution(Rejection::Unavailable)
    );
    assert_eq!(
        host.decide(&domain, &observation, 2, &[Value::Int(7)]),
        Event::Accepted
    );
    assert_eq!(host.advance(&domain, &observation, 2), Event::Started(7));
    assert_eq!(
        host.advance(&domain, &observation, 3),
        Event::Progress(7, 1)
    );
    assert_eq!(host.advance(&domain, &observation, 4), Event::Completed(7));
    assert_eq!(
        host.advance(&domain, &Observation::ready(7, 2), 5),
        Event::Expanded
    );
    assert_eq!(
        host.advance(&domain, &Observation::ready(7, 3), 5),
        Event::RejectedStart(Rejection::StalePosition)
    );
}

const CHAIN: &str = r#"(version:1,name:"chain",entries:["entry"],tasks:[
  (name:"entry",parameters:[],methods:[(name:"start",steps:[Defer(name:"first",arguments:[])])]),
  (name:"first",parameters:[],methods:[(name:"middle",steps:[Defer(name:"last",arguments:[])])]),
  (name:"last",parameters:[],methods:[(name:"empty",steps:[])])])"#;
#[test_tag::tag(unit)]
#[test]
fn empty_deferred_chains_and_reset_cannot_plan_again_in_same_logical_tick() {
    let domain = lifecycle::admit(CHAIN, htn_planner::Limits::small()).unwrap();
    let observation = Observation::ready(7, 1);
    let mut host = Host::new(&domain);
    assert_eq!(host.decide(&domain, &observation, 0, &[]), Event::Accepted);
    assert_eq!(host.advance(&domain, &observation, 0), Event::Suppressed);
    assert_eq!(host.advance(&domain, &observation, 1), Event::Expanded);
    assert_eq!(host.advance(&domain, &observation, 1), Event::Suppressed);
    assert_eq!(
        host.advance(&domain, &observation, 2),
        Event::CompletedEmpty
    );
    assert_eq!(
        host.decide(&domain, &observation, 2, &[]),
        Event::Suppressed
    );
    host.reset();
    assert_eq!(host.advance(&domain, &observation, 2), Event::Finished);
    assert_eq!(
        host.decide(&domain, &observation, 2, &[]),
        Event::Suppressed
    );
    assert_eq!(host.decide(&domain, &observation, 3, &[]), Event::Accepted);
}

#[test_tag::tag(compatibility)]
#[test]
fn checkpoint_rejects_changed_definition_and_reset_releases_active_work() {
    let domain = lifecycle::domain().unwrap();
    let observation = Observation::ready(7, 1);
    let mut host = Host::new(&domain);
    assert_eq!(
        host.decide(&domain, &observation, 0, &[Value::Int(7)]),
        Event::Accepted
    );
    assert_eq!(host.advance(&domain, &observation, 0), Event::Started(7));
    let changed = lifecycle::admit(CHAIN, htn_planner::Limits::small()).unwrap();
    assert!(Host::resume(&changed, host.checkpoint()).is_err());
    host.reset();
    let resumed = Host::resume(&domain, host.checkpoint()).unwrap();
    assert_eq!(
        ron::ser::to_string(&host.checkpoint()).unwrap(),
        ron::ser::to_string(&resumed.checkpoint()).unwrap()
    );
    assert_eq!(host.advance(&domain, &observation, 1), Event::Finished);
}

#[test_tag::tag(unit)]
#[test]
fn equivalent_full_plan_after_checkpoint_does_not_restart_completed_prefix() {
    let domain = lifecycle::domain().unwrap();
    let observation = Observation::ready(7, 1);
    let mut host = Host::new(&domain);
    assert_eq!(
        host.decide(&domain, &observation, 0, &[Value::Int(7)]),
        Event::Accepted
    );
    assert_eq!(host.advance(&domain, &observation, 0), Event::Started(7));
    assert_eq!(
        host.advance(&domain, &observation, 1),
        Event::Progress(7, 1)
    );
    assert_eq!(host.advance(&domain, &observation, 2), Event::Completed(7));
    let checkpoint = ron::ser::to_string(&host.checkpoint()).unwrap();
    let mut resumed = Host::resume(&domain, ron::from_str(&checkpoint).unwrap()).unwrap();
    let fresh = Observation::ready(7, 2);
    assert_eq!(
        resumed.decide(&domain, &fresh, 3, &[Value::Int(7)]),
        Event::Retained
    );
    assert_eq!(resumed.advance(&domain, &fresh, 3), Event::Suppressed);
    assert_eq!(resumed.advance(&domain, &fresh, 4), Event::Expanded);
    assert_eq!(resumed.advance(&domain, &fresh, 4), Event::Started(7));
}

#[test_tag::tag(unit)]
#[test]
fn completed_empty_root_is_not_no_plan_and_cannot_retry_after_same_tick_reset() {
    let source = r#"(version:1,name:"empty-root",entries:["entry"],tasks:[(name:"entry",parameters:[],methods:[(name:"empty",steps:[])])])"#;
    let domain = lifecycle::admit(source, htn_planner::Limits::small()).unwrap();
    let observation = Observation::ready(7, 1);
    let mut host = Host::new(&domain);
    assert_eq!(
        host.decide(&domain, &observation, 0, &[]),
        Event::CompletedEmpty
    );
    assert_eq!(
        host.decide(&domain, &observation, 0, &[]),
        Event::Suppressed
    );
    host.reset();
    assert_eq!(
        host.decide(&domain, &observation, 0, &[]),
        Event::Suppressed
    );
    assert_eq!(
        host.decide(&domain, &observation, 1, &[]),
        Event::CompletedEmpty
    );
    let checkpoint = ron::ser::to_string(&host.checkpoint()).unwrap();
    // The semantics marker is the compatibility field under test.
    let stale = checkpoint.replace(htn_planner::COMPATIBILITY_ID, "old-semantics");
    assert!(Host::resume(&domain, ron::from_str(&stale).unwrap()).is_err());
}

#[test_tag::tag(unit)]
#[test]
fn failed_deferred_expansion_keeps_token_and_never_adopts_an_executable_prefix() {
    let source = r#"(version:1,name:"fail-expansion",entries:["entry"],tasks:[
      (name:"entry",parameters:[],methods:[(name:"capture",steps:[Defer(name:"later",arguments:[])])]),
      (name:"later",parameters:[],methods:[(name:"fail",steps:[Action(name:"prepare",arguments:[Literal(Int(7))]),Require(name:"ready",arguments:[Literal(Int(7)),Literal(Int(9))])])])])"#;
    let domain = lifecycle::admit(source, htn_planner::Limits::small()).unwrap();
    let observation = Observation::ready(7, 1);
    let mut host = Host::new(&domain);
    assert_eq!(host.decide(&domain, &observation, 0, &[]), Event::Accepted);
    assert_eq!(host.advance(&domain, &observation, 1), Event::NoPlan);
    assert_eq!(host.advance(&domain, &observation, 1), Event::Suppressed);
    let recovered = Observation::ready(7, 9);
    assert_eq!(host.advance(&domain, &recovered, 2), Event::Expanded);
    assert_eq!(host.advance(&domain, &recovered, 2), Event::Started(7));
}

#[test_tag::tag(unit)]
#[test]
fn host_distinguishes_exhausted_deferred_expansion_from_capacity_and_no_plan() {
    let steps = vec!["Action(name:\"prepare\",arguments:[Literal(Int(7))])"; 20].join(",");
    let source = format!(
        "(version:1,name:\"exhausted\",entries:[\"entry\"],tasks:[(name:\"entry\",parameters:[],methods:[(name:\"capture\",steps:[Defer(name:\"later\",arguments:[])])]),(name:\"later\",parameters:[],methods:[(name:\"many\",steps:[{steps}])])])"
    );
    let observation = Observation::ready(7, 1);
    for (limits, expected) in [
        (
            htn_planner::Limits {
                work: 500,
                ..htn_planner::Limits::small()
            },
            Event::BudgetExhausted,
        ),
        (
            htn_planner::Limits {
                plan: 1,
                ..htn_planner::Limits::small()
            },
            Event::CapacityExceeded,
        ),
    ] {
        let domain = lifecycle::admit(&source, limits).unwrap();
        let mut host = Host::new(&domain);
        assert_eq!(host.decide(&domain, &observation, 0, &[]), Event::Accepted);
        assert_eq!(host.advance(&domain, &observation, 1), expected);
        assert_eq!(host.advance(&domain, &observation, 1), Event::Suppressed);
    }
}

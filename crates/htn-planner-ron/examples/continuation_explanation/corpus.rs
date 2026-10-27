//! Supplied-source fixtures and actual provider/semantic checks; no clocks.
use super::require;
use htn_planner::{
    CompiledDomain, DeferredTask, FactSnapshot, Limits, Outcome, PlanEntry, PlanningResult,
    QueryBudget, QueryError, QueryInputs, QueryProvider, Schema, SearchExplanation,
    SearchResolution, Signature, Type, Value, plan_with_queries,
};
use htn_planner_ron::domain::{Source, compile};
#[derive(Clone, Copy)]
pub(super) enum Case {
    Recovery,
    Eligibility,
    NoRows,
    Empty,
    Recovered,
    Exhausted,
}
impl Case {
    pub(super) const ALL: [Self; 6] = [
        Self::Recovery,
        Self::Eligibility,
        Self::NoRows,
        Self::Empty,
        Self::Recovered,
        Self::Exhausted,
    ];
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Recovery => "recovery-detail-failure",
            Self::Eligibility => "eligibility-failure",
            Self::NoRows => "no-provider-rows",
            Self::Empty => "explicit-empty",
            Self::Recovered => "earlier-rejection-later-success",
            Self::Exhausted => "multiple-failures-no-plan",
        }
    }
    const fn methods(self) -> &'static str {
        match self {
            Self::Recovery => "(check (recovery 121 120) ((!emit 121))) (decline () ())",
            Self::Eligibility => "(check (eligible 7) ((!emit 7))) (decline () ())",
            Self::NoRows => "(check (candidate 7 ?choice) ((!emit ?choice)))",
            Self::Empty => "(decline () ())",
            Self::Recovered => {
                "(select (and (candidate 7 ?choice) (eligible ?choice)) ((!emit ?choice)))"
            }
            Self::Exhausted => "(first (eligible 1) ((!emit 1))) (second (eligible 3) ((!emit 3)))",
        }
    }
}

pub(super) struct Provider(Case);
impl QueryProvider for Provider {
    fn query(
        &self,
        predicate: &str,
        inputs: &[Value],
        budget: &mut QueryBudget<'_>,
    ) -> Result<Vec<Vec<Value>>, QueryError> {
        // Same charges and provider code run with the collector disabled/enabled.
        budget.charge(8)?;
        budget.reserve(32)?;
        match (predicate, inputs) {
            ("recovery", [Value::Int(requested), Value::Int(recovery)]) => {
                if requested > recovery {
                    budget.explain_absence(
                        "link_ticks_exceed_recovery",
                        &[("requested", *requested), ("recovery", *recovery)],
                    );
                    Ok(Vec::new())
                } else {
                    Ok(vec![inputs.to_vec()])
                }
            }
            ("eligible", [Value::Int(choice)]) => {
                if *choice == 2 {
                    Ok(vec![inputs.to_vec()])
                } else {
                    budget.explain_absence("synthetic_ineligible", &[("choice", *choice)]);
                    Ok(Vec::new())
                }
            }
            ("candidate", [Value::Int(prefix)]) => Ok(if matches!(self.0, Case::NoRows) {
                Vec::new()
            } else {
                vec![
                    vec![Value::Int(*prefix), Value::Int(1)],
                    vec![Value::Int(*prefix), Value::Int(2)],
                ]
            }),
            _ => Err(QueryError::InvalidInput),
        }
    }
}
pub(super) struct Fixture {
    pub(super) case: Case,
    pub(super) text: String,
    pub(super) domain: CompiledDomain,
    pub(super) facts: FactSnapshot,
    pub(super) token: DeferredTask,
    pub(super) provider: Provider,
}
pub(super) fn fixture(case: Case) -> Result<Fixture, String> {
    let signatures = [("candidate", 2), ("eligible", 1), ("recovery", 2)];
    let schema = Schema {
        predicates: signatures
            .iter()
            .map(|(name, arity)| Signature {
                name: (*name).into(),
                arguments: vec![Type::Int; *arity],
            })
            .collect(),
        query_inputs: signatures
            .iter()
            .map(|(name, arity)| QueryInputs {
                predicate: (*name).into(),
                bound: if *name == "candidate" { 1 } else { *arity },
            })
            .collect(),
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    };
    let text = format!(
        "(:domain Example top_level_domain (:method (ready) top_level_method (start () ((&resume)))) (:method (resume) {}))",
        case.methods()
    );
    let domain = compile(
        "Example.domain",
        &[Source {
            id: "Example.domain",
            text: &text,
        }],
        schema,
        &[],
        Limits::small(),
    )
    .map_err(|error| error.message)?;
    let facts = domain
        .snapshot(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let provider = Provider(case);
    let initial = plan_with_queries(&domain, &facts, &provider, "ready", &[]);
    let Outcome::Success(entries) = initial.outcome else {
        return Err(format!("{}: initial plan failed", case.name()));
    };
    let [PlanEntry::Deferred(token)] = entries.as_slice() else {
        return Err("initial plan must contain exactly one actual Deferred token".into());
    };
    Ok(Fixture {
        case,
        text,
        domain,
        facts,
        token: token.clone(),
        provider,
    })
}
pub(super) fn validate(
    fixture: &Fixture,
    result: &PlanningResult,
    e: &SearchExplanation,
) -> Result<(), String> {
    require(e.failures.len() <= 4, "collector exceeded four records")?;
    let failure = |predicate: &str| e.failures.iter().find(|f| f.predicate == predicate);
    match fixture.case {
        Case::Recovery | Case::Eligibility => {
            require(
                e.resolution == SearchResolution::AuthoredEmpty,
                "missing authored-empty classification",
            )?;
            require(
                matches!(&result.outcome, Outcome::Success(entries) if entries.is_empty()),
                "decline must return an empty success",
            )?;
            let predicate = if matches!(fixture.case, Case::Recovery) {
                "recovery"
            } else {
                "eligible"
            };
            let f = failure(predicate).ok_or("actual failed predicate missing")?;
            require(
                f.locator.source == "Example.domain",
                "failure source alias missing",
            )?;
            require(
                f.locator.task.as_deref() == Some("resume"),
                "failure task missing",
            )?;
            let absence = f.absence.as_ref().ok_or("observed host detail missing")?;
            if matches!(fixture.case, Case::Recovery) {
                require(
                    absence.code == "link_ticks_exceed_recovery",
                    "wrong recovery code",
                )?;
                require(
                    absence.values == [("requested".into(), 121), ("recovery".into(), 120)],
                    "wrong actual recovery arguments",
                )?;
                require(
                    f.arguments.contains("121") && f.arguments.contains("120"),
                    "resolved predicate arguments missing",
                )?;
            } else {
                require(
                    absence.code == "synthetic_ineligible",
                    "wrong eligibility code",
                )?;
                require(
                    absence.values == [("choice".into(), 7)],
                    "wrong actual choice",
                )?;
            }
            require(
                e.selected_empty.is_some(),
                "actual empty branch locator missing",
            )?;
        }
        Case::NoRows => {
            require(
                e.resolution == SearchResolution::Exhausted && result.outcome == Outcome::NoPlan,
                "no rows must exhaust search",
            )?;
            let f = failure("candidate").ok_or("empty provider predicate missing")?;
            require(
                f.absence.is_none(),
                "unexplained absence fabricated a host reason",
            )?;
        }
        Case::Empty => {
            require(
                e.resolution == SearchResolution::AuthoredEmpty,
                "explicit empty not classified",
            )?;
            require(
                e.failures.is_empty() && e.explored_rejections == 0,
                "explicit empty fabricated failure",
            )?;
            require(
                matches!(&result.outcome, Outcome::Success(entries) if entries.is_empty()),
                "explicit empty changed outcome",
            )?;
        }
        Case::Recovered => {
            require(
                e.resolution == SearchResolution::Planned,
                "recovered search not planned",
            )?;
            require(
                e.explored_rejections > 0 && e.recovered_rejections == e.explored_rejections,
                "recovered rejection counts missing",
            )?;
            let Outcome::Success(entries) = &result.outcome else {
                return Err("later candidate failed".into());
            };
            require(
                entries.len() == 1
                    && entries[0]
                        .as_action()
                        .is_some_and(|action| action.arguments == [Value::Int(2)]),
                "wrong recovered concrete candidate",
            )?;
        }
        Case::Exhausted => {
            require(
                e.resolution == SearchResolution::Exhausted && result.outcome == Outcome::NoPlan,
                "multiple failures did not exhaust",
            )?;
            require(
                e.explored_rejections >= 2 && e.recovered_rejections == 0,
                "multiple failure evidence missing",
            )?;
            require(failure("eligible").is_some(), "observed predicate missing")?;
        }
    }
    Ok(())
}

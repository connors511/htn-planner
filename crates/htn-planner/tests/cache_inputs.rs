//! Closed-record keys exercise both expression and already-evaluated query callers.
use htn_planner::{
    CompiledDomain, Condition, Definition, Expression as E, Limits, Method, Outcome, QueryBudget,
    QueryError, QueryInputs, QueryProvider, RecordType, Schema, Signature, Step, Task, Type, Value,
    plan_with_queries,
};
use std::cell::Cell;

pub fn key(other: bool) -> Value {
    Value::Record {
        type_name: "Key".into(),
        fields: vec![
            Value::Symbol("owned-symbol-payload".into()),
            Value::Int(i64::from(other)),
        ],
    }
}

/// Build the admitted cache fixture.
///
/// # Errors
/// Returns the admission error for an invalid fixture or requested limits.
pub fn domain(limits: Limits, changed: bool) -> Result<CompiledDomain, String> {
    let input = if changed {
        E::Literal(key(true))
    } else {
        E::Parameter(0)
    };
    let condition = |input: E| {
        Step::Condition(Condition::Fact {
            name: "candidate".into(),
            arguments: vec![input, E::Literal(Value::Int(7))],
        })
    };
    let require = |input: E| Step::Require {
        name: "candidate".into(),
        arguments: vec![input, E::Literal(Value::Int(7))],
    };
    CompiledDomain::admit(
        "cache-key-contract",
        Schema {
            records: vec![RecordType {
                name: "Key".into(),
                fields: vec![Type::Symbol, Type::Int],
            }],
            query_inputs: vec![QueryInputs {
                predicate: "candidate".into(),
                bound: 1,
            }],
            predicates: vec![
                Signature {
                    name: "candidate".into(),
                    arguments: vec![Type::Record("Key".into()), Type::Int],
                },
                Signature {
                    name: "never".into(),
                    arguments: vec![],
                },
            ],
            actions: vec![Signature {
                name: "emit".into(),
                arguments: vec![Type::Record("Key".into())],
            }],
            ..Schema::default()
        },
        Definition {
            version: 1,
            name: "cache-key-contract".into(),
            entries: vec!["entry".into()],
            queries: vec![],
            tasks: vec![Task {
                name: "entry".into(),
                parameters: vec![Type::Record("Key".into())],
                outputs: vec![],
                methods: vec![
                    Method {
                        name: "rolled-back".into(),
                        locals: vec![],
                        result: vec![],
                        steps: vec![
                            condition(E::Parameter(0)),
                            require(E::Parameter(0)),
                            Step::Require {
                                name: "never".into(),
                                arguments: vec![],
                            },
                        ],
                    },
                    Method {
                        name: "fallback".into(),
                        locals: vec![],
                        result: vec![],
                        steps: vec![
                            require(input.clone()),
                            condition(input.clone()),
                            Step::Action {
                                name: "emit".into(),
                                arguments: vec![input],
                            },
                        ],
                    },
                ],
            }],
        },
        limits,
    )
    .map_err(|error| error.message)
}

pub struct Provider {
    pub empty: bool,
    pub calls: Cell<usize>,
}
impl QueryProvider for Provider {
    fn query(
        &self,
        predicate: &str,
        inputs: &[Value],
        budget: &mut QueryBudget<'_>,
    ) -> Result<Vec<Vec<Value>>, QueryError> {
        assert_eq!(predicate, "candidate");
        budget.charge(5)?;
        budget.reserve(128)?;
        self.calls.set(self.calls.get() + 1);
        Ok(if self.empty {
            vec![]
        } else {
            vec![vec![inputs[0].clone(), Value::Int(7)]]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[test_tag::tag(invariant)]
    fn record_keys_preserve_hit_identity_empty_batches_and_rollback() -> Result<(), String> {
        for empty in [false, true] {
            for changed in [false, true] {
                let domain = domain(
                    Limits {
                        diagnostics: 4096,
                        ..Limits::small()
                    },
                    changed,
                )?;
                let facts = domain
                    .snapshot(vec![])
                    .map_err(|error| format!("{error:?}"))?;
                let provider = Provider {
                    empty,
                    calls: Cell::new(0),
                };
                let result = plan_with_queries(&domain, &facts, &provider, "entry", &[key(false)]);
                assert_eq!(provider.calls.get(), if changed { 2 } else { 1 });
                if empty {
                    assert_eq!(result.outcome, Outcome::NoPlan);
                } else {
                    let Outcome::Success(entries) = result.outcome else {
                        return Err("expected cached fallback plan".into());
                    };
                    assert_eq!(
                        entries[0].as_action().ok_or("expected action")?.arguments,
                        vec![key(changed)]
                    );
                }
            }
        }
        Ok(())
    }
    #[test]
    #[test_tag::tag(invariant)]
    fn trace_clipping_preserves_record_cache_work_and_provider_calls() -> Result<(), String> {
        let domain = domain(
            Limits {
                diagnostics: 4096,
                ..Limits::small()
            },
            false,
        )?;
        let facts = domain
            .snapshot(vec![])
            .map_err(|error| format!("{error:?}"))?;
        for empty in [false, true] {
            let provider = Provider {
                empty,
                calls: Cell::new(0),
            };
            let expected = plan_with_queries(&domain, &facts, &provider, "entry", &[key(false)]);
            for capacity in [0, 1, 4096] {
                let provider = Provider {
                    empty,
                    calls: Cell::new(0),
                };
                let traced = htn_planner::plan_traced_with_queries(
                    &domain,
                    &facts,
                    &provider,
                    "entry",
                    &[key(false)],
                    capacity,
                );
                assert_eq!(traced.result, expected);
                assert_eq!(provider.calls.get(), 1);
            }
        }
        Ok(())
    }
}

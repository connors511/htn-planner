//! Public `.domain` admission and deterministic planning contracts.
use htn_planner::{Fact, Limits, Outcome, Schema, Signature, Type, Value, plan};
use htn_planner_ron::domain::{Source, compile};

#[test_tag::tag(unit)]
#[test]
fn derived_private_witnesses_reopen_after_later_caller_failure() {
    let text = r"(:domain Choices top_level_domain
        (:method (ready) top_level_method
            (choose
                (and (seed ?private) (#select_choice ?choice) (allowed ?choice))
                ((!emit ?private ?choice))))
        (:axiom (select_choice ?out_choice)
            (and (candidate ?private ?out_choice))))";
    let schema = int_schema(
        &[("seed", 1), ("candidate", 2), ("allowed", 1)],
        &[("emit", 2)],
    );
    let domain = compile(
        "Choices.domain",
        &[Source {
            id: "Choices.domain",
            text,
        }],
        schema,
        &[],
        Limits::small(),
    )
    .unwrap();
    let row = |predicate: &str, values: &[i64]| Fact {
        predicate: predicate.into(),
        arguments: values.iter().copied().map(Value::Int).collect(),
    };
    let facts = domain
        .snapshot(vec![
            row("seed", &[99]),
            row("candidate", &[7, 1]),
            row("candidate", &[8, 2]),
            row("allowed", &[2]),
        ])
        .unwrap();
    let result = plan(&domain, &facts, "ready", &[]);
    let Outcome::Success(entries) = result.outcome else {
        panic!("{result:?}");
    };
    // The later allowed-row rejects the first witness. The second witness must
    // reopen without overwriting the identically named caller-local binding.
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].as_action().unwrap().arguments,
        vec![Value::Int(99), Value::Int(2)]
    );
}

fn int_schema(predicates: &[(&str, usize)], actions: &[(&str, usize)]) -> Schema {
    let signatures = |items: &[(&str, usize)]| {
        items
            .iter()
            .map(|(name, arity)| Signature {
                name: (*name).into(),
                arguments: vec![Type::Int; *arity],
            })
            .collect()
    };
    Schema {
        predicates: signatures(predicates),
        actions: signatures(actions),
        ..Schema::default()
    }
}

#[test_tag::tag(unit)]
#[test]
fn bound_output_fails_before_private_body_work_and_preserves_caller_fallback() {
    // This test-only host observes query invocation without changing facts.
    struct CandidateCalls(std::cell::Cell<usize>);
    impl htn_planner::QueryProvider for CandidateCalls {
        fn query(
            &self,
            _predicate: &str,
            _inputs: &[Value],
            budget: &mut htn_planner::QueryBudget<'_>,
        ) -> Result<Vec<Vec<Value>>, htn_planner::QueryError> {
            budget.charge(1)?;
            self.0.set(self.0.get() + 1);
            Ok(Vec::new())
        }
    }
    let text = r"(:domain StrictOutput top_level_domain
        (:method (ready) top_level_method
            (bound (and (seed ?choice) (#select ?choice)) ((!emit 0)))
            (fallback (seed ?choice) ((!emit ?choice))))
        (:axiom (select ?out_choice) (candidate ?out_choice)))";
    let mut schema = int_schema(&[("seed", 1), ("candidate", 1)], &[("emit", 1)]);
    schema.query_inputs.push(htn_planner::QueryInputs {
        predicate: "candidate".into(),
        bound: 0,
    });
    let domain = compile(
        "Strict.domain",
        &[Source {
            id: "Strict.domain",
            text,
        }],
        schema,
        &[],
        Limits::small(),
    )
    .unwrap();
    let facts = domain
        .snapshot(vec![Fact {
            predicate: "seed".into(),
            arguments: vec![Value::Int(99)],
        }])
        .unwrap();
    let calls = CandidateCalls(std::cell::Cell::new(0));
    let result = htn_planner::plan_with_queries(&domain, &facts, &calls, "ready", &[]);
    assert_eq!(
        calls.0.get(),
        0,
        "strict bound out must not enter the axiom body"
    );
    let Outcome::Success(entries) = result.outcome else {
        panic!("{result:?}");
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].as_action().unwrap().arguments,
        vec![Value::Int(99)]
    );
}

#[test_tag::tag(unit)]
#[test]
fn included_task_captures_constant_arguments_and_expands_against_fresh_facts() {
    let root = r#"(:include "helpers.domain")
        (:domain Root top_level_domain
            (:constants Timing (delay 90))
            (:method (ready) top_level_method
                (select (candidate ?choice) ((&after ?choice @delay)))))"#;
    let helpers = r"(:domain Helpers
        (:method (after ?inp_choice ?inp_delay)
            (fresh (allowed ?inp_choice ?current)
                ((!emit ?inp_choice ?current ?inp_delay)))))";
    let domain = compile(
        "Root.domain",
        &[
            Source {
                id: "Root.domain",
                text: root,
            },
            Source {
                id: "helpers.domain",
                text: helpers,
            },
        ],
        int_schema(&[("candidate", 1), ("allowed", 2)], &[("emit", 3)]),
        &[],
        Limits::small(),
    )
    .unwrap();
    let initial = domain
        .snapshot(vec![Fact {
            predicate: "candidate".into(),
            arguments: vec![Value::Int(7)],
        }])
        .unwrap();
    let result = plan(&domain, &initial, "ready", &[]);
    let Outcome::Success(entries) = result.outcome else {
        panic!("{result:?}");
    };
    let [htn_planner::PlanEntry::Deferred(token)] = entries.as_slice() else {
        panic!("{entries:?}");
    };
    assert_eq!(token.arguments(), &[Value::Int(7), Value::Int(90)]);
    let fresh = domain
        .snapshot(vec![
            Fact {
                predicate: "candidate".into(),
                arguments: vec![Value::Int(9)],
            },
            Fact {
                predicate: "allowed".into(),
                arguments: vec![Value::Int(7), Value::Int(42)],
            },
        ])
        .unwrap();
    let result = htn_planner::expand_deferred(&domain, &fresh, token);
    let Outcome::Success(expanded) = result.outcome else {
        panic!("{result:?}");
    };
    assert_eq!(expanded.len(), 1);
    // The captured choice survives the changed candidate; current comes only
    // from fresh facts, and the literal constant is captured without a clock.
    assert_eq!(
        expanded[0].as_action().unwrap().arguments,
        vec![Value::Int(7), Value::Int(42), Value::Int(90)]
    );
}

#[test_tag::tag(unit)]
#[test]
fn alt_reopens_later_children_and_grounded_not_filters_the_bound_choice() {
    let text = r"(:domain Alternatives top_level_domain
        (:method (ready) top_level_method
            (select (and (alt (preferred ?choice) (fallback ?choice))
                         (not (blocked ?choice)) (allowed ?choice))
                ((!emit ?choice)))))";
    let domain = compile(
        "Alt.domain",
        &[Source {
            id: "Alt.domain",
            text,
        }],
        int_schema(
            &[
                ("preferred", 1),
                ("fallback", 1),
                ("blocked", 1),
                ("allowed", 1),
            ],
            &[("emit", 1)],
        ),
        &[],
        Limits::small(),
    )
    .unwrap();
    let facts = domain
        .snapshot(
            [
                ("preferred", 1),
                ("blocked", 1),
                ("fallback", 2),
                ("allowed", 2),
            ]
            .into_iter()
            .map(|(predicate, value)| Fact {
                predicate: predicate.into(),
                arguments: vec![Value::Int(value)],
            })
            .collect(),
        )
        .unwrap();
    let result = plan(&domain, &facts, "ready", &[]);
    let Outcome::Success(entries) = result.outcome else {
        panic!("{result:?}");
    };
    assert_eq!(
        entries[0].as_action().unwrap().arguments,
        vec![Value::Int(2)]
    );
}

#[test_tag::tag(unit)]
#[test]
fn negation_rejects_fresh_and_branch_dependent_caller_variables() {
    for condition in [
        "(not (blocked ?choice))",
        "(and (alt (seed ?choice) ()) (not (blocked ?choice)))",
    ] {
        let text = format!(
            "(:domain Negative top_level_domain (:method (ready) top_level_method (select {condition} ((!emit ?choice)))))"
        );
        let sources = [Source {
            id: "Negative.domain",
            text: &text,
        }];
        let schema = int_schema(&[("seed", 1), ("blocked", 1)], &[("emit", 1)]);
        // Named source lowering itself enforces safe negation; rejection must
        // not depend only on the later executable compiler's separate checks.
        assert!(
            htn_planner_ron::domain::lower("Negative.domain", &sources, &schema, &[]).is_err(),
            "condition={condition}"
        );
        assert!(
            compile("Negative.domain", &sources, schema, &[], Limits::small()).is_err(),
            "condition={condition}"
        );
    }
}

#[test_tag::tag(unit)]
#[test]
fn unsupported_or_and_io_modes_reject_instead_of_changing_commitment_semantics() {
    for text in [
        "(:domain Cut top_level_domain (:method (ready) top_level_method (choose (or (candidate ?choice) (candidate ?choice)) ((!emit ?choice)))))",
        "(:domain Io top_level_domain (:method (ready) top_level_method (choose (#select ?choice) ((!emit ?choice)))) (:axiom (select ?io_choice) (candidate ?io_choice)))",
    ] {
        assert!(
            compile(
                "Unsupported.domain",
                &[Source {
                    id: "Unsupported.domain",
                    text
                }],
                int_schema(&[("candidate", 1)], &[("emit", 1)]),
                &[],
                Limits::small()
            )
            .is_err()
        );
    }
}

#[test_tag::tag(unit)]
#[test]
fn include_graph_rejects_cycles_missing_inputs_and_duplicate_source_identities() {
    let root = r#"(:include "helpers.domain") (:domain Root top_level_domain (:method (ready) top_level_method (run () ((!emit 1)))))"#;
    let cyclic = r#"(:include "Root.domain") (:domain Helpers)"#;
    for sources in [
        vec![Source {
            id: "Root.domain",
            text: root,
        }],
        vec![
            Source {
                id: "Root.domain",
                text: root,
            },
            Source {
                id: "helpers.domain",
                text: cyclic,
            },
        ],
        vec![
            Source {
                id: "Root.domain",
                text: root,
            },
            Source {
                id: "Root.domain",
                text: root,
            },
        ],
    ] {
        assert!(
            compile(
                "Root.domain",
                &sources,
                int_schema(&[], &[("emit", 1)]),
                &[],
                Limits::small()
            )
            .is_err()
        );
    }
}

#[test_tag::tag(unit)]
#[test]
fn bare_true_is_a_symbol_and_cannot_satisfy_a_bool_schema() {
    let text = "(:domain Symbols top_level_domain (:method (ready) top_level_method (run () ((!emit true)))))";
    let schema = |ty| Schema {
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![ty],
        }],
        ..Schema::default()
    };
    let domain = compile(
        "Symbols.domain",
        &[Source {
            id: "Symbols.domain",
            text,
        }],
        schema(Type::Symbol),
        &[],
        Limits::small(),
    )
    .unwrap();
    let facts = domain.snapshot(vec![]).unwrap();
    let result = plan(&domain, &facts, "ready", &[]);
    let Outcome::Success(entries) = result.outcome else {
        panic!("{result:?}");
    };
    assert_eq!(
        entries[0].as_action().unwrap().arguments,
        vec![Value::Symbol("true".into())]
    );
    assert!(
        compile(
            "Symbols.domain",
            &[Source {
                id: "Symbols.domain",
                text
            }],
            schema(Type::Bool),
            &[],
            Limits::small()
        )
        .is_err()
    );
}

#[test_tag::tag(unit)]
#[test]
fn unsupported_literal_forms_and_unresolved_constants_reject_before_execution() {
    let mut literals = [
        "\"text\"",
        "1.25",
        "9223372036854775808",
        "@missing",
        "(1 2)",
    ]
    .map(str::to_owned)
    .to_vec();
    // The public frontend must retain the syntax parser's 64-list ceiling.
    literals.push(format!("{}{}", "(".repeat(65), ")".repeat(65)));
    for literal in literals {
        let text = format!(
            "(:domain Values top_level_domain (:method (ready) top_level_method (run () ((!emit {literal})))))"
        );
        assert!(
            compile(
                "Values.domain",
                &[Source {
                    id: "Values.domain",
                    text: &text
                }],
                int_schema(&[], &[("emit", 1)]),
                &[],
                Limits::small()
            )
            .is_err(),
            "literal={literal}"
        );
    }
}

#[test_tag::tag(unit)]
#[test]
fn overdeep_owned_schema_rejects_before_missing_root_source() {
    let mut value = Value::Int(0);
    for _ in 0..1024 {
        value = Value::Record {
            type_name: "Deep".into(),
            fields: vec![value],
        };
    }
    let schema = Schema {
        records: vec![htn_planner::RecordType {
            name: "Deep".into(),
            fields: vec![Type::Record("Deep".into())],
        }],
        predicates: vec![Signature {
            name: "unused".into(),
            arguments: vec![Type::Record("Deep".into())],
        }],
        effects: vec![htn_planner::PlanningEffect {
            name: "deep".into(),
            parameters: vec![],
            operations: vec![htn_planner::EffectOperation::Assert {
                predicate: "unused".into(),
                arguments: vec![htn_planner::Expression::Literal(value)],
            }],
        }],
        ..Schema::default()
    };
    // Borrowed lowering must reject before cloning; owned compile must also
    // dispose the same rejected tree through its iterative ownership guard.
    let borrowed = htn_planner_ron::domain::lower("Missing.domain", &[], &schema, &[]).unwrap_err();
    assert!(
        borrowed.message.to_ascii_lowercase().contains("capacity"),
        "{borrowed:?}"
    );
    let owned = compile("Missing.domain", &[], schema, &[], Limits::small()).unwrap_err();
    assert!(
        owned.message.to_ascii_lowercase().contains("capacity"),
        "{owned:?}"
    );
}

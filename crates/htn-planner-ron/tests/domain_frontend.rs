use htn_planner::{Fact, FactSnapshot, Limits, Outcome, Schema, Signature, Type, Value, plan};
use htn_planner_ron::domain::{Source, compile};

#[test_tag::tag(unit)]
#[test]
fn ordered_candidates_are_selected_through_the_public_domain_frontend() {
    let text = "(:domain Example top_level_domain (:method (ready) top_level_method (select (candidate ?choice) ((!emit ?choice)))))";
    let schema = Schema {
        predicates: vec![Signature {
            name: "candidate".into(),
            arguments: vec![Type::Int],
        }],
        actions: vec![Signature {
            name: "emit".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    };
    let domain = compile(
        "Example.domain",
        &[Source {
            id: "Example.domain",
            text,
        }],
        schema.clone(),
        &[],
        Limits::small(),
    )
    .unwrap();
    let facts = FactSnapshot::new(
        &schema,
        vec![
            Fact {
                predicate: "candidate".into(),
                arguments: vec![Value::Int(7)],
            },
            Fact {
                predicate: "candidate".into(),
                arguments: vec![Value::Int(9)],
            },
        ],
    )
    .unwrap();
    let result = plan(&domain, &facts, "ready", &[]);
    let Outcome::Success(entries) = result.outcome else {
        panic!("{result:?}")
    };
    assert_eq!(
        entries[0].as_action().unwrap().arguments,
        vec![Value::Int(7)]
    );
}

#[test_tag::tag(unit)]
#[test]
fn unused_entry_inputs_require_bounded_registered_host_types() {
    let text = "(:domain Example top_level_domain (:method (ready ?inp_unused) top_level_method (empty () ())))";
    let sources = [Source {
        id: "Example.domain",
        text,
    }];
    let lower = |hints: &[Signature]| {
        htn_planner_ron::domain::lower("Example.domain", &sources, &Schema::default(), hints)
    };
    assert!(lower(&[]).is_err());
    for ty in [
        Type::Ref("Unknown".into()),
        Type::Record("x".repeat(4096)),
        Type::Fixed(19),
    ] {
        assert!(
            lower(&[Signature {
                name: "ready".into(),
                arguments: vec![ty]
            }])
            .is_err()
        );
    }
    assert!(
        lower(&[Signature {
            name: "ready".into(),
            arguments: vec![Type::Int]
        }])
        .is_ok()
    );
}

#[test_tag::tag(unit)]
#[test]
fn strict_output_failure_cannot_hide_a_recursive_axiom_from_admission() {
    let text = "(:domain Example top_level_domain (:method (ready) top_level_method (empty () ())) (:axiom (loop ?out_value) (and (seed ?out_value) (#loop ?out_value))))";
    let schema = Schema {
        predicates: vec![Signature {
            name: "seed".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    };
    assert!(
        compile(
            "Example.domain",
            &[Source {
                id: "Example.domain",
                text
            }],
            schema,
            &[],
            Limits::small()
        )
        .is_err()
    );
}

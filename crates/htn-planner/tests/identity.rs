use htn_planner::{
    CompiledDomain, Definition, Expression, Limits, Method, Schema, Signature, Step, Task, Type,
    Value,
};

fn inputs(destination: i64) -> (Schema, Definition) {
    let schema = Schema {
        actions: vec![Signature {
            name: "dispatch".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    };
    let definition = Definition {
        queries: vec![],
        version: 1,
        name: "warehouse".into(),
        entries: vec!["ship".into()],
        tasks: vec![Task {
            name: "ship".into(),
            parameters: vec![],
            outputs: vec![],
            methods: vec![Method {
                name: "dispatch".into(),
                locals: vec![],
                result: vec![],
                steps: vec![Step::Action {
                    name: "dispatch".into(),
                    arguments: vec![Expression::Literal(Value::Int(destination))],
                }],
            }],
        }],
    };
    (schema, definition)
}

fn admitted(
    source: &str,
    destination: i64,
    limits: Limits,
) -> Result<CompiledDomain, htn_planner::Error> {
    let (schema, definition) = inputs(destination);
    CompiledDomain::admit(source, schema, definition, limits)
}

#[test_tag::tag(compatibility)]
#[test]
fn definition_identity_preserves_semantics_across_source_locations() {
    let first = admitted("authoring/warehouse.ron", 23, Limits::small()).unwrap();
    let equivalent = admitted("memory:warehouse", 23, Limits::small()).unwrap();
    assert_eq!(first.identity(), equivalent.identity());
    assert_ne!(
        first.identity(),
        admitted("authoring/warehouse.ron", 24, Limits::small())
            .unwrap()
            .identity()
    );
}

#[test_tag::tag(compatibility)]
#[test]
fn definition_identity_changes_when_any_admitted_limit_changes() {
    let initial = Limits::small();
    let identity = admitted("fixture", 23, initial).unwrap().identity();
    let variants = [
        Limits {
            work: initial.work + 1,
            ..initial
        },
        Limits {
            storage: initial.storage + 1,
            ..initial
        },
        Limits {
            depth: initial.depth + 1,
            ..initial
        },
        Limits {
            plan: initial.plan + 1,
            ..initial
        },
        Limits {
            bindings: initial.bindings + 1,
            ..initial
        },
        Limits {
            diagnostics: initial.diagnostics + 1,
            ..initial
        },
    ];
    for limits in variants {
        assert_ne!(
            identity,
            admitted("fixture", 23, limits).unwrap().identity()
        );
    }
}

#[test_tag::tag(compatibility)]
#[test]
fn definition_identity_includes_registered_host_schema() {
    let (schema, definition) = inputs(-23);
    let initial = CompiledDomain::admit(
        "fixture",
        schema.clone(),
        definition.clone(),
        Limits::small(),
    )
    .unwrap();
    let mut augmented = schema;
    augmented.references.push("Parcel".into());
    let changed = CompiledDomain::admit("fixture", augmented, definition, Limits::small()).unwrap();
    assert_ne!(initial.identity(), changed.identity());
}

#[test_tag::tag(compatibility)]
#[test]
fn read_only_admitted_exports_preserve_schema_definition_and_identity() {
    let (schema, definition) = inputs(23);
    let domain = CompiledDomain::admit(
        "original",
        schema.clone(),
        definition.clone(),
        Limits::small(),
    )
    .unwrap();
    assert_eq!(domain.schema(), &schema);
    assert_eq!(domain.definition(), &definition);
    let exported = CompiledDomain::admit(
        "exported",
        domain.schema().clone(),
        domain.definition().clone(),
        domain.limits(),
    )
    .unwrap();
    assert_eq!(exported.identity(), domain.identity());
}

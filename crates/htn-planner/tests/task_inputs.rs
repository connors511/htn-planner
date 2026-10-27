use htn_planner::{CompiledDomain, Definition, Limits, Method, Schema, Task, Type, Value};

fn domain(parameters: Vec<Type>) -> Result<CompiledDomain, htn_planner::Error> {
    CompiledDomain::admit(
        "input-fixture",
        Schema::default(),
        Definition {
            version: 1,
            queries: vec![],
            name: "inputs".into(),
            entries: vec!["entry".into()],
            tasks: vec![
                Task {
                    name: "entry".into(),
                    parameters: vec![],
                    outputs: vec![],
                    methods: vec![Method {
                        name: "empty".into(),
                        locals: vec![],
                        result: vec![],
                        steps: vec![],
                    }],
                },
                Task {
                    name: "private".into(),
                    parameters,
                    outputs: vec![],
                    methods: vec![Method {
                        name: "empty".into(),
                        locals: vec![],
                        result: vec![],
                        steps: vec![],
                    }],
                },
            ],
        },
        Limits::small(),
    )
}

#[test_tag::tag(unit)]
#[test]
fn declared_private_task_inputs_can_be_validated_without_execution() {
    let domain = domain(vec![Type::Int]).unwrap();
    assert!(
        domain
            .validate_task_inputs("private", &[Value::Int(7)])
            .is_ok()
    );
    assert!(
        domain
            .validate_task_inputs("private", &[Value::Bool(true)])
            .is_err()
    );
    assert!(domain.validate_task_inputs("private", &[]).is_err());
    assert!(domain.validate_task_inputs("unknown", &[]).is_err());
}

#[test_tag::tag(invariant)]
#[test]
fn input_validation_caps_aggregate_values_and_diagnostic_names() {
    let domain = domain(vec![Type::Symbol, Type::Symbol]).unwrap();
    let value = Value::Symbol("x".repeat(500_000));
    assert!(
        domain
            .validate_task_inputs("private", &[value.clone(), value])
            .is_err()
    );
    let error = domain
        .validate_task_inputs(&"x".repeat(1025), &[])
        .unwrap_err();
    assert!(error.locator.declaration.len() <= 1024);
    assert!(
        error
            .locator
            .task
            .as_ref()
            .is_none_or(|name| name.len() <= 1024)
    );
}

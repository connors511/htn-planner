//! Pure planning with a bounded source-linked reasoning capture.
use htn_planner::{
    CompiledDomain, Definition, Expression, Fact, FactSnapshot, Limits, Method, Schema, Signature,
    Step, Task, Type, Value, plan_traced,
};
use std::io::Write;
fn main() -> Result<(), String> {
    let schema = Schema {
        predicates: vec![Signature {
            name: "available".into(),
            arguments: vec![Type::Int],
        }],
        actions: vec![Signature {
            name: "dispatch".into(),
            arguments: vec![Type::Int],
        }],
        ..Schema::default()
    };
    let definition = Definition {
        version: 1,
        name: "warehouse-trace".into(),
        queries: vec![],
        entries: vec!["ship".into()],
        tasks: vec![Task {
            name: "ship".into(),
            parameters: vec![],
            outputs: vec![],
            methods: vec![Method {
                name: "dispatch-available".into(),
                locals: vec![Type::Int],
                result: vec![],
                steps: vec![
                    Step::Query {
                        name: "available".into(),
                        arguments: vec![Expression::Local(0)],
                    },
                    Step::Action {
                        name: "dispatch".into(),
                        arguments: vec![Expression::Local(0)],
                    },
                ],
            }],
        }],
    };
    let mut limits = Limits::small();
    // An explicit synthetic inspection profile: the default cap16 may retain no root.
    limits.diagnostics = 4096;
    let domain = CompiledDomain::admit("example:warehouse", schema.clone(), definition, limits)
        .map_err(|error| error.message)?;
    let facts = FactSnapshot::new(
        &schema,
        vec![Fact {
            predicate: "available".into(),
            arguments: vec![Value::Int(7)],
        }],
    )?;
    let captured = plan_traced(&domain, &facts, "ship", &[], 4096);
    let mut output = std::io::stdout().lock();
    for node in &captured.trace.nodes {
        writeln!(
            output,
            "[{} parent={:?}] {:?} {} task={:?} declaration={} step={:?} arguments={:?}",
            node.id,
            node.parent,
            node.event,
            node.locator.source,
            node.locator.task,
            node.locator.declaration,
            node.locator.step,
            node.arguments
        )
        .map_err(|error| error.to_string())?;
    }
    writeln!(
        output,
        "Final result: {:?}; work: {:?}; trace units: {}/{}; truncated: {}",
        captured.result.outcome,
        captured.result.work,
        captured.trace.retained_units,
        captured.trace.capacity,
        captured.trace.truncated
    )
    .map_err(|error| error.to_string())
}

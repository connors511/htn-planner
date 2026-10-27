//! Owned admission rejects arbitrary trees; cleanup is linear in owned input,
//! outside decision metering, and uses an explicit heap worklist rather than recursion.
use crate::{Condition, Definition, EffectOperation, Expression, Fact, Schema, Step, Value};
use std::ops::Deref;

pub(crate) trait RejectedInput {
    fn empty() -> Self;
    fn discard(&mut self);
}
/// Installed before any rejection. Taking a successful input leaves an empty owner.
pub(crate) struct Owned<T: RejectedInput>(T);
impl<T: RejectedInput> Owned<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self(value)
    }
    pub(crate) fn take(mut self) -> T {
        std::mem::replace(&mut self.0, T::empty())
    }
}
impl<T: RejectedInput> Deref for Owned<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
impl<T: RejectedInput> Drop for Owned<T> {
    fn drop(&mut self) {
        self.0.discard();
    }
}
impl RejectedInput for Vec<Fact> {
    fn empty() -> Self {
        Self::new()
    }
    fn discard(&mut self) {
        for fact in self {
            discard_values(&mut fact.arguments);
        }
    }
}
impl RejectedInput for Schema {
    fn empty() -> Self {
        Self::default()
    }
    fn discard(&mut self) {
        for effect in &mut self.effects {
            for operation in &mut effect.operations {
                match operation {
                    EffectOperation::Assert { arguments, .. }
                    | EffectOperation::Retract { arguments, .. } => discard_expressions(arguments),
                }
            }
        }
    }
}
impl RejectedInput for Definition {
    fn empty() -> Self {
        Self {
            queries: Vec::new(),
            version: 0,
            name: String::new(),
            entries: Vec::new(),
            tasks: Vec::new(),
        }
    }
    fn discard(&mut self) {
        for query in &mut self.queries {
            discard_expressions(&mut query.result);
            // Condition's existing Drop also drains every literal and control child.
        }
        for task in &mut self.tasks {
            for method in &mut task.methods {
                discard_expressions(&mut method.result);
                for step in &mut method.steps {
                    match step {
                        Step::Condition(_) => {}
                        Step::Call {
                            arguments, outputs, ..
                        } => {
                            discard_expressions(arguments);
                            discard_expressions(outputs);
                        }
                        Step::Effect { arguments, .. }
                        | Step::Defer { arguments, .. }
                        | Step::Query { arguments, .. }
                        | Step::Require { arguments, .. }
                        | Step::Action { arguments, .. }
                        | Step::Task { arguments, .. } => discard_expressions(arguments),
                    }
                }
            }
        }
    }
}
fn discard_values(values: &mut Vec<Value>) {
    let mut pending = std::mem::take(values);
    while let Some(value) = pending.pop() {
        match value {
            Value::Record { mut fields, .. } => pending.append(&mut fields),
            Value::Bool(_)
            | Value::Int(_)
            | Value::Fixed { .. }
            | Value::Symbol(_)
            | Value::Ref { .. } => {}
        }
    }
}
fn discard_expression(expression: &mut Expression) {
    match expression {
        Expression::Literal(value) => {
            discard_values(&mut vec![std::mem::replace(value, Value::Int(0))]);
        }
        Expression::Parameter(_) | Expression::Local(_) => {}
    }
}
fn discard_expressions(expressions: &mut [Expression]) {
    for value in expressions {
        discard_expression(value);
    }
}
/// Shared with the already-public Condition Drop: controls and literal payloads
/// must both be hollow before Rust drops a child Condition.
pub(crate) fn discard_condition(root: &mut Condition) {
    let mut pending = Vec::new();
    drain_condition(root, &mut pending);
    while let Some(mut child) = pending.pop() {
        drain_condition(&mut child, &mut pending);
    }
}
fn drain_condition(condition: &mut Condition, pending: &mut Vec<Condition>) {
    match condition {
        Condition::And(children) | Condition::Or(children) | Condition::FirstSuccess(children) => {
            pending.append(children);
        }
        Condition::Not(child) => pending.push(std::mem::replace(
            child.as_mut(),
            Condition::And(Vec::new()),
        )),
        Condition::Fact { arguments, .. } => discard_expressions(arguments),
        Condition::Derived {
            arguments, outputs, ..
        } => {
            discard_expressions(arguments);
            discard_expressions(outputs);
        }
        Condition::Compare { left, right, .. } => {
            discard_expression(left);
            discard_expression(right);
        }
        Condition::Calculate {
            left,
            right,
            output,
            ..
        } => {
            discard_expression(left);
            discard_expression(right);
            discard_expression(output);
        }
    }
}

impl RejectedInput for Vec<Vec<Value>> {
    fn empty() -> Self {
        Self::new()
    }
    fn discard(&mut self) {
        for row in self {
            discard_values(row);
        }
    }
}

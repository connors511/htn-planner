//! Private owned-boundary cleanup also handles rejected programmatic trees.
use super::{Condition, Definition, Expression, Step};
use htn_planner::Value;

pub(super) struct Owned(pub Definition);
impl Drop for Owned {
    fn drop(&mut self) {
        for query in self.0.queries.drain(..) {
            discard_condition(query.condition);
            discard_expressions(query.result);
        }
        for task in self.0.tasks.drain(..) {
            for method in task.methods {
                discard_expressions(method.result);
                for step in method.steps {
                    match step {
                        Step::Condition(c) => discard_condition(c),
                        Step::Call {
                            arguments, outputs, ..
                        } => {
                            discard_expressions(arguments);
                            discard_expressions(outputs);
                        }
                        Step::Action { arguments, .. }
                        | Step::Task { arguments, .. }
                        | Step::Require { arguments, .. }
                        | Step::Query { arguments, .. }
                        | Step::Effect { arguments, .. }
                        | Step::Defer { arguments, .. } => discard_expressions(arguments),
                    }
                }
            }
        }
    }
}
fn discard_expressions(expressions: impl IntoIterator<Item = Expression>) {
    for expression in expressions {
        if let Expression::Literal(value) = expression {
            let mut pending = vec![value];
            while let Some(value) = pending.pop() {
                if let Value::Record { fields, .. } = value {
                    pending.extend(fields);
                }
            }
        }
    }
}
fn discard_condition(condition: Condition) {
    let mut pending = vec![condition];
    while let Some(condition) = pending.pop() {
        match condition {
            Condition::And(children)
            | Condition::Or(children)
            | Condition::FirstSuccess(children) => pending.extend(children),
            Condition::Not(child) => pending.push(*child),
            Condition::Fact { arguments, .. } => discard_expressions(arguments),
            Condition::Derived {
                arguments, outputs, ..
            } => {
                discard_expressions(arguments);
                discard_expressions(outputs);
            }
            Condition::Compare { left, right, .. } => discard_expressions([left, right]),
            Condition::Calculate {
                left,
                right,
                output,
                ..
            } => discard_expressions([left, right, output]),
        }
    }
}

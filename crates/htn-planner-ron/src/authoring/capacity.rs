//! Check cold clone/recursion bounds before lowering any untrusted tree.
use super::{Binding, Condition, Definition, Expression, Step};
use htn_planner::{Type, Value};
const MAX_SIZE: usize = 1_000_000;
const MAX_DEPTH: usize = 128;
struct Check(usize);
impl Check {
    const fn add(&mut self, count: usize) -> Result<(), &'static str> {
        self.0 = self.0.saturating_add(count);
        if self.0 > MAX_SIZE {
            Err("named authoring aggregate capacity exceeded")
        } else {
            Ok(())
        }
    }
    fn types(&mut self, types: &[Type]) -> Result<(), &'static str> {
        self.add(types.len())?;
        for ty in types {
            if let Type::Ref(name) | Type::Record(name) = ty {
                self.add(name.len())?;
            }
        }
        Ok(())
    }
    fn bindings(&mut self, bindings: &[Binding]) -> Result<(), &'static str> {
        self.add(bindings.len())?;
        for binding in bindings {
            match binding {
                Binding::Ref(name, ty) | Binding::Record(name, ty) => {
                    self.add(name.len().saturating_add(ty.len()))?;
                }
                Binding::Int(name)
                | Binding::Symbol(name)
                | Binding::Bool(name)
                | Binding::Fixed(name, _) => self.add(name.len())?,
            }
        }
        Ok(())
    }
    fn expressions(&mut self, expressions: &[Expression]) -> Result<(), &'static str> {
        self.add(expressions.len())?;
        for expression in expressions {
            self.expression(expression)?;
        }
        Ok(())
    }
    fn expression(&mut self, expression: &Expression) -> Result<(), &'static str> {
        match expression {
            Expression::Var(name) => self.add(name.len()),
            Expression::Literal(value) => {
                let mut pending = vec![(value, 0)];
                while let Some((value, depth)) = pending.pop() {
                    if depth > MAX_DEPTH {
                        return Err("named literal depth capacity exceeded");
                    }
                    self.add(1)?;
                    match value {
                        Value::Symbol(name)
                        | Value::Ref {
                            type_name: name, ..
                        } => self.add(name.len())?,
                        Value::Record { type_name, fields } => {
                            self.add(type_name.len().saturating_add(fields.len()))?;
                            pending.extend(fields.iter().map(|value| (value, depth + 1)));
                        }
                        Value::Bool(_) | Value::Int(_) | Value::Fixed { .. } => {}
                    }
                }
                Ok(())
            }
        }
    }
    fn condition(&mut self, condition: &Condition) -> Result<(), &'static str> {
        let mut pending = vec![(condition, 0)];
        while let Some((condition, depth)) = pending.pop() {
            if depth > MAX_DEPTH {
                return Err("named condition depth capacity exceeded");
            }
            self.add(1)?;
            match condition {
                Condition::And(children)
                | Condition::Or(children)
                | Condition::FirstSuccess(children) => {
                    self.add(children.len())?;
                    pending.extend(children.iter().map(|c| (c, depth + 1)));
                }
                Condition::Not(child) => pending.push((child, depth + 1)),
                Condition::Fact { name, arguments } => {
                    self.add(name.len())?;
                    self.expressions(arguments)?;
                }
                Condition::Derived {
                    name,
                    arguments,
                    outputs,
                } => {
                    self.add(name.len())?;
                    self.expressions(arguments)?;
                    self.expressions(outputs)?;
                }
                Condition::Compare { left, right, .. } => {
                    self.expression(left)?;
                    self.expression(right)?;
                }
                Condition::Calculate {
                    left,
                    right,
                    output,
                    ..
                } => {
                    self.expression(left)?;
                    self.expression(right)?;
                    self.expression(output)?;
                }
            }
        }
        Ok(())
    }
}
pub(super) fn check(definition: &Definition) -> Result<(), &'static str> {
    let mut check = Check(0);
    check.add(
        definition
            .name
            .len()
            .saturating_add(definition.entries.len())
            .saturating_add(definition.tasks.len())
            .saturating_add(definition.queries.len()),
    )?;
    for entry in &definition.entries {
        check.add(entry.len())?;
    }
    for task in &definition.tasks {
        check.add(task.name.len().saturating_add(task.methods.len()))?;
        check.bindings(&task.parameters)?;
        check.types(&task.outputs)?;
        for method in &task.methods {
            check.add(method.name.len().saturating_add(method.steps.len()))?;
            check.bindings(&method.locals)?;
            check.expressions(&method.result)?;
            for step in &method.steps {
                match step {
                    Step::Condition(condition) => check.condition(condition)?,
                    Step::Call {
                        name,
                        arguments,
                        outputs,
                    } => {
                        check.add(name.len())?;
                        check.expressions(arguments)?;
                        check.expressions(outputs)?;
                    }
                    Step::Action { name, arguments }
                    | Step::Task { name, arguments }
                    | Step::Query { name, arguments }
                    | Step::Require { name, arguments }
                    | Step::Effect { name, arguments }
                    | Step::Defer { name, arguments } => {
                        check.add(name.len())?;
                        check.expressions(arguments)?;
                    }
                }
            }
        }
    }
    for query in &definition.queries {
        check.add(query.name.len())?;
        check.bindings(&query.parameters)?;
        check.bindings(&query.locals)?;
        check.types(&query.outputs)?;
        check.condition(&query.condition)?;
        check.expressions(&query.result)?;
    }
    Ok(())
}

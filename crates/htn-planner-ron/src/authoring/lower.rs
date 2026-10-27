use super::{Binding, Condition, Definition, Expression, Step, owned::Owned};
use htn_planner::{Error, Locator, Type};
use std::collections::BTreeMap;

impl Definition {
    /// Resolve names to ordered numeric slots without changing the runtime IR contract.
    /// # Errors
    /// Rejects unsupported versions, invalid/duplicate/shadowed names, unknown variables,
    /// and excessive cold authoring size/depth. Runtime admission still checks types/flow.
    pub fn lower(self, source_id: &str) -> Result<htn_planner::Definition, Error> {
        let owned = Owned(self);
        let definition = &owned.0;
        let context = Scope::new(source_id, None, &definition.name, &[], &[])?;
        if source_id.len() > 1024 || definition.version != 2 {
            return Err(context.error("expected authoring version 2 and bounded source identity"));
        }
        super::capacity::check(definition).map_err(|message| context.error(message))?;
        let mut tasks = Vec::new();
        for task in &definition.tasks {
            let parameters = Scope::new(
                source_id,
                Some(&task.name),
                &task.name,
                &task.parameters,
                &[],
            )?;
            let mut methods = Vec::new();
            for method in &task.methods {
                let mut scope = Scope::new(
                    source_id,
                    Some(&task.name),
                    &method.name,
                    &task.parameters,
                    &method.locals,
                )?;
                let mut steps = Vec::new();
                for (index, step) in method.steps.iter().enumerate() {
                    scope.step = Some(index);
                    steps.push(scope.lower_step(step)?);
                }
                scope.step = None;
                methods.push(htn_planner::Method {
                    name: method.name.clone(),
                    locals: binding_types(&method.locals),
                    result: scope.expressions(&method.result)?,
                    steps,
                });
            }
            tasks.push(htn_planner::Task {
                name: task.name.clone(),
                parameters: binding_types(&task.parameters),
                outputs: task.outputs.clone(),
                methods,
            });
            drop(parameters);
        }
        let mut queries = Vec::new();
        for query in &definition.queries {
            let scope = Scope::new(
                source_id,
                Some(&query.name),
                &query.name,
                &query.parameters,
                &query.locals,
            )?;
            queries.push(htn_planner::DerivedQuery {
                name: query.name.clone(),
                parameters: binding_types(&query.parameters),
                outputs: query.outputs.clone(),
                locals: binding_types(&query.locals),
                condition: scope.condition(&query.condition)?,
                result: scope.expressions(&query.result)?,
            });
        }
        Ok(htn_planner::Definition {
            version: 1,
            name: definition.name.clone(),
            entries: definition.entries.clone(),
            tasks,
            queries,
        })
    }
}
fn binding_types(bindings: &[Binding]) -> Vec<Type> {
    bindings
        .iter()
        .map(|binding| match binding {
            Binding::Ref(_, ty) => Type::Ref(ty.clone()),
            Binding::Record(_, ty) => Type::Record(ty.clone()),
            Binding::Bool(_) => Type::Bool,
            Binding::Int(_) => Type::Int,
            Binding::Symbol(_) => Type::Symbol,
            Binding::Fixed(_, scale) => Type::Fixed(*scale),
        })
        .collect()
}
fn name(binding: &Binding) -> &str {
    match binding {
        Binding::Ref(name, _)
        | Binding::Record(name, _)
        | Binding::Bool(name)
        | Binding::Int(name)
        | Binding::Symbol(name)
        | Binding::Fixed(name, _) => name,
    }
}
struct Scope<'a> {
    source: &'a str,
    task: Option<&'a str>,
    declaration: &'a str,
    step: Option<usize>,
    names: BTreeMap<&'a str, htn_planner::Expression>,
}
impl<'a> Scope<'a> {
    fn new(
        source: &'a str,
        task: Option<&'a str>,
        declaration: &'a str,
        parameters: &'a [Binding],
        locals: &'a [Binding],
    ) -> Result<Self, Error> {
        let mut scope = Self {
            source,
            task,
            declaration,
            step: None,
            names: BTreeMap::new(),
        };
        for (local, bindings) in [(false, parameters), (true, locals)] {
            for (index, binding) in bindings.iter().enumerate() {
                let name = name(binding);
                if name.len() > 128
                    || !name
                        .bytes()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
                    || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                {
                    return Err(scope.error(if name.len() > 128 {
                        "variable name exceeds 128 bytes"
                    } else {
                        "invalid ASCII variable name"
                    }));
                }
                if scope
                    .names
                    .insert(
                        name,
                        if local {
                            htn_planner::Expression::Local(index)
                        } else {
                            htn_planner::Expression::Parameter(index)
                        },
                    )
                    .is_some()
                {
                    return Err(scope.error(&format!("duplicate or shadowed variable {name:?}")));
                }
            }
        }
        Ok(scope)
    }
    fn error(&self, message: &str) -> Error {
        let bounded = |s: &str| {
            if s.len() <= 1024 {
                s.to_owned()
            } else {
                "<oversized declaration>".into()
            }
        };
        Error {
            locator: Locator {
                source: bounded(self.source),
                task: self.task.map(bounded),
                declaration: bounded(self.declaration),
                step: self.step,
            },
            message: message.into(),
        }
    }
    fn expression(&self, expression: &Expression) -> Result<htn_planner::Expression, Error> {
        match expression {
            Expression::Literal(value) => Ok(htn_planner::Expression::Literal(value.clone())),
            Expression::Var(name) if name.len() > 128 => {
                Err(self.error("variable name exceeds 128 bytes"))
            }
            Expression::Var(name) => self
                .names
                .get(name.as_str())
                .cloned()
                .ok_or_else(|| self.error(&format!("unknown variable {name:?}"))),
        }
    }
    fn expressions(
        &self,
        expressions: &[Expression],
    ) -> Result<Vec<htn_planner::Expression>, Error> {
        expressions.iter().map(|e| self.expression(e)).collect()
    }
    fn condition(&self, condition: &Condition) -> Result<htn_planner::Condition, Error> {
        use htn_planner::Condition as C;
        Ok(match condition {
            Condition::Fact { name, arguments } => C::Fact {
                name: name.clone(),
                arguments: self.expressions(arguments)?,
            },
            Condition::Derived {
                name,
                arguments,
                outputs,
            } => C::Derived {
                name: name.clone(),
                arguments: self.expressions(arguments)?,
                outputs: self.expressions(outputs)?,
            },
            Condition::And(children) => C::And(self.conditions(children)?),
            Condition::Or(children) => C::Or(self.conditions(children)?),
            Condition::FirstSuccess(children) => C::FirstSuccess(self.conditions(children)?),
            Condition::Not(child) => C::Not(Box::new(self.condition(child)?)),
            Condition::Compare { op, left, right } => C::Compare {
                op: *op,
                left: self.expression(left)?,
                right: self.expression(right)?,
            },
            Condition::Calculate {
                op,
                left,
                right,
                output,
            } => C::Calculate {
                op: *op,
                left: self.expression(left)?,
                right: self.expression(right)?,
                output: self.expression(output)?,
            },
        })
    }
    fn conditions(&self, conditions: &[Condition]) -> Result<Vec<htn_planner::Condition>, Error> {
        conditions.iter().map(|c| self.condition(c)).collect()
    }
    fn lower_step(&self, step: &Step) -> Result<htn_planner::Step, Error> {
        use htn_planner::Step as S;
        Ok(match step {
            Step::Condition(condition) => S::Condition(self.condition(condition)?),
            Step::Effect { name, arguments } => S::Effect {
                name: name.clone(),
                arguments: self.expressions(arguments)?,
            },
            Step::Defer { name, arguments } => S::Defer {
                name: name.clone(),
                arguments: self.expressions(arguments)?,
            },
            Step::Query { name, arguments } => S::Query {
                name: name.clone(),
                arguments: self.expressions(arguments)?,
            },
            Step::Call {
                name,
                arguments,
                outputs,
            } => S::Call {
                name: name.clone(),
                arguments: self.expressions(arguments)?,
                outputs: self.expressions(outputs)?,
            },
            Step::Require { name, arguments } => S::Require {
                name: name.clone(),
                arguments: self.expressions(arguments)?,
            },
            Step::Action { name, arguments } => S::Action {
                name: name.clone(),
                arguments: self.expressions(arguments)?,
            },
            Step::Task { name, arguments } => S::Task {
                name: name.clone(),
                arguments: self.expressions(arguments)?,
            },
        })
    }
}

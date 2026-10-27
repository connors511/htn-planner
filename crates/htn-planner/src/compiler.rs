mod condition;
mod extensions;
use crate::admission::{known_type, value_size};
use crate::{
    Definition, Error, Expression, Limits, Locator, Method, Schema, Step, Task, Type, Value,
};
pub(crate) use condition::{CompiledCondition, CompiledQuery};
pub(crate) use extensions::CompiledEffect;

#[derive(Clone, Debug)]
pub(crate) struct CompiledTask {
    pub(crate) methods: Vec<CompiledMethod>,
}
#[derive(Clone, Debug)]
pub(crate) struct CompiledMethod {
    pub(crate) name: String,
    pub(crate) locals: usize,
    pub(crate) result: Vec<CompiledExpression>,
    pub(crate) steps: Vec<Instruction>,
}
#[derive(Clone, Debug)]
pub(crate) enum Instruction {
    Condition {
        condition: usize,
    },
    Effect {
        effect: usize,
        arguments: Vec<CompiledExpression>,
    },
    Defer {
        task: usize,
        arguments: Vec<CompiledExpression>,
    },
    Require {
        predicate: usize,
        arguments: Vec<CompiledExpression>,
    },
    Action {
        action: usize,
        arguments: Vec<CompiledExpression>,
    },
    Task {
        task: usize,
        arguments: Vec<CompiledExpression>,
        outputs: Vec<CompiledExpression>,
    },
}
#[derive(Clone, Debug)]
pub(crate) enum CompiledExpression {
    Literal { value: Value, cost: usize },
    Slot(usize),
}

fn error(source: &str, declaration: &str, step: Option<usize>, message: &str) -> Error {
    Error {
        locator: Locator {
            task: None,
            source: source.into(),
            declaration: if declaration.len() <= 1024 {
                declaration.into()
            } else {
                "<oversized declaration>".into()
            },
            step,
        },
        message: message.into(),
    }
}
pub(crate) fn compile_definition(
    source: &str,
    schema: &Schema,
    definition: &Definition,
    limits: Limits,
) -> Result<CompiledProgram, Error> {
    let fail = |declaration: &str, message: &str| error(source, declaration, None, message);
    if definition.tasks.len() > 1024 || definition.entries.len() > 1024 {
        return Err(fail(
            &definition.name,
            "domain declaration capacity exceeded",
        ));
    }
    let mut size = source.len() + definition.name.len();
    let mut names: Vec<&str> = Vec::new();
    for task in &definition.tasks {
        if task.name.is_empty() || task.name.len() > 1024 || names.contains(&task.name.as_str()) {
            return Err(fail(&task.name, "duplicate or empty task"));
        }
        names.push(&task.name);
        if task.parameters.len() > limits.bindings
            || task.outputs.len() > limits.bindings
            || task.methods.len() > 1024
        {
            return Err(fail(
                &task.name,
                "task parameter/output/method capacity exceeded",
            ));
        }
        size += 1 + task.name.len();
        for ty in task.parameters.iter().chain(&task.outputs) {
            if !known_type(schema, ty) {
                return Err(fail(&task.name, "invalid task type"));
            }
            size = size.saturating_add(crate::values::type_cost(ty));
        }
    }
    if definition.entries.is_empty() {
        return Err(fail(&definition.name, "invalid entry task"));
    }
    for entry in &definition.entries {
        if !names.contains(&entry.as_str()) {
            return Err(fail(&definition.name, "invalid entry task"));
        }
        size = size.saturating_add(1 + entry.len());
    }
    condition::validate_queries(source, schema, definition, limits, &mut size)?;
    let mut arena = Vec::new();
    let mut tasks = Vec::new();
    for task in &definition.tasks {
        let mut method_names: Vec<&str> = Vec::new();
        let mut methods = Vec::new();
        for method in &task.methods {
            if method.name.is_empty()
                || method.name.len() > 1024
                || method_names.contains(&method.name.as_str())
            {
                return Err(fail(&method.name, "duplicate or empty method"));
            }
            method_names.push(&method.name);
            size = size.saturating_add(1 + method.name.len() + method.steps.len());
            if method.steps.len() > 16384
                || task.parameters.len().saturating_add(method.locals.len()) > limits.bindings
            {
                return Err(fail(&method.name, "method step/local capacity exceeded"));
            }
            for ty in &method.locals {
                if !known_type(schema, ty) {
                    return Err(fail(&method.name, "invalid local type"));
                }
                size = size.saturating_add(crate::values::type_cost(ty));
            }
            if size > crate::values::MAX_ADMITTED_SIZE {
                return Err(fail(&method.name, "definition aggregate capacity exceeded"));
            }
            let mut context = MethodCompiler {
                source,
                schema,
                definition,
                task,
                method,
                bound: vec![false; method.locals.len()],
                size: &mut size,
                step: None,
                arena: &mut arena,
                max_depth: limits.depth,
            };
            let mut steps = Vec::new();
            for (index, step) in method.steps.iter().enumerate() {
                context.step = Some(index);
                steps.push(context.instruction(step)?);
            }
            context.step = None;
            let result = context.expressions(&method.result, &task.outputs, false)?;
            methods.push(CompiledMethod {
                name: method.name.clone(),
                locals: method.locals.len(),
                result,
                steps,
            });
        }
        tasks.push(CompiledTask { methods });
    }
    if size > crate::values::MAX_ADMITTED_SIZE {
        return Err(fail(
            &definition.name,
            "definition aggregate capacity exceeded",
        ));
    }
    validate_graph(source, definition, &tasks, limits.depth)?;
    let queries =
        condition::compile_queries(source, schema, definition, limits, &mut size, &mut arena)?;
    let effects =
        extensions::compile_effects(source, schema, definition, limits, &mut size, &mut arena)?;
    if size > crate::values::MAX_ADMITTED_SIZE {
        return Err(fail(
            &definition.name,
            "definition aggregate capacity exceeded",
        ));
    }
    Ok(CompiledProgram {
        tasks,
        conditions: arena,
        queries,
        effects,
    })
}
struct MethodCompiler<'a> {
    source: &'a str,
    schema: &'a Schema,
    definition: &'a Definition,
    task: &'a Task,
    method: &'a Method,
    bound: Vec<bool>,
    size: &'a mut usize,
    step: Option<usize>,
    arena: &'a mut Vec<CompiledCondition>,
    max_depth: usize,
}
impl MethodCompiler<'_> {
    fn fail(&self, message: &str) -> Error {
        error(self.source, &self.method.name, self.step, message)
    }
    fn query_prefix(&self, name: &str, arguments: &[Expression]) -> Result<(), Error> {
        if let Some(input) = self
            .schema
            .query_inputs
            .iter()
            .find(|q| q.predicate == name)
        {
            if arguments.len() < input.bound {
                return Err(self.fail("query input prefix arity mismatch"));
            }
            for expression in &arguments[..input.bound] {
                if let Expression::Local(slot) = expression
                    && !self.bound.get(*slot).copied().unwrap_or(false)
                {
                    return Err(self.fail("unbound query input prefix"));
                }
            }
        }
        Ok(())
    }
    fn expressions(
        &mut self,
        expressions: &[Expression],
        types: &[Type],
        bind: bool,
    ) -> Result<Vec<CompiledExpression>, Error> {
        if expressions.len() != types.len() {
            return Err(self.fail("arity mismatch"));
        }
        let mut compiled = Vec::new();
        for (expression, ty) in expressions.iter().zip(types) {
            let compiled_expression = match expression {
                Expression::Literal(value) => {
                    let cost = value_size(self.schema, ty, value)
                        .map_err(|_| self.fail("literal type mismatch"))?;
                    *self.size = self.size.saturating_add(cost);
                    if *self.size > crate::values::MAX_ADMITTED_SIZE {
                        return Err(self.fail("definition aggregate capacity exceeded"));
                    }
                    CompiledExpression::Literal {
                        value: value.clone(),
                        cost,
                    }
                }
                Expression::Parameter(slot) => {
                    if self.task.parameters.get(*slot) != Some(ty) {
                        return Err(self.fail("parameter type or slot mismatch"));
                    }
                    CompiledExpression::Slot(*slot)
                }
                Expression::Local(slot) => {
                    if self.method.locals.get(*slot) != Some(ty) {
                        return Err(self.fail("local type or slot mismatch"));
                    }
                    if !bind && !self.bound[*slot] {
                        return Err(self.fail("unbound local read"));
                    }
                    if bind {
                        self.bound[*slot] = true;
                    }
                    CompiledExpression::Slot(self.task.parameters.len() + slot)
                }
            };
            *self.size = self.size.saturating_add(1);
            if *self.size > crate::values::MAX_ADMITTED_SIZE {
                return Err(self.fail("definition aggregate capacity exceeded"));
            }
            compiled.push(compiled_expression);
        }
        Ok(compiled)
    }
    fn instruction(&mut self, step: &Step) -> Result<Instruction, Error> {
        if let Step::Condition(condition) = step {
            return Ok(Instruction::Condition {
                condition: self.condition(condition)?,
            });
        }
        let (name, arguments) = match step {
            Step::Condition(_) => return Err(self.fail("invalid condition instruction")),
            Step::Effect { name, arguments }
            | Step::Defer { name, arguments }
            | Step::Require { name, arguments }
            | Step::Query { name, arguments }
            | Step::Action { name, arguments }
            | Step::Task { name, arguments }
            | Step::Call {
                name, arguments, ..
            } => (name, arguments),
        };
        if name.len() > 1024 {
            return Err(self.fail("step reference name exceeds 1024 bytes"));
        }
        *self.size = self.size.saturating_add(name.len());
        match step {
            Step::Condition(_) => Err(self.fail("invalid condition instruction")),
            Step::Effect { .. } => {
                let effect = self
                    .schema
                    .effects
                    .iter()
                    .position(|effect| effect.name == *name)
                    .ok_or_else(|| self.fail("unknown effect"))?;
                Ok(Instruction::Effect {
                    effect,
                    arguments: self.expressions(
                        arguments,
                        &self.schema.effects[effect].parameters,
                        false,
                    )?,
                })
            }
            Step::Require { .. } | Step::Query { .. } => {
                self.query_prefix(name, arguments)?;
                let predicate = self
                    .schema
                    .predicates
                    .iter()
                    .position(|s| s.name == *name)
                    .ok_or_else(|| self.fail("unknown predicate"))?;
                let args = self.expressions(
                    arguments,
                    &self.schema.predicates[predicate].arguments,
                    matches!(step, Step::Query { .. }),
                )?;
                if matches!(step, Step::Query { .. }) {
                    let condition = self.arena.len();
                    self.arena.push(CompiledCondition::Fact {
                        predicate,
                        arguments: args,
                    });
                    Ok(Instruction::Condition { condition })
                } else {
                    Ok(Instruction::Require {
                        predicate,
                        arguments: args,
                    })
                }
            }
            Step::Action { .. } => {
                let action = self
                    .schema
                    .actions
                    .iter()
                    .position(|s| s.name == *name)
                    .ok_or_else(|| self.fail("unknown action"))?;
                Ok(Instruction::Action {
                    action,
                    arguments: self.expressions(
                        arguments,
                        &self.schema.actions[action].arguments,
                        false,
                    )?,
                })
            }
            Step::Defer { .. } | Step::Task { .. } | Step::Call { .. } => {
                let task = self
                    .definition
                    .tasks
                    .iter()
                    .position(|t| t.name == *name)
                    .ok_or_else(|| self.fail("unknown task"))?;
                let args =
                    self.expressions(arguments, &self.definition.tasks[task].parameters, false)?;
                if matches!(step, Step::Defer { .. }) {
                    if !self.definition.tasks[task].outputs.is_empty() {
                        return Err(self.fail("deferred task cannot return outputs"));
                    }
                    return Ok(Instruction::Defer {
                        task,
                        arguments: args,
                    });
                }
                let outputs = match step {
                    Step::Call { outputs, .. } => outputs.as_slice(),
                    _ => &[],
                };
                let outputs =
                    self.expressions(outputs, &self.definition.tasks[task].outputs, true)?;
                Ok(Instruction::Task {
                    task,
                    arguments: args,
                    outputs,
                })
            }
        }
    }
}
fn validate_graph(
    source: &str,
    definition: &Definition,
    tasks: &[CompiledTask],
    max_depth: usize,
) -> Result<(), Error> {
    let mut remaining = vec![0usize; tasks.len()];
    let mut parents = vec![Vec::new(); tasks.len()];
    for (parent, task) in tasks.iter().enumerate() {
        for instruction in task.methods.iter().flat_map(|m| &m.steps) {
            if let Instruction::Task { task: child, .. } | Instruction::Defer { task: child, .. } =
                instruction
            {
                remaining[parent] += 1;
                parents[*child].push(parent);
            }
        }
    }
    let mut ready: Vec<usize> = remaining
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect();
    let mut depths = vec![1usize; tasks.len()];
    let mut visited = 0;
    while let Some(child) = ready.pop() {
        visited += 1;
        for parent in &parents[child] {
            depths[*parent] = depths[*parent].max(depths[child] + 1);
            if depths[*parent] > max_depth {
                return Err(error(
                    source,
                    &definition.tasks[*parent].name,
                    None,
                    "task depth exceeds limit",
                ));
            }
            remaining[*parent] -= 1;
            if remaining[*parent] == 0 {
                ready.push(*parent);
            }
        }
    }
    if visited != tasks.len() {
        return Err(error(source, &definition.name, None, "recursive task"));
    }
    Ok(())
}

pub(crate) struct CompiledProgram {
    pub(crate) tasks: Vec<CompiledTask>,
    pub(crate) conditions: Vec<CompiledCondition>,
    pub(crate) queries: Vec<CompiledQuery>,
    pub(crate) effects: Vec<CompiledEffect>,
}

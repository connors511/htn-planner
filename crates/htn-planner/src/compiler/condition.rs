use super::{CompiledExpression, MethodCompiler, error};
use crate::admission::known_type;
use crate::{
    Comparison, Condition, Definition, Error, Expression, Limits, Method, NumericOperation, Schema,
    Task, Type, Value,
};
#[derive(Clone, Debug)]
pub(crate) enum CompiledCondition {
    Fact {
        predicate: usize,
        arguments: Vec<CompiledExpression>,
    },
    Derived {
        query: usize,
        arguments: Vec<CompiledExpression>,
        outputs: Vec<CompiledExpression>,
    },
    And(Vec<usize>),
    Or(Vec<usize>),
    FirstSuccess(Vec<usize>),
    Not(usize),
    Compare {
        op: Comparison,
        left: CompiledExpression,
        right: CompiledExpression,
    },
    Calculate {
        op: NumericOperation,
        left: CompiledExpression,
        right: CompiledExpression,
        output: CompiledExpression,
    },
}
#[derive(Clone, Debug)]
pub(crate) struct CompiledQuery {
    pub(crate) parameters: usize,
    pub(crate) locals: usize,
    pub(crate) root: usize,
    pub(crate) result: Vec<CompiledExpression>,
}
enum Visit<'a> {
    Enter(&'a Condition, bool, usize),
    Group {
        kind: u8,
        children: &'a [Condition],
        next: usize,
        ids: Vec<usize>,
        before: Vec<bool>,
        common: Option<Vec<bool>>,
        strict: bool,
        depth: usize,
    },
    Negated(Vec<bool>),
}
impl MethodCompiler<'_> {
    pub(super) fn condition(&mut self, root: &Condition) -> Result<usize, Error> {
        let mut pending = vec![Visit::Enter(root, false, 1)];
        let mut result = None;
        while let Some(visit) = pending.pop() {
            match visit {
                Visit::Enter(node, strict, depth) => {
                    if depth > self.max_depth {
                        return Err(self.fail("condition depth exceeds limit"));
                    }
                    *self.size = self.size.saturating_add(1);
                    if *self.size > 1_000_000 {
                        return Err(self.fail("condition aggregate capacity exceeded"));
                    }
                    if let Condition::Fact { name, .. } | Condition::Derived { name, .. } = node {
                        if name.len() > 1024 {
                            return Err(self.fail("condition name exceeds capacity"));
                        }
                        *self.size = self.size.saturating_add(name.len());
                    }
                    let compiled = match node {
                        Condition::Fact { name, arguments } => {
                            self.query_prefix(name, arguments)?;
                            let predicate = self
                                .schema
                                .predicates
                                .iter()
                                .position(|p| p.name == *name)
                                .ok_or_else(|| self.fail("unknown condition predicate"))?;
                            CompiledCondition::Fact {
                                predicate,
                                arguments: self.expressions(
                                    arguments,
                                    &self.schema.predicates[predicate].arguments,
                                    !strict,
                                )?,
                            }
                        }
                        Condition::Derived {
                            name,
                            arguments,
                            outputs,
                        } => {
                            let query = self
                                .definition
                                .queries
                                .iter()
                                .position(|q| q.name == *name)
                                .ok_or_else(|| self.fail("unknown derived query"))?;
                            let args = self.expressions(
                                arguments,
                                &self.definition.queries[query].parameters,
                                false,
                            )?;
                            let outputs = self.expressions(
                                outputs,
                                &self.definition.queries[query].outputs,
                                !strict,
                            )?;
                            CompiledCondition::Derived {
                                query,
                                arguments: args,
                                outputs,
                            }
                        }
                        Condition::And(children)
                        | Condition::Or(children)
                        | Condition::FirstSuccess(children) => {
                            if children.len() > 65536 {
                                return Err(
                                    self.fail("condition fanout exceeds admission capacity")
                                );
                            }
                            let kind = match node {
                                Condition::And(_) => 0,
                                Condition::Or(_) => 1,
                                _ => 2,
                            };
                            pending.push(Visit::Group {
                                kind,
                                children,
                                next: 0,
                                ids: Vec::new(),
                                before: self.bound.clone(),
                                common: None,
                                strict,
                                depth,
                            });
                            continue;
                        }
                        Condition::Not(child) => {
                            pending.push(Visit::Negated(self.bound.clone()));
                            pending.push(Visit::Enter(child, true, depth + 1));
                            continue;
                        }
                        Condition::Compare { op, left, right } => {
                            let ty = self.numeric_type(left)?;
                            let left = self
                                .expressions(
                                    std::slice::from_ref(left),
                                    std::slice::from_ref(&ty),
                                    false,
                                )?
                                .remove(0);
                            let right = self
                                .expressions(std::slice::from_ref(right), &[ty], false)?
                                .remove(0);
                            CompiledCondition::Compare {
                                op: *op,
                                left,
                                right,
                            }
                        }
                        Condition::Calculate {
                            op,
                            left,
                            right,
                            output,
                        } => {
                            let ty = self.numeric_type(left)?;
                            let left = self
                                .expressions(
                                    std::slice::from_ref(left),
                                    std::slice::from_ref(&ty),
                                    false,
                                )?
                                .remove(0);
                            let right = self
                                .expressions(
                                    std::slice::from_ref(right),
                                    std::slice::from_ref(&ty),
                                    false,
                                )?
                                .remove(0);
                            let outputs =
                                self.expressions(std::slice::from_ref(output), &[ty], !strict)?;
                            CompiledCondition::Calculate {
                                op: *op,
                                left,
                                right,
                                output: outputs[0].clone(),
                            }
                        }
                    };
                    let id = self.arena.len();
                    self.arena.push(compiled);
                    result = Some(id);
                }
                Visit::Negated(before) => {
                    self.bound = before;
                    let child = result
                        .take()
                        .ok_or_else(|| self.fail("missing negation child"))?;
                    let id = self.arena.len();
                    self.arena.push(CompiledCondition::Not(child));
                    result = Some(id);
                }
                Visit::Group {
                    kind,
                    children,
                    next,
                    mut ids,
                    before,
                    mut common,
                    strict,
                    depth,
                } => {
                    if next > 0 {
                        ids.push(
                            result
                                .take()
                                .ok_or_else(|| self.fail("missing condition child"))?,
                        );
                        if kind != 0 {
                            if let Some(bound) = &mut common {
                                for (slot, current) in bound.iter_mut().zip(&self.bound) {
                                    *slot &= *current;
                                }
                            } else {
                                common = Some(self.bound.clone());
                            }
                        }
                    }
                    if next < children.len() {
                        if kind != 0 {
                            self.bound.clone_from(&before);
                        }
                        pending.push(Visit::Group {
                            kind,
                            children,
                            next: next + 1,
                            ids,
                            before,
                            common,
                            strict,
                            depth,
                        });
                        pending.push(Visit::Enter(&children[next], strict, depth + 1));
                    } else {
                        if kind != 0 {
                            self.bound = common.unwrap_or(before);
                        }
                        let id = self.arena.len();
                        self.arena.push(match kind {
                            0 => CompiledCondition::And(ids),
                            1 => CompiledCondition::Or(ids),
                            _ => CompiledCondition::FirstSuccess(ids),
                        });
                        result = Some(id);
                    }
                }
            }
        }
        result.ok_or_else(|| self.fail("missing condition"))
    }
    fn numeric_type(&self, expression: &Expression) -> Result<Type, Error> {
        let ty = match expression {
            Expression::Parameter(slot) => self.task.parameters.get(*slot).cloned(),
            Expression::Local(slot) => self.method.locals.get(*slot).cloned(),
            Expression::Literal(Value::Int(_)) => Some(Type::Int),
            Expression::Literal(Value::Fixed { scale, .. }) => Some(Type::Fixed(*scale)),
            Expression::Literal(_) => None,
        };
        match ty {
            Some(ty @ (Type::Int | Type::Fixed(_))) => Ok(ty),
            _ => Err(self.fail("numeric operation requires Int or matching Fixed scale")),
        }
    }
}
pub(super) fn validate_queries(
    source: &str,
    schema: &Schema,
    definition: &Definition,
    limits: Limits,
    size: &mut usize,
) -> Result<(), Error> {
    if definition.queries.len() > 1024 {
        return Err(error(
            source,
            &definition.name,
            None,
            "too many derived queries",
        ));
    }
    let mut names = Vec::new();
    for query in &definition.queries {
        if query.name.is_empty() || query.name.len() > 1024 || names.contains(&query.name.as_str())
        {
            return Err(error(
                source,
                &query.name,
                None,
                "duplicate or invalid derived query",
            ));
        }
        names.push(query.name.as_str());
        if query.parameters.len().saturating_add(query.locals.len()) > limits.bindings
            || query.outputs.len() > limits.bindings
        {
            return Err(error(
                source,
                &query.name,
                None,
                "derived slot capacity exceeded",
            ));
        }
        *size = size.saturating_add(1 + query.name.len());
        for ty in query
            .parameters
            .iter()
            .chain(&query.outputs)
            .chain(&query.locals)
        {
            if !known_type(schema, ty) {
                return Err(error(source, &query.name, None, "unknown derived type"));
            }
            *size = size.saturating_add(crate::values::type_cost(ty));
        }
    }
    Ok(())
}
pub(super) fn compile_queries(
    source: &str,
    schema: &Schema,
    definition: &Definition,
    limits: Limits,
    size: &mut usize,
    arena: &mut Vec<CompiledCondition>,
) -> Result<Vec<CompiledQuery>, Error> {
    let mut queries = Vec::new();
    let mut dependencies = Vec::new();
    for query in &definition.queries {
        let task = Task {
            name: query.name.clone(),
            parameters: query.parameters.clone(),
            outputs: query.outputs.clone(),
            methods: vec![],
        };
        let method = Method {
            name: query.name.clone(),
            locals: query.locals.clone(),
            result: vec![],
            steps: vec![],
        };
        let mut context = MethodCompiler {
            source,
            schema,
            definition,
            task: &task,
            method: &method,
            bound: vec![false; query.locals.len()],
            size,
            step: None,
            arena,
            max_depth: limits.depth,
        };
        let start = context.arena.len();
        let root = context.condition(&query.condition)?;
        let result = context.expressions(&query.result, &query.outputs, false)?;
        dependencies.push(
            context.arena[start..]
                .iter()
                .filter_map(|node| {
                    if let CompiledCondition::Derived { query, .. } = node {
                        Some(*query)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>(),
        );
        queries.push(CompiledQuery {
            parameters: query.parameters.len(),
            locals: query.locals.len(),
            root,
            result,
        });
    }
    let mut left = dependencies.iter().map(Vec::len).collect::<Vec<_>>();
    let mut parents = vec![Vec::new(); queries.len()];
    for (parent, children) in dependencies.iter().enumerate() {
        for child in children {
            parents[*child].push(parent);
        }
    }
    let mut ready = left
        .iter()
        .enumerate()
        .filter_map(|(id, count)| (*count == 0).then_some(id))
        .collect::<Vec<_>>();
    let mut depths = vec![1usize; queries.len()];
    let mut visited = 0;
    while let Some(child) = ready.pop() {
        visited += 1;
        for parent in &parents[child] {
            depths[*parent] = depths[*parent].max(depths[child] + 1);
            if depths[*parent] > limits.depth {
                return Err(error(
                    source,
                    &definition.queries[*parent].name,
                    None,
                    "derived query depth exceeds limit",
                ));
            }
            left[*parent] -= 1;
            if left[*parent] == 0 {
                ready.push(*parent);
            }
        }
    }
    if visited != queries.len() {
        return Err(error(
            source,
            &definition.name,
            None,
            "recursive derived query",
        ));
    }
    Ok(queries)
}

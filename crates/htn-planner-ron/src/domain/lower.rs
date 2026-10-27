use super::{
    declarations::{Declaration, Program, literal},
    syntax::Node,
    types::Types,
};
use htn_planner::{
    Condition, Definition, DerivedQuery, Error, Expression, Method, Schema, Signature, Step, Task,
    Type, Value,
};
use std::collections::{BTreeMap, BTreeSet};

struct Scope {
    names: BTreeMap<String, (Expression, usize)>,
    locals: Vec<usize>,
    bound: BTreeSet<String>,
    possible: BTreeSet<String>,
}
struct Context<'a> {
    program: &'a Program,
    schema: &'a Schema,
    types: &'a mut Types,
}
impl Scope {
    fn new(decl: &Declaration) -> Self {
        let mut scope = Self {
            names: BTreeMap::new(),
            locals: vec![],
            bound: BTreeSet::new(),
            possible: BTreeSet::new(),
        };
        let mut input = 0;
        for param in &decl.parameters {
            let expr = if param.output {
                let index = scope.locals.len();
                scope.locals.push(param.id);
                Expression::Local(index)
            } else {
                scope.bound.insert(param.name.clone());
                scope.possible.insert(param.name.clone());
                let index = input;
                input += 1;
                Expression::Parameter(index)
            };
            scope.names.insert(param.name.clone(), (expr, param.id));
        }
        scope
    }
    fn expression(
        &mut self,
        node: &Node,
        ctx: &mut Context<'_>,
    ) -> Result<(Expression, usize), Error> {
        let text = atom(node)?;
        if text.starts_with('?') {
            if !text.strip_prefix('?').is_some_and(valid_name) {
                return Err(node.error("invalid variable name"));
            }
            if let Some(found) = self.names.get(text) {
                return Ok(found.clone());
            }
            if text.starts_with("?inp_") || text.starts_with("?out_") || text.starts_with("?io_") {
                return Err(node.error("parameter is not declared in this scope"));
            }
            let id = ctx.types.fresh();
            let expr = Expression::Local(self.locals.len());
            self.locals.push(id);
            self.names.insert(text.into(), (expr.clone(), id));
            return Ok((expr, id));
        }
        let value = if let Some(name) = text.strip_prefix('@') {
            ctx.program
                .constants
                .get(name)
                .cloned()
                .ok_or_else(|| node.error("unknown constant"))?
        } else {
            literal(node)?
        };
        let id = ctx.types.fresh();
        ctx.types.bind(
            id,
            &if matches!(value, Value::Int(_)) {
                Type::Int
            } else {
                Type::Symbol
            },
            node,
        )?;
        Ok((Expression::Literal(value), id))
    }
    fn require_bound(&self, node: &Node) -> Result<(), Error> {
        if node
            .atom()
            .is_some_and(|name| name.starts_with('?') && !self.bound.contains(name))
        {
            return Err(node.error("input must be bound on every preceding alternative"));
        }
        Ok(())
    }
    fn bind(&mut self, node: &Node) {
        if let Some(name) = node.atom().filter(|name| name.starts_with('?')) {
            self.bound.insert(name.into());
            self.possible.insert(name.into());
        }
    }
    fn arguments(
        &mut self,
        nodes: &[Node],
        sig: &Signature,
        ctx: &mut Context<'_>,
        at: &Node,
        bind: bool,
    ) -> Result<Vec<Expression>, Error> {
        if nodes.len() != sig.arguments.len() {
            return Err(at.error("argument arity mismatch"));
        }
        nodes
            .iter()
            .zip(&sig.arguments)
            .map(|(node, ty)| {
                let (expr, id) = self.expression(node, ctx)?;
                ctx.types.bind(id, ty, node)?;
                if bind {
                    self.bind(node);
                } else {
                    self.require_bound(node)?;
                }
                Ok(expr)
            })
            .collect()
    }
    fn condition(&mut self, node: &Node, ctx: &mut Context<'_>) -> Result<Condition, Error> {
        let form = list(node)?;
        if form.is_empty() {
            return Ok(Condition::And(vec![]));
        }
        let head = atom(&form[0])?;
        match head {
            "and" => {
                return Ok(Condition::And(
                    form[1..]
                        .iter()
                        .map(|node| self.condition(node, ctx))
                        .collect::<Result<_, _>>()?,
                ));
            }
            "alt" => {
                let initial = (self.bound.clone(), self.possible.clone());
                let mut definite: Option<BTreeSet<String>> = None;
                let mut possible = initial.1.clone();
                let mut children = Vec::new();
                for child in &form[1..] {
                    self.bound.clone_from(&initial.0);
                    self.possible.clone_from(&initial.1);
                    children.push(self.condition(child, ctx)?);
                    definite = Some(definite.map_or_else(
                        || self.bound.clone(),
                        |previous| previous.intersection(&self.bound).cloned().collect(),
                    ));
                    possible.extend(self.possible.iter().cloned());
                }
                self.bound = definite.unwrap_or(initial.0);
                self.possible = possible;
                return Ok(Condition::Or(children));
            }
            "not" => {
                let [_, child] = form else {
                    return Err(node.error("not requires one grounded condition"));
                };
                let before = (self.bound.clone(), self.possible.clone());
                let mut pending = vec![child];
                while let Some(part) = pending.pop() {
                    if let Some(children) = part.list() {
                        pending.extend(children);
                    } else {
                        self.require_bound(part)?;
                    }
                }
                let condition = self.condition(child, ctx)?;
                if self.possible != before.1 {
                    return Err(node.error("not requires grounded caller bindings; use private axiom locals for existential witnesses"));
                }
                self.bound = before.0;
                self.possible = before.1;
                return Ok(Condition::Not(Box::new(condition)));
            }
            "or" => return Err(node.error(
                "or first-result cut is unsupported; alt explicitly reopens ordered alternatives",
            )),
            "==" | "!=" | "<" | "<=" | ">" | ">=" => {
                let [_, left, right] = form else {
                    return Err(node.error("comparison requires two arguments"));
                };
                self.require_bound(left)?;
                self.require_bound(right)?;
                let (left_expr, left_id) = self.expression(left, ctx)?;
                let (right_expr, right_id) = self.expression(right, ctx)?;
                ctx.types.unify(left_id, right_id, node)?;
                let op = match head {
                    "==" => htn_planner::Comparison::Equal,
                    "!=" => htn_planner::Comparison::NotEqual,
                    "<" => htn_planner::Comparison::Less,
                    "<=" => htn_planner::Comparison::LessEqual,
                    ">" => htn_planner::Comparison::Greater,
                    _ => htn_planner::Comparison::GreaterEqual,
                };
                return Ok(Condition::Compare {
                    op,
                    left: left_expr,
                    right: right_expr,
                });
            }
            _ => {}
        }
        if let Some(name) = head.strip_prefix('#') {
            let decl = ctx
                .program
                .queries
                .iter()
                .find(|decl| decl.name == name)
                .ok_or_else(|| node.error("unknown axiom"))?;
            if decl.parameters.len() != form.len() - 1 {
                return Err(node.error("axiom arity mismatch"));
            }
            let mut arguments = vec![];
            let mut outputs = vec![];
            let mut bound_output = false;
            let before = self.bound.clone();
            let mut fresh = BTreeSet::new();
            for (actual, param) in form[1..].iter().zip(&decl.parameters) {
                let (expr, id) = self.expression(actual, ctx)?;
                ctx.types.unify(id, param.id, actual)?;
                if param.output {
                    if let Some(name) = actual.atom().filter(|name| name.starts_with('?')) {
                        if before.contains(name) {
                            bound_output = true;
                        } else if self.possible.contains(name) {
                            return Err(actual.error(
                                "out actual has branch-dependent binding; split the methods",
                            ));
                        }
                        if !fresh.insert(name) {
                            return Err(
                                actual.error("out actuals must be distinct fresh variables")
                            );
                        }
                    } else {
                        bound_output = true;
                    }
                    outputs.push(expr);
                } else {
                    self.require_bound(actual)?;
                    arguments.push(expr);
                }
            }
            // Strict upstream out checks occur before executing the private body.
            if bound_output {
                // Retain the unreachable call for static dependency/type admission:
                // a failing mode check must not conceal a recursive declaration.
                return Ok(Condition::And(vec![
                    Condition::Or(vec![]),
                    Condition::Derived {
                        name: name.into(),
                        arguments,
                        outputs,
                    },
                ]));
            }
            for (actual, param) in form[1..].iter().zip(&decl.parameters) {
                if param.output {
                    self.bind(actual);
                }
            }
            return Ok(Condition::Derived {
                name: name.into(),
                arguments,
                outputs,
            });
        }
        let sig = ctx
            .schema
            .predicates
            .iter()
            .find(|sig| sig.name == head)
            .ok_or_else(|| node.error(format!("unknown or unsupported condition {head}")))?;
        let prefix = ctx
            .schema
            .query_inputs
            .iter()
            .find(|query| query.predicate == head)
            .map_or(0, |query| query.bound);
        for argument in form[1..].iter().take(prefix) {
            self.require_bound(argument)?;
        }
        Ok(Condition::Fact {
            name: head.into(),
            arguments: self.arguments(&form[1..], sig, ctx, node, true)?,
        })
    }
    fn step(&mut self, node: &Node, ctx: &mut Context<'_>) -> Result<Step, Error> {
        let form = list(node)?;
        let head = atom(form.first().ok_or_else(|| node.error("empty task call"))?)?;
        if let Some(name) = head.strip_prefix('!') {
            let sig = ctx
                .schema
                .actions
                .iter()
                .find(|sig| sig.name == name)
                .ok_or_else(|| node.error("unknown action"))?;
            return Ok(Step::Action {
                name: name.into(),
                arguments: self.arguments(&form[1..], sig, ctx, node, false)?,
            });
        }
        let (name, deferred) = head
            .strip_prefix('&')
            .map_or((head, false), |name| (name, true));
        let decl = ctx
            .program
            .tasks
            .iter()
            .find(|decl| decl.name == name)
            .ok_or_else(|| node.error("unknown task"))?;
        if form.len() - 1 != decl.parameters.len() {
            return Err(node.error("task arity mismatch"));
        }
        let mut arguments = Vec::new();
        for (actual, param) in form[1..].iter().zip(&decl.parameters) {
            self.require_bound(actual)?;
            let (expr, id) = self.expression(actual, ctx)?;
            ctx.types.unify(id, param.id, actual)?;
            arguments.push(expr);
        }
        Ok(if deferred {
            Step::Defer {
                name: name.into(),
                arguments,
            }
        } else {
            Step::Task {
                name: name.into(),
                arguments,
            }
        })
    }
}
pub(super) fn list(node: &Node) -> Result<&[Node], Error> {
    node.list().ok_or_else(|| node.error("expected a list"))
}
pub(super) fn atom(node: &Node) -> Result<&str, Error> {
    node.atom()
        .ok_or_else(|| node.error("expected an identifier"))
}
pub(super) fn valid_name(name: &str) -> bool {
    name.len() <= 128
        && name
            .bytes()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

pub(super) fn lower(
    program: &Program,
    schema: &Schema,
    types: &mut Types,
) -> Result<Definition, Error> {
    let mut ctx = Context {
        program,
        schema,
        types,
    };
    let mut tasks = Vec::new();
    let mut task_locals = Vec::new();
    for decl in &program.tasks {
        let mut methods = Vec::new();
        let mut locals = Vec::new();
        for branch in &decl.body {
            let body = list(branch)?;
            if body.len() != 3 {
                return Err(branch.error("branch requires name, condition and task list"));
            }
            let name = atom(&body[0])?;
            if !valid_name(name) {
                return Err(body[0].error("invalid branch name"));
            }
            let mut scope = Scope::new(decl);
            let condition = scope.condition(&body[1], &mut ctx)?;
            let mut steps = vec![Step::Condition(condition)];
            for node in list(&body[2])? {
                steps.push(scope.step(node, &mut ctx)?);
            }
            locals.push(scope.locals);
            methods.push(Method {
                name: name.into(),
                locals: vec![],
                result: vec![],
                steps,
            });
        }
        task_locals.push(locals);
        tasks.push(Task {
            name: decl.name.clone(),
            parameters: vec![],
            outputs: vec![],
            methods,
        });
    }
    let mut queries = Vec::new();
    let mut query_locals = Vec::new();
    for decl in &program.queries {
        let mut scope = Scope::new(decl);
        let condition = scope.condition(&decl.body[0], &mut ctx)?;
        let mut result = vec![];
        for param in decl.parameters.iter().filter(|param| param.output) {
            if !scope.bound.contains(&param.name) {
                return Err(decl
                    .at
                    .error("axiom output is not bound on every successful alternative"));
            }
            result.push(scope.names[&param.name].0.clone());
        }
        query_locals.push(scope.locals);
        queries.push(DerivedQuery {
            name: decl.name.clone(),
            parameters: vec![],
            outputs: vec![],
            locals: vec![],
            condition,
            result,
        });
    }
    for ((task, decl), locals) in tasks.iter_mut().zip(&program.tasks).zip(task_locals) {
        task.parameters = decl
            .parameters
            .iter()
            .map(|param| ctx.types.resolve(param.id, &decl.at))
            .collect::<Result<_, _>>()?;
        for (method, ids) in task.methods.iter_mut().zip(locals) {
            method.locals = ids
                .into_iter()
                .map(|id| ctx.types.resolve(id, &decl.at))
                .collect::<Result<_, _>>()?;
        }
    }
    for ((query, decl), locals) in queries.iter_mut().zip(&program.queries).zip(query_locals) {
        query.parameters = decl
            .parameters
            .iter()
            .filter(|param| !param.output)
            .map(|param| ctx.types.resolve(param.id, &decl.at))
            .collect::<Result<_, _>>()?;
        query.outputs = decl
            .parameters
            .iter()
            .filter(|param| param.output)
            .map(|param| ctx.types.resolve(param.id, &decl.at))
            .collect::<Result<_, _>>()?;
        query.locals = locals
            .into_iter()
            .map(|id| ctx.types.resolve(id, &decl.at))
            .collect::<Result<_, _>>()?;
    }
    Ok(Definition {
        version: 1,
        name: program.name.clone(),
        entries: program
            .tasks
            .iter()
            .filter(|decl| decl.entry)
            .map(|decl| decl.name.clone())
            .collect(),
        tasks,
        queries,
    })
}

//! Structural payload sizes, independent of allocator bookkeeping and search logical units.
use crate::compiler::{CompiledCondition, CompiledExpression, Instruction};
use crate::{
    CompiledDomain, Condition, Definition, EffectOperation, Expression, Locator, PlanEntry, Schema,
    Step, Type, Value,
};

/// Owned backing storage, excluding inline self, allocator headers and traversal scratch.
///
/// `capacity_bytes` uses current String/Vec capacities. `clone_bytes` uses their lengths,
/// matching the payload requested by the current derived Clone implementations. Box payloads
/// count their complete inline pointee and its recursively owned allocations in both fields.
/// These are target-specific structural bytes, not allocation count, peak memory or RSS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeapFootprint {
    pub capacity_bytes: usize,
    pub clone_bytes: usize,
}
impl HeapFootprint {
    const fn string(&mut self, value: &String) {
        self.capacity_bytes += value.capacity();
        self.clone_bytes += value.len();
    }
    const fn vector<T>(&mut self, value: &Vec<T>) {
        self.capacity_bytes += value.capacity() * size_of::<T>();
        self.clone_bytes += value.len() * size_of::<T>();
    }
    const fn boxed<T>(&mut self) {
        self.capacity_bytes += size_of::<T>();
        self.clone_bytes += size_of::<T>();
    }
}

impl Definition {
    /// Measures all owned authoring payload, including derived queries and nested conditions.
    /// Traversal is iterative for recursive Conditions/Values, including unadmitted input.
    pub fn heap_footprint(&self) -> HeapFootprint {
        let mut walker = Walker::default();
        walker.definition(self);
        walker.finish()
    }
}
impl PlanEntry {
    /// Measures action/deferred arguments, names and locators; excludes inline `PlanEntry`.
    pub fn heap_footprint(&self) -> HeapFootprint {
        let mut walker = Walker::default();
        match self {
            Self::Action(action) => {
                walker.sizes.string(&action.name);
                walker.values(&action.arguments);
                walker.locator(&action.locator);
            }
            Self::Deferred(task) => {
                walker.sizes.string(&task.semantics);
                walker.sizes.string(&task.task);
                walker.values(&task.arguments);
                walker.locator(&task.locator);
            }
        }
        walker.finish()
    }
}
impl crate::Trace {
    /// Measures owned trace nodes, locators and argument payload, excluding inline self.
    /// Hosts sharing a trace through Arc count this once as immutable storage;
    /// an inspection deep copy uses `clone_bytes`. This is structural memory, not RSS.
    pub fn heap_footprint(&self) -> HeapFootprint {
        let mut walker = Walker::default();
        if let Some(task) = &self.focus_task {
            walker.sizes.string(task);
        }
        if let Some(declaration) = &self.focus_declaration {
            walker.sizes.string(declaration);
        }
        walker.sizes.vector(&self.nodes);
        for node in &self.nodes {
            walker.locator(&node.locator);
            match &node.arguments {
                crate::TraceArguments::Slots(slots) => {
                    walker.sizes.vector(slots);
                    walker
                        .pending
                        .extend(slots.iter().filter_map(Option::as_ref).map(Pending::Value));
                }
                crate::TraceArguments::UnvalidatedOmitted
                | crate::TraceArguments::NotApplicable => {}
            }
        }
        walker.finish()
    }
}
impl CompiledDomain {
    /// Measures raw schema/definition/source and all privately compiled owned programs.
    /// Inline domain fields and cached scalar identities/costs are not heap allocations.
    /// Capacity includes the reachable shared schema once for this domain; multiple
    /// domain/snapshot handles can reference that same allocation. Clone bytes
    /// exclude shared payload because cloning the Arc allocates no schema copy.
    pub fn heap_footprint(&self) -> HeapFootprint {
        let mut walker = Walker::default();
        walker.schema(&self.schema);
        // Cold schema copy is shared by snapshots; domain clones copy only its Arc.
        let mut shared = Walker::default();
        shared.schema(&self.snapshot_schema);
        walker.sizes.capacity_bytes += size_of::<Schema>() + shared.finish().capacity_bytes;
        walker.definition(&self.definition);
        walker.sizes.string(&self.source);
        walker.sizes.vector(&self.tasks);
        for task in &self.tasks {
            walker.sizes.vector(&task.methods);
            for method in &task.methods {
                walker.sizes.string(&method.name);
                walker.compiled_expressions(&method.result);
                walker.sizes.vector(&method.steps);
                for step in &method.steps {
                    match step {
                        Instruction::Condition { .. } => {}
                        Instruction::Effect { arguments, .. }
                        | Instruction::Defer { arguments, .. }
                        | Instruction::Require { arguments, .. }
                        | Instruction::Action { arguments, .. } => {
                            walker.compiled_expressions(arguments);
                        }
                        Instruction::Task {
                            arguments, outputs, ..
                        } => {
                            walker.compiled_expressions(arguments);
                            walker.compiled_expressions(outputs);
                        }
                    }
                }
            }
        }
        walker.sizes.vector(&self.query_bounds);
        walker.sizes.vector(&self.deferred_targets);
        walker.sizes.vector(&self.conditions);
        for condition in &self.conditions {
            walker.compiled_condition(condition);
        }
        walker.sizes.vector(&self.queries);
        for query in &self.queries {
            walker.compiled_expressions(&query.result);
        }
        walker.sizes.vector(&self.effects);
        for effect in &self.effects {
            walker.sizes.vector(&effect.operations);
            for operation in &effect.operations {
                walker.compiled_expressions(&operation.arguments);
            }
        }
        walker.finish()
    }
}

enum Pending<'a> {
    Value(&'a Value),
    Condition(&'a Condition),
}
#[derive(Default)]
struct Walker<'a> {
    sizes: HeapFootprint,
    pending: Vec<Pending<'a>>,
}
impl<'a> Walker<'a> {
    fn finish(mut self) -> HeapFootprint {
        while let Some(node) = self.pending.pop() {
            match node {
                Pending::Value(value) => match value {
                    Value::Symbol(name)
                    | Value::Ref {
                        type_name: name, ..
                    } => {
                        self.sizes.string(name);
                    }
                    Value::Record { type_name, fields } => {
                        self.sizes.string(type_name);
                        self.values(fields);
                    }
                    Value::Bool(_) | Value::Int(_) | Value::Fixed { .. } => {}
                },
                Pending::Condition(condition) => self.condition(condition),
            }
        }
        self.sizes
    }
    fn values(&mut self, values: &'a Vec<Value>) {
        self.sizes.vector(values);
        self.pending.extend(values.iter().map(Pending::Value));
    }
    fn expression(&mut self, expression: &'a Expression) {
        match expression {
            Expression::Literal(value) => self.pending.push(Pending::Value(value)),
            Expression::Parameter(_) | Expression::Local(_) => {}
        }
    }
    fn expressions(&mut self, expressions: &'a Vec<Expression>) {
        self.sizes.vector(expressions);
        for expression in expressions {
            self.expression(expression);
        }
    }
    fn compiled_expression(&mut self, expression: &'a CompiledExpression) {
        match expression {
            CompiledExpression::Literal { value, .. } => self.pending.push(Pending::Value(value)),
            CompiledExpression::Slot(_) => {}
        }
    }
    fn compiled_expressions(&mut self, expressions: &'a Vec<CompiledExpression>) {
        self.sizes.vector(expressions);
        for expression in expressions {
            self.compiled_expression(expression);
        }
    }
    fn strings(&mut self, strings: &Vec<String>) {
        self.sizes.vector(strings);
        for string in strings {
            self.sizes.string(string);
        }
    }
    fn types(&mut self, types: &Vec<Type>) {
        self.sizes.vector(types);
        for ty in types {
            match ty {
                Type::Ref(name) | Type::Record(name) => self.sizes.string(name),
                Type::Bool | Type::Int | Type::Fixed(_) | Type::Symbol => {}
            }
        }
    }
    const fn locator(&mut self, locator: &Locator) {
        if let Some(task) = &locator.task {
            self.sizes.string(task);
        }
        self.sizes.string(&locator.source);
        self.sizes.string(&locator.declaration);
    }
    fn definition(&mut self, definition: &'a Definition) {
        self.sizes.string(&definition.name);
        self.strings(&definition.entries);
        self.sizes.vector(&definition.tasks);
        for task in &definition.tasks {
            self.sizes.string(&task.name);
            self.types(&task.parameters);
            self.types(&task.outputs);
            self.sizes.vector(&task.methods);
            for method in &task.methods {
                self.sizes.string(&method.name);
                self.types(&method.locals);
                self.expressions(&method.result);
                self.sizes.vector(&method.steps);
                for step in &method.steps {
                    match step {
                        Step::Condition(condition) => {
                            self.pending.push(Pending::Condition(condition));
                        }
                        Step::Call {
                            name,
                            arguments,
                            outputs,
                        } => {
                            self.sizes.string(name);
                            self.expressions(arguments);
                            self.expressions(outputs);
                        }
                        Step::Effect { name, arguments }
                        | Step::Defer { name, arguments }
                        | Step::Query { name, arguments }
                        | Step::Require { name, arguments }
                        | Step::Action { name, arguments }
                        | Step::Task { name, arguments } => {
                            self.sizes.string(name);
                            self.expressions(arguments);
                        }
                    }
                }
            }
        }
        self.sizes.vector(&definition.queries);
        for query in &definition.queries {
            self.sizes.string(&query.name);
            self.types(&query.parameters);
            self.types(&query.outputs);
            self.types(&query.locals);
            self.pending.push(Pending::Condition(&query.condition));
            self.expressions(&query.result);
        }
    }
    fn schema(&mut self, schema: &'a Schema) {
        self.sizes.vector(&schema.query_inputs);
        for input in &schema.query_inputs {
            self.sizes.string(&input.predicate);
        }
        self.strings(&schema.references);
        self.sizes.vector(&schema.records);
        for record in &schema.records {
            self.sizes.string(&record.name);
            self.types(&record.fields);
        }
        self.sizes.vector(&schema.predicates);
        self.sizes.vector(&schema.actions);
        for signature in schema.predicates.iter().chain(&schema.actions) {
            self.sizes.string(&signature.name);
            self.types(&signature.arguments);
        }
        self.sizes.vector(&schema.effects);
        for effect in &schema.effects {
            self.sizes.string(&effect.name);
            self.types(&effect.parameters);
            self.sizes.vector(&effect.operations);
            for operation in &effect.operations {
                let (predicate, arguments) = match operation {
                    EffectOperation::Assert {
                        predicate,
                        arguments,
                    }
                    | EffectOperation::Retract {
                        predicate,
                        arguments,
                    } => (predicate, arguments),
                };
                self.sizes.string(predicate);
                self.expressions(arguments);
            }
        }
    }
    fn condition(&mut self, condition: &'a Condition) {
        match condition {
            Condition::Fact { name, arguments } => {
                self.sizes.string(name);
                self.expressions(arguments);
            }
            Condition::Derived {
                name,
                arguments,
                outputs,
            } => {
                self.sizes.string(name);
                self.expressions(arguments);
                self.expressions(outputs);
            }
            Condition::And(children)
            | Condition::Or(children)
            | Condition::FirstSuccess(children) => {
                self.sizes.vector(children);
                self.pending.extend(children.iter().map(Pending::Condition));
            }
            Condition::Not(child) => {
                self.sizes.boxed::<Condition>();
                self.pending.push(Pending::Condition(child));
            }
            Condition::Compare { left, right, .. } => {
                self.expression(left);
                self.expression(right);
            }
            Condition::Calculate {
                left,
                right,
                output,
                ..
            } => {
                self.expression(left);
                self.expression(right);
                self.expression(output);
            }
        }
    }
    fn compiled_condition(&mut self, condition: &'a CompiledCondition) {
        match condition {
            CompiledCondition::Fact { arguments, .. } => self.compiled_expressions(arguments),
            CompiledCondition::Derived {
                arguments, outputs, ..
            } => {
                self.compiled_expressions(arguments);
                self.compiled_expressions(outputs);
            }
            CompiledCondition::And(children)
            | CompiledCondition::Or(children)
            | CompiledCondition::FirstSuccess(children) => self.sizes.vector(children),
            CompiledCondition::Not(_) => {}
            CompiledCondition::Compare { left, right, .. } => {
                self.compiled_expression(left);
                self.compiled_expression(right);
            }
            CompiledCondition::Calculate {
                left,
                right,
                output,
                ..
            } => {
                self.compiled_expression(left);
                self.compiled_expression(right);
                self.compiled_expression(output);
            }
        }
    }
}

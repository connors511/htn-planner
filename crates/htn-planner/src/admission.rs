use crate::compiler::{CompiledTask, compile_definition};
use crate::rejected_input::Owned;
use crate::{Definition, Error, Fact, Limits, Locator, Schema, Type, Value};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct CompiledDomain {
    pub(crate) schema: Schema,
    pub(crate) snapshot_schema: Arc<Schema>,
    pub(crate) query_bounds: Vec<Option<usize>>,
    pub(crate) definition: Definition,
    pub(crate) tasks: Vec<CompiledTask>,
    pub(crate) limits: Limits,
    pub(crate) source: String,
    pub(crate) schema_cost: usize,
    pub(crate) identity: crate::DefinitionIdentity,
    pub(crate) deferred_targets: Vec<usize>,
    pub(crate) conditions: Vec<crate::compiler::CompiledCondition>,
    pub(crate) queries: Vec<crate::compiler::CompiledQuery>,
    pub(crate) effects: Vec<crate::compiler::CompiledEffect>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FactSnapshot {
    pub(crate) schema: Arc<Schema>,
    pub(crate) facts: Vec<Fact>,
    pub(crate) predicates: Vec<usize>,
    pub(crate) costs: Vec<usize>,
    pub(crate) argument_costs: Vec<Vec<usize>>,
    pub(crate) schema_cost: usize,
}

impl FactSnapshot {
    /// Input row order is retained. No sorting or host callbacks occur during planning.
    ///
    /// # Errors
    /// Rejects unknown predicates, arity/type mismatches and oversized values.
    pub fn new(schema: &Schema, facts: Vec<Fact>) -> Result<Self, String> {
        let facts = Owned::new(facts);
        let schema_cost = crate::values::schema_cost(schema)?;
        validate_schema(schema)?;
        Self::from_admitted(Arc::new(schema.clone()), schema_cost, facts.take())
    }
    fn from_admitted(
        schema: Arc<Schema>,
        schema_cost: usize,
        facts: Vec<Fact>,
    ) -> Result<Self, String> {
        let facts = Owned::new(facts);
        let mut costs = Vec::new();
        let mut argument_costs = Vec::new();
        let mut total = 0usize;
        for fact in facts.iter() {
            let signature = schema
                .predicates
                .iter()
                .find(|p| p.name == fact.predicate)
                .ok_or("unknown predicate")?;
            let row_costs = argument_costs_checked(&schema, &signature.arguments, &fact.arguments)?;
            let cost = row_costs.iter().sum::<usize>();
            total = total.saturating_add(1 + fact.predicate.len() + cost);
            if total > crate::values::MAX_ADMITTED_SIZE {
                return Err("fact snapshot aggregate capacity exceeded".into());
            }
            argument_costs.push(row_costs);
            costs.push(cost);
        }
        let predicates = facts
            .iter()
            .map(|f| {
                schema
                    .predicates
                    .iter()
                    .position(|p| p.name == f.predicate)
                    .unwrap_or(usize::MAX)
            })
            .collect();
        Ok(Self {
            schema,
            facts: facts.take(),
            predicates,
            costs,
            argument_costs,
            schema_cost,
        })
    }
}
impl CompiledDomain {
    /// Validate only new rows against this domain's shared immutable admitted schema.
    /// # Errors
    /// Rejects unknown predicates, wrong arity/types or oversized rows.
    pub fn snapshot(&self, facts: Vec<Fact>) -> Result<FactSnapshot, String> {
        FactSnapshot::from_admitted(Arc::clone(&self.snapshot_schema), self.schema_cost, facts)
    }
    /// Admit an immutable executable definition. Deserialization alone grants no execution capability.
    ///
    /// # Errors
    /// Returns a source/declaration/step locator for unsupported schema, references,
    /// types, recursion, or invalid limits. Admission never runs host actions.
    pub fn admit(
        source: impl Into<String>,
        schema: Schema,
        definition: Definition,
        limits: Limits,
    ) -> Result<Self, Error> {
        let schema = Owned::new(schema);
        let definition = Owned::new(definition);
        let source = source.into();
        let fail = |declaration: &str, step, message: &str| Error {
            locator: Locator {
                task: None,
                source: source.clone(),
                declaration: if declaration.len() <= 1024 {
                    declaration.into()
                } else {
                    "<oversized declaration>".into()
                },
                step,
            },
            message: message.into(),
        };
        if source.len() > 1024 || definition.name.len() > 1024 {
            return Err(Error {
                locator: Locator {
                    task: None,
                    source: "<oversized source>".into(),
                    declaration: "<domain>".into(),
                    step: None,
                },
                message: "source or domain name exceeds 1024 bytes".into(),
            });
        }
        if !limits.is_valid() {
            return Err(fail(&definition.name, None, "invalid limits"));
        }
        if definition.version != 1 {
            return Err(fail(
                &definition.name,
                None,
                "unsupported definition version",
            ));
        }
        let schema_cost = crate::values::schema_cost(&schema)
            .map_err(|message| fail(&definition.name, None, &message))?;
        validate_schema(&schema).map_err(|message| fail(&definition.name, None, &message))?;
        let program = compile_definition(&source, &schema, &definition, limits)?;
        let tasks = program.tasks;
        let mut deferred_targets = Vec::new();
        for instruction in tasks
            .iter()
            .flat_map(|task| &task.methods)
            .flat_map(|method| &method.steps)
        {
            if let crate::compiler::Instruction::Defer { task, .. } = instruction
                && !deferred_targets.contains(task)
            {
                deferred_targets.push(*task);
            }
        }
        let identity =
            crate::identity::compute_identity(&schema, &definition, &limits).map_err(|_| {
                fail(
                    &definition.name,
                    None,
                    "unsupported definition identity representation",
                )
            })?;
        let query_bounds = schema
            .predicates
            .iter()
            .map(|p| {
                schema
                    .query_inputs
                    .iter()
                    .find(|q| q.predicate == p.name)
                    .map(|q| q.bound)
            })
            .collect();
        let snapshot_schema = Arc::new((*schema).clone());
        Ok(Self {
            snapshot_schema,
            query_bounds,
            schema: schema.take(),
            definition: definition.take(),
            tasks,
            limits,
            source,
            schema_cost,
            identity,
            deferred_targets,
            conditions: program.conditions,
            queries: program.queries,
            effects: program.effects,
        })
    }
    /// Validate concrete inputs for any declared task, including private tasks.
    ///
    /// This checks the admitted schema only; planning may still exhaust work or storage.
    /// It grants no execution capability and does not consume a planning budget.
    ///
    /// # Errors
    /// Rejects unknown tasks, mismatched arity/types, and values beyond admission limits.
    pub fn validate_task_inputs(&self, task: &str, arguments: &[Value]) -> Result<(), Error> {
        let bounded = task.len() <= 1024;
        let fail = |message: &str| Error {
            locator: Locator {
                source: self.source.clone(),
                task: bounded.then(|| task.to_owned()),
                declaration: if bounded {
                    task.to_owned()
                } else {
                    "<oversized task>".into()
                },
                step: None,
            },
            message: message.into(),
        };
        if !bounded {
            return Err(fail("task name exceeds 1024 bytes"));
        }
        let declared = self
            .definition
            .tasks
            .iter()
            .find(|candidate| candidate.name == task)
            .ok_or_else(|| fail("unknown task"))?;
        let size = check_arguments(&self.schema, &declared.parameters, arguments)
            .map_err(|message| fail(&message))?;
        if size > crate::values::MAX_ADMITTED_SIZE {
            return Err(fail("task inputs aggregate capacity exceeded"));
        }
        Ok(())
    }
    /// The schema admitted with this immutable definition.
    pub const fn schema(&self) -> &Schema {
        &self.schema
    }
    /// The validated source definition for portable content export.
    pub const fn definition(&self) -> &Definition {
        &self.definition
    }
    pub const fn limits(&self) -> Limits {
        self.limits
    }
    pub fn name(&self) -> &str {
        &self.definition.name
    }
}
pub(crate) fn known_type(schema: &Schema, value: &Type) -> bool {
    match value {
        Type::Ref(name) => schema.references.contains(name),
        Type::Record(name) => schema.records.iter().any(|r| r.name == *name),
        Type::Fixed(scale) => *scale <= 18,
        _ => true,
    }
}
fn validate_schema(schema: &Schema) -> Result<(), String> {
    for (index, input) in schema.query_inputs.iter().enumerate() {
        let signature = schema
            .predicates
            .iter()
            .find(|p| p.name == input.predicate)
            .ok_or("unknown query input predicate")?;
        if input.predicate.len() > 1024
            || input.bound > signature.arguments.len()
            || schema.query_inputs[..index]
                .iter()
                .any(|q| q.predicate == input.predicate)
        {
            return Err("invalid or duplicate query input prefix".into());
        }
    }
    for names in [
        schema.references.clone(),
        schema.records.iter().map(|r| r.name.clone()).collect(),
        schema.predicates.iter().map(|s| s.name.clone()).collect(),
        schema.actions.iter().map(|s| s.name.clone()).collect(),
        schema
            .effects
            .iter()
            .map(|effect| effect.name.clone())
            .collect(),
    ] {
        for (index, name) in names.iter().enumerate() {
            if name.is_empty() || name.len() > 1024 || names[..index].contains(name) {
                return Err("duplicate or empty schema name".into());
            }
        }
    }
    for ty in schema
        .records
        .iter()
        .flat_map(|r| &r.fields)
        .chain(
            schema
                .predicates
                .iter()
                .chain(&schema.actions)
                .flat_map(|s| &s.arguments),
        )
        .chain(schema.effects.iter().flat_map(|effect| &effect.parameters))
    {
        if !known_type(schema, ty) {
            return Err("unregistered schema type".into());
        }
    }
    Ok(())
}
pub(crate) fn check_arguments(
    schema: &Schema,
    types: &[Type],
    values: &[Value],
) -> Result<usize, String> {
    argument_costs_checked(schema, types, values).map(|costs| costs.into_iter().sum())
}
fn argument_costs_checked(
    schema: &Schema,
    types: &[Type],
    values: &[Value],
) -> Result<Vec<usize>, String> {
    let mut remaining = 16_000_000usize;
    crate::values::validate_values(schema, types, values, 1024, &mut 0, |units| {
        remaining = remaining
            .checked_sub(units)
            .ok_or(crate::Outcome::CapacityExceeded)?;
        Ok(())
    })
    .map_err(|error| format!("{error:?}"))
}
pub(crate) fn value_size(schema: &Schema, ty: &Type, value: &Value) -> Result<usize, String> {
    check_arguments(
        schema,
        std::slice::from_ref(ty),
        std::slice::from_ref(value),
    )
}

impl Schema {
    /// Check a borrowed schema without cloning its untrusted literal trees.
    /// This grants no execution capability or lasting trust after mutation.
    ///
    /// # Errors
    /// Rejects unresolved names/types/effects and admission capacity violations.
    pub fn validate_ref(&self) -> Result<(), String> {
        crate::values::schema_cost(self)?;
        validate_schema(self)
    }
    /// Validate this owned authoring schema, returning it unchanged on success.
    /// This grants no execution capability or lasting trust for subsequent mutation.
    /// Rejection disposes owned literal values iteratively, outside planning limits.
    ///
    /// # Errors
    /// Rejects unresolved names/types/effects and schema admission capacity violations.
    pub fn validate(self) -> Result<Self, String> {
        let schema = Owned::new(self);
        schema.validate_ref()?;
        Ok(schema.take())
    }
}

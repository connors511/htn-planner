use crate::{Outcome, Schema, Type, Value};

pub(crate) const MAX_ADMITTED_SIZE: usize = 1_000_000;

/// Bounds and charges every caller-controlled traversal before its operation.
pub(crate) fn validate_values(
    schema: &Schema,
    types: &[Type],
    values: &[Value],
    scratch_limit: usize,
    scratch_peak: &mut usize,
    mut charge: impl FnMut(usize) -> Result<(), Outcome>,
) -> Result<Vec<usize>, Outcome> {
    charge(1)?;
    if types.len() != values.len() {
        return Err(Outcome::InvalidInput);
    }
    if values.len().saturating_add(1) > scratch_limit {
        return Err(Outcome::CapacityExceeded);
    }
    let mut costs = Vec::new();
    let mut stack = ValidationStack::default();
    for (ty, value) in types.iter().zip(values) {
        *scratch_peak = (*scratch_peak).max(1 + costs.len());
        stack.push((ty, value, 0usize));
        let mut size = 0usize;
        while let Some((ty, value, depth)) = stack.pop() {
            charge(1)?;
            size = size.saturating_add(1);
            if size > MAX_ADMITTED_SIZE || depth > 128 {
                return Err(Outcome::CapacityExceeded);
            }
            match (ty, value) {
                (Type::Bool, Value::Bool(_)) | (Type::Int, Value::Int(_)) => {}
                (Type::Fixed(expected), Value::Fixed { scale, .. })
                    if expected == scale && *scale <= 18 => {}
                (Type::Symbol, Value::Symbol(s)) => {
                    charge(s.len())?;
                    size = size.saturating_add(s.len());
                }
                (Type::Ref(expected), Value::Ref { type_name, .. }) => {
                    charge(expected.len().saturating_add(type_name.len()))?;
                    if expected != type_name {
                        return Err(Outcome::InvalidInput);
                    }
                    let mut found = false;
                    for name in &schema.references {
                        charge(name.len().saturating_add(type_name.len()).saturating_add(1))?;
                        if name == type_name {
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        return Err(Outcome::InvalidInput);
                    }
                    size = size.saturating_add(type_name.len());
                }
                (Type::Record(expected), Value::Record { type_name, fields }) => {
                    charge(expected.len().saturating_add(type_name.len()))?;
                    if expected != type_name {
                        return Err(Outcome::InvalidInput);
                    }
                    let mut found = None;
                    for record in &schema.records {
                        charge(
                            record
                                .name
                                .len()
                                .saturating_add(type_name.len())
                                .saturating_add(1),
                        )?;
                        if record.name == *type_name {
                            found = Some(record);
                            break;
                        }
                    }
                    let record = found.ok_or(Outcome::InvalidInput)?;
                    if fields.len() != record.fields.len() {
                        return Err(Outcome::InvalidInput);
                    }
                    if stack
                        .len()
                        .saturating_add(fields.len())
                        .saturating_add(costs.len())
                        .saturating_add(1)
                        > scratch_limit.min(1024)
                    {
                        return Err(Outcome::CapacityExceeded);
                    }
                    charge(fields.len())?;
                    *scratch_peak =
                        (*scratch_peak).max(stack.len() + fields.len() + costs.len() + 1);
                    for (ty, value) in record.fields.iter().zip(fields) {
                        stack.push((ty, value, depth + 1));
                    }
                    size = size.saturating_add(type_name.len());
                }
                _ => return Err(Outcome::InvalidInput),
            }
        }
        if size > MAX_ADMITTED_SIZE {
            return Err(Outcome::CapacityExceeded);
        }
        *scratch_peak = (*scratch_peak).max(costs.len() + 1);
        costs.push(size);
    }
    Ok(costs)
}

pub(crate) fn schema_cost(schema: &Schema) -> Result<usize, String> {
    let mut cost = 1usize;
    if schema
        .references
        .len()
        .saturating_add(schema.records.len())
        .saturating_add(schema.predicates.len())
        .saturating_add(schema.actions.len())
        > 4096
    {
        return Err("schema declaration capacity exceeded".into());
    }
    for name in &schema.references {
        cost = cost.saturating_add(1 + name.len());
    }
    for record in &schema.records {
        cost = cost.saturating_add(1 + record.name.len());
        if record.fields.len() > 1024 {
            return Err("record field capacity exceeded".into());
        }
        for ty in &record.fields {
            cost = cost.saturating_add(type_cost(ty));
        }
    }
    for signature in schema.predicates.iter().chain(&schema.actions) {
        cost = cost.saturating_add(1 + signature.name.len());
        if signature.arguments.len() > 1024 {
            return Err("signature capacity exceeded".into());
        }
        for ty in &signature.arguments {
            cost = cost.saturating_add(type_cost(ty));
        }
    }
    if schema.effects.len() > 4096 {
        return Err("effect declaration capacity exceeded".into());
    }
    for effect in &schema.effects {
        if effect.name.len() > 1024
            || effect.parameters.len() > 1024
            || effect.operations.len() > 16384
        {
            return Err("effect declaration capacity exceeded".into());
        }
        cost = cost.saturating_add(1 + effect.name.len());
        for ty in &effect.parameters {
            cost = cost.saturating_add(type_cost(ty));
        }
        for operation in &effect.operations {
            let (predicate, arguments) = match operation {
                crate::EffectOperation::Assert {
                    predicate,
                    arguments,
                }
                | crate::EffectOperation::Retract {
                    predicate,
                    arguments,
                } => (predicate, arguments),
            };
            let signature = schema
                .predicates
                .iter()
                .find(|p| p.name == *predicate)
                .ok_or("unknown effect predicate")?;
            if arguments.len() != signature.arguments.len() {
                return Err("effect operation arity mismatch".into());
            }
            cost = cost.saturating_add(1 + predicate.len());
            for (expression, ty) in arguments.iter().zip(&signature.arguments) {
                cost = cost.saturating_add(
                    1 + match expression {
                        crate::Expression::Literal(value) => {
                            crate::admission::value_size(schema, ty, value)?
                        }
                        crate::Expression::Parameter(slot) => {
                            if effect.parameters.get(*slot) != Some(ty) {
                                return Err("effect operation parameter type mismatch".into());
                            }
                            0
                        }
                        crate::Expression::Local(_) => {
                            return Err("effect operation cannot use local slots".into());
                        }
                    },
                );
                if cost > MAX_ADMITTED_SIZE {
                    return Err("schema effect capacity exceeded".into());
                }
            }
        }
    }
    if schema.query_inputs.len() > 4096 {
        return Err("query input declaration capacity exceeded".into());
    }
    for input in &schema.query_inputs {
        cost = cost.saturating_add(1 + input.predicate.len());
    }
    if cost > MAX_ADMITTED_SIZE {
        return Err("schema aggregate capacity exceeded".into());
    }
    Ok(cost)
}
pub(crate) const fn type_cost(ty: &Type) -> usize {
    match ty {
        Type::Ref(name) | Type::Record(name) => 1 + name.len(),
        _ => 1,
    }
}

#[cfg(test)]
mod tests;

// Four inline entries cover the measured ordinary rows; larger admitted traversals spill.
type PendingValue<'a> = (&'a Type, &'a Value, usize);
#[derive(Default)]
struct ValidationStack<'a> {
    inline: [Option<PendingValue<'a>>; 4],
    overflow: Vec<PendingValue<'a>>,
    len: usize,
}
impl<'a> ValidationStack<'a> {
    const fn len(&self) -> usize {
        self.len
    }
    fn push(&mut self, pending: PendingValue<'a>) {
        if self.len < self.inline.len() {
            self.inline[self.len] = Some(pending);
        } else {
            self.overflow.push(pending);
        }
        self.len += 1;
    }
    fn pop(&mut self) -> Option<PendingValue<'a>> {
        self.len = self.len.checked_sub(1)?;
        if self.len < self.inline.len() {
            self.inline[self.len].take()
        } else {
            self.overflow.pop()
        }
    }
}

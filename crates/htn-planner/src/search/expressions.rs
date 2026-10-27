use super::state::{Arguments, Bindings, Meter};
use crate::compiler::CompiledExpression;
use crate::{Outcome, Value};

pub(super) fn value<'a>(
    expression: &'a CompiledExpression,
    env: &'a Bindings,
) -> Result<(&'a Value, usize), Outcome> {
    match expression {
        CompiledExpression::Literal { value, cost } => Ok((value, *cost)),
        CompiledExpression::Slot(slot) => Ok((
            env.values
                .get(*slot)
                .and_then(Option::as_ref)
                .ok_or(Outcome::InvalidInput)?,
            *env.costs.get(*slot).ok_or(Outcome::InvalidInput)?,
        )),
    }
}
pub(super) fn size(
    expressions: &[CompiledExpression],
    env: &Bindings,
    meter: &mut Meter,
) -> Result<usize, Outcome> {
    let mut total = 2 * expressions.len();
    for expression in expressions {
        meter.charge(1)?;
        total = total.saturating_add(value(expression, env)?.1);
    }
    Ok(total)
}
/// Caller reserves this complete cached size before invoking this copying operation.
pub(super) fn evaluate(
    expressions: &[CompiledExpression],
    env: &Bindings,
    size: usize,
    meter: &mut Meter,
) -> Result<Arguments, Outcome> {
    meter.charge(size)?;
    Input::Expressions {
        expressions,
        env,
        size,
    }
    .own()
}

/// Borrow the exact key until a cache miss; logical copying charges remain at the caller.
#[derive(Clone, Copy)]
pub(super) enum Input<'a> {
    Expressions {
        expressions: &'a [CompiledExpression],
        env: &'a Bindings,
        size: usize,
    },
    Values {
        values: &'a [Value],
        costs: &'a [usize],
        size: usize,
    },
}
impl Input<'_> {
    pub const fn size(self) -> usize {
        match self {
            Self::Expressions { size, .. } | Self::Values { size, .. } => size,
        }
    }
    pub fn matches(self, stored: &[Value]) -> Result<bool, Outcome> {
        match self {
            Self::Values { values, .. } => Ok(values == stored),
            Self::Expressions {
                expressions, env, ..
            } => {
                if expressions.len() != stored.len() {
                    return Ok(false);
                }
                for (expression, stored) in expressions.iter().zip(stored) {
                    if value(expression, env)?.0 != stored {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }
    pub fn own(self) -> Result<Arguments, Outcome> {
        match self {
            Self::Values {
                values,
                costs,
                size,
            } => Ok(Arguments {
                values: values.to_vec(),
                costs: costs.to_vec(),
                size,
            }),
            Self::Expressions {
                expressions,
                env,
                size,
            } => {
                let mut output = Arguments {
                    values: Vec::new(),
                    costs: Vec::new(),
                    size,
                };
                for expression in expressions {
                    let (value, cost) = value(expression, env)?;
                    output.values.push(value.clone());
                    output.costs.push(cost);
                }
                Ok(output)
            }
        }
    }
}

/// Preview without mutation/allocation, including same-tuple repeated slots.
pub(super) fn compatible(
    expressions: &[CompiledExpression],
    values: &[Value],
    costs: &[usize],
    env: &Bindings,
    meter: &mut Meter,
) -> Result<bool, Outcome> {
    if expressions.len() != values.len() || costs.len() != values.len() {
        return Err(Outcome::InvalidInput);
    }
    for (index, expression) in expressions.iter().enumerate() {
        meter.charge(1)?;
        match expression {
            CompiledExpression::Slot(slot)
                if env
                    .values
                    .get(*slot)
                    .ok_or(Outcome::InvalidInput)?
                    .is_none() =>
            {
                for earlier in 0..index {
                    meter.charge(1)?;
                    if matches!(expressions[earlier],CompiledExpression::Slot(previous) if previous == *slot)
                    {
                        meter.charge(costs[earlier].saturating_add(costs[index]))?;
                        if values[earlier] != values[index] {
                            return Ok(false);
                        }
                        break;
                    }
                }
            }
            _ => {
                let (bound, cost) = value(expression, env)?;
                meter.charge(cost.saturating_add(costs[index]))?;
                if *bound != values[index] {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}
pub(super) fn additional(
    expressions: &[CompiledExpression],
    costs: &[usize],
    env: &Bindings,
    meter: &mut Meter,
) -> Result<usize, Outcome> {
    let mut extra = 0usize;
    for (index, expression) in expressions.iter().enumerate() {
        meter.charge(1)?;
        if let CompiledExpression::Slot(slot) = expression
            && env
                .values
                .get(*slot)
                .ok_or(Outcome::InvalidInput)?
                .is_none()
        {
            let mut repeated = false;
            for earlier in &expressions[..index] {
                meter.charge(1)?;
                if matches!(earlier,CompiledExpression::Slot(previous) if previous == slot) {
                    repeated = true;
                    break;
                }
            }
            if !repeated {
                extra = extra.saturating_add(*costs.get(index).ok_or(Outcome::InvalidInput)?);
            }
        }
    }
    Ok(extra)
}
/// Only admitted empty local slots are filled; existing values were checked by preview.
pub(super) fn bind(
    expressions: &[CompiledExpression],
    values: &[Value],
    costs: &[usize],
    env: &mut Bindings,
    meter: &mut Meter,
) -> Result<(), Outcome> {
    for (index, expression) in expressions.iter().enumerate() {
        meter.charge(1)?;
        if let CompiledExpression::Slot(slot) = expression
            && env
                .values
                .get(*slot)
                .ok_or(Outcome::InvalidInput)?
                .is_none()
        {
            meter.charge(costs[index])?;
            env.values[*slot] = Some(values[index].clone());
            env.costs[*slot] = costs[index];
            env.size += costs[index];
        }
    }
    Ok(())
}

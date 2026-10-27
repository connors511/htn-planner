use crate::{Value, compiler::CompiledExpression};
#[derive(Clone, Copy)]
pub(crate) enum Snapshot<'a> {
    Integers(&'a [i64]),
    Slots(&'a [Option<Value>], &'a [usize]),
    Expressions(&'a [CompiledExpression], &'a [Option<Value>], &'a [usize]),
    Values(&'a [Value], &'a [usize]),
    Numeric {
        left: &'a CompiledExpression,
        right: &'a CompiledExpression,
        output: Option<&'a CompiledExpression>,
        values: &'a [Option<Value>],
        costs: &'a [usize],
    },
}
impl<'a> Snapshot<'a> {
    const fn len(self) -> usize {
        match self {
            Self::Integers(values) => values.len(),
            Self::Slots(values, _) => values.len(),
            Self::Expressions(expressions, ..) => expressions.len(),
            Self::Values(values, _) => values.len(),
            Self::Numeric { output, .. } => {
                if output.is_some() {
                    3
                } else {
                    2
                }
            }
        }
    }
    fn value(self, index: usize) -> (Option<&'a Value>, usize) {
        match self {
            // Integers are handled directly by cost/copy without temporary Values.
            Self::Integers(_) => (None, 0),
            Self::Slots(values, costs) => (values[index].as_ref(), costs[index]),
            Self::Values(values, costs) => (Some(&values[index]), costs[index]),
            Self::Expressions(expressions, values, costs) => {
                expression(&expressions[index], values, costs)
            }
            Self::Numeric {
                left,
                right,
                output,
                values,
                costs,
            } => {
                let selected = match index {
                    0 => Some(left),
                    1 => Some(right),
                    _ => output,
                };
                selected.map_or((None, 0), |selected| expression(selected, values, costs))
            }
        }
    }
    pub fn cost(self, remaining: usize) -> usize {
        if let Self::Integers(values) = self {
            return values.len().saturating_mul(2);
        }
        let mut cost = 0usize;
        for index in 0..self.len() {
            let (value, size) = self.value(index);
            cost = cost.saturating_add(1 + if value.is_some() { size } else { 0 });
            if cost > remaining {
                break;
            }
        }
        cost
    }
    pub fn copy(self) -> Vec<Option<Value>> {
        if let Self::Integers(values) = self {
            return values
                .iter()
                .map(|value| Some(Value::Int(*value)))
                .collect();
        }
        (0..self.len())
            .map(|index| self.value(index).0.cloned())
            .collect()
    }
}

fn expression<'a>(
    expression: &'a CompiledExpression,
    values: &'a [Option<Value>],
    costs: &[usize],
) -> (Option<&'a Value>, usize) {
    match expression {
        CompiledExpression::Literal { value, cost } => (Some(value), *cost),
        CompiledExpression::Slot(slot) => (values[*slot].as_ref(), costs[*slot]),
    }
}

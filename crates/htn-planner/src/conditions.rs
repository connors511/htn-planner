use crate::{Expression, Type};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedQuery {
    pub name: String,
    pub parameters: Vec<Type>,
    pub outputs: Vec<Type>,
    pub locals: Vec<Type>,
    pub condition: Condition,
    pub result: Vec<Expression>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Condition {
    Fact {
        name: String,
        arguments: Vec<Expression>,
    },
    Derived {
        name: String,
        arguments: Vec<Expression>,
        outputs: Vec<Expression>,
    },
    And(Vec<Self>),
    Or(Vec<Self>),
    FirstSuccess(Vec<Self>),
    Not(Box<Self>),
    Compare {
        op: Comparison,
        left: Expression,
        right: Expression,
    },
    Calculate {
        op: NumericOperation,
        left: Expression,
        right: Expression,
        output: Expression,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Comparison {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumericOperation {
    Add,
    Subtract,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningEffect {
    pub name: String,
    pub parameters: Vec<Type>,
    pub operations: Vec<EffectOperation>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum EffectOperation {
    Assert {
        predicate: String,
        arguments: Vec<Expression>,
    },
    Retract {
        predicate: String,
        arguments: Vec<Expression>,
    },
}

// Rejected programmatic controls and literal values may exceed admitted depth.
impl Drop for Condition {
    fn drop(&mut self) {
        crate::rejected_input::discard_condition(self);
    }
}

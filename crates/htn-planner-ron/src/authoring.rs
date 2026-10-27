//! Version-2 names are a cold authoring convenience, not executable runtime metadata.
use htn_planner::{Comparison, NumericOperation, Type, Value};
use serde::{Deserialize, Serialize};

mod capacity;
mod lower;
mod owned;

/// An ordered named parameter or local declaration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Binding {
    Ref(String, String),
    Record(String, String),
    Int(String),
    Symbol(String),
    Bool(String),
    Fixed(String, u8),
}
/// Names resolve within one task parameter/method-local or derived-query scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Expression {
    Var(String),
    Literal(Value),
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Step {
    Condition(Condition),
    Effect {
        name: String,
        arguments: Vec<Expression>,
    },
    Defer {
        name: String,
        arguments: Vec<Expression>,
    },
    Query {
        name: String,
        arguments: Vec<Expression>,
    },
    Call {
        name: String,
        arguments: Vec<Expression>,
        outputs: Vec<Expression>,
    },
    Require {
        name: String,
        arguments: Vec<Expression>,
    },
    Action {
        name: String,
        arguments: Vec<Expression>,
    },
    Task {
        name: String,
        arguments: Vec<Expression>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Method {
    #[serde(default)]
    pub locals: Vec<Binding>,
    #[serde(default)]
    pub result: Vec<Expression>,
    pub name: String,
    pub steps: Vec<Step>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    #[serde(default)]
    pub outputs: Vec<Type>,
    pub name: String,
    pub parameters: Vec<Binding>,
    pub methods: Vec<Method>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedQuery {
    pub name: String,
    pub parameters: Vec<Binding>,
    pub outputs: Vec<Type>,
    pub locals: Vec<Binding>,
    pub condition: Condition,
    pub result: Vec<Expression>,
}
/// Supplied named authoring data. Lowering does not admit execution or perform IO.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    #[serde(default)]
    pub queries: Vec<DerivedQuery>,
    pub version: u32,
    pub name: String,
    pub entries: Vec<String>,
    pub tasks: Vec<Task>,
}

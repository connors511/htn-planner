use serde::{Deserialize, Serialize};

/// Schema types preserve fixed-point scale and host identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Type {
    Bool,
    Int,
    Fixed(u8),
    Symbol,
    Ref(String),
    Record(String),
}
/// No floating-point or implicit conversions are supported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Fixed {
        raw: i64,
        scale: u8,
    },
    Symbol(String),
    Ref {
        type_name: String,
        key: u64,
    },
    Record {
        type_name: String,
        fields: Vec<Self>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signature {
    pub name: String,
    pub arguments: Vec<Type>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordType {
    pub name: String,
    pub fields: Vec<Type>,
}
/// Required bound input prefix for a lazily supplied predicate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryInputs {
    pub predicate: String,
    pub bound: usize,
}
/// Declaration vectors are ordered; duplicate names are rejected.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_inputs: Vec<QueryInputs>,
    #[serde(default)]
    pub effects: Vec<crate::PlanningEffect>,
    pub references: Vec<String>,
    pub records: Vec<RecordType>,
    pub predicates: Vec<Signature>,
    pub actions: Vec<Signature>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Expression {
    Literal(Value),
    Parameter(usize),
    Local(usize),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Step {
    Condition(crate::Condition),
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
    pub locals: Vec<Type>,
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
    pub parameters: Vec<Type>,
    pub methods: Vec<Method>,
}
/// Untrusted authoring IR. Only `CompiledDomain::admit` makes it executable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    #[serde(default)]
    pub queries: Vec<crate::DerivedQuery>,
    pub version: u32,
    pub name: String,
    pub entries: Vec<String>,
    pub tasks: Vec<Task>,
}
/// Limits are explicit, positive, and checked against implementation maxima.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub work: u64,
    pub storage: usize,
    pub depth: usize,
    pub plan: usize,
    pub bindings: usize,
    pub diagnostics: usize,
}
impl Limits {
    /// Conservative example profile; no production performance qualification is implied.
    pub const fn small() -> Self {
        Self {
            work: 10_000,
            storage: 16_384,
            depth: 32,
            plan: 64,
            bindings: 64,
            diagnostics: 16,
        }
    }
    /// Whether every allowance is positive and within the implementation maxima.
    /// This validates the envelope, not whether a particular domain fits it.
    pub const fn is_valid(self) -> bool {
        self.work > 0
            && self.work <= 1_000_000
            && self.storage > 0
            && self.storage <= 1_000_000
            && self.depth > 0
            && self.depth <= 128
            && self.plan > 0
            && self.plan <= 1024
            && self.bindings > 0
            && self.bindings <= 1024
            && self.diagnostics > 0
            && self.diagnostics <= 4096
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Locator {
    #[serde(default)]
    pub task: Option<String>,
    pub source: String,
    pub declaration: String,
    pub step: Option<usize>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub locator: Locator,
    pub message: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fact {
    pub predicate: String,
    pub arguments: Vec<Value>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub name: String,
    pub arguments: Vec<Value>,
    pub locator: Locator,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Success(Vec<crate::PlanEntry>),
    NoPlan,
    BudgetExhausted,
    CapacityExceeded,
    InvalidInput,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkReport {
    pub stages: crate::StageCounters,
    pub consumed: u64,
    pub peak_storage: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanningResult {
    pub diagnostic: Option<Error>,
    pub outcome: Outcome,
    pub work: WorkReport,
}

//! Optional diagnostics. IDs belong to one returned trace, never to host state.
use serde::{Deserialize, Serialize};
mod capture;
mod snapshot;
use crate::{DefinitionIdentity, Locator, Outcome, PlanningResult, Value, WorkReport};
pub(crate) use capture::{Capture, Site};
pub(crate) use snapshot::Snapshot;
/// Exact optional diagnostic site selection, independent of search semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceFocus<'a> {
    pub task: &'a str,
    pub declaration: Option<&'a str>,
}
pub type TraceId = u32;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceOutcome {
    Success,
    NoPlan,
    BudgetExhausted,
    CapacityExceeded,
    InvalidInput,
}
impl From<&Outcome> for TraceOutcome {
    fn from(outcome: &Outcome) -> Self {
        match outcome {
            Outcome::Success(_) => Self::Success,
            Outcome::NoPlan => Self::NoPlan,
            Outcome::BudgetExhausted => Self::BudgetExhausted,
            Outcome::CapacityExceeded => Self::CapacityExceeded,
            Outcome::InvalidInput => Self::InvalidInput,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceControl {
    And,
    Or,
    FirstSuccess,
    Not,
    Compare,
    Calculate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceRejection {
    NoMatchingTuple,
    UnificationConflict,
    ConditionFalse,
    ProbeFoundWitness,
    NoSuccessfulMethod,
    ArithmeticOverflow,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceStep {
    Condition,
    Query,
    Require,
    Action,
    Task,
    Call,
    Defer,
    Effect,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceEffectOperation {
    Assert,
    Retract,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceEvent {
    Attempt {
        deferred: bool,
    },
    TaskEntered,
    MethodEntered,
    StepEntered(TraceStep),
    StepCompleted,
    EffectEntered,
    EffectOperationCompleted {
        ordinal: usize,
        operation: TraceEffectOperation,
    },
    FactQueryEntered,
    DerivedQueryEntered,
    ControlEntered(TraceControl),
    AlternativeEntered {
        ordinal: usize,
    },
    FirstSuccessSelected,
    Witness,
    Rejected(TraceRejection),
    Restored,
    Finished(TraceOutcome),
    /// A validated row actually materialized by a demanded lazy predicate.
    QueryCandidate,
    /// Bounded integers from an actual host calculation; declaration is its static code.
    HostObservation,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceArguments {
    UnvalidatedOmitted,
    NotApplicable,
    Slots(Vec<Option<Value>>),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceNode {
    pub id: TraceId,
    pub parent: Option<TraceId>,
    pub event: TraceEvent,
    pub locator: Locator,
    pub arguments: TraceArguments,
    pub work: WorkReport,
}
/// Logical work inside actual host calls, excluding cache lookup and row validation.
/// This is bounded diagnostic accounting, not CPU timing or allocated bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceProviderWork {
    pub calls: u64,
    pub consumed: u64,
    /// Predicate index and consumed work of the first largest individual host call.
    pub maximum_call: Option<(usize, u64)>,
}
/// First failed work charge, retained even when the trace payload is clipped.
/// Resolve indices with bounds checks against this trace's compiled domain.
/// No argument payload is copied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceWorkFailure {
    pub consumed_before: u64,
    pub requested: u64,
    pub limit: u64,
    /// Last recorded task/method/search cursor; initial work can have no site,
    /// and cleanup can retain the previous cursor or the method-end boundary.
    pub site: Option<(usize, usize, Option<usize>)>,
    /// Present only while executing this lazy host predicate.
    pub provider_predicate: Option<usize>,
}
/// Fixed metadata is outside the independent variable payload cap. Capacity zero allocates no nodes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trace {
    pub domain: DefinitionIdentity,
    /// Exact task payload filter; None retains the original full trace behavior.
    /// Names over1024bytes omit all payload and use `<oversized-task-focus-omitted>`.
    /// Focus never changes planning, work, identity or fixed final metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_task: Option<String>,
    /// Optional exact declaration filter within the selected task.
    /// Oversized names use the same bounded omission marker and disable all payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_declaration: Option<String>,
    pub capacity: usize,
    pub retained_units: usize,
    pub nodes: Vec<TraceNode>,
    pub truncated: bool,
    /// Skipped hook emissions after truncation, saturating at `u64::MAX`.
    pub dropped_emissions: u64,
    pub dropped_saturated: bool,
    pub final_outcome: TraceOutcome,
    pub final_work: WorkReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_failure: Option<TraceWorkFailure>,
    #[serde(default)]
    pub provider_work: TraceProviderWork,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TracedPlanningResult {
    pub result: PlanningResult,
    pub trace: Trace,
}

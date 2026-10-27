//! Pure, deterministic bounded HTN planning. See the package README for the contract.
mod admission;
mod compiler;
mod conditions;
mod counters;
mod deferred;
mod heap;
mod identity;
mod rejected_input;
mod search;
mod trace;
mod types;
mod values;
pub use admission::{CompiledDomain, FactSnapshot};
pub use conditions::*;
pub use counters::StageCounters;
pub use deferred::{DeferredTask, PlanEntry};
pub use heap::HeapFootprint;
pub use identity::DefinitionIdentity;
pub use search::{
    AbsenceExplanation, ExplainedPlanningResult, PredicateFailure, QueryBudget, QueryError,
    QueryProvider, SearchExplanation, SearchResolution, expand_deferred,
    expand_deferred_explained_with_queries, expand_deferred_explained_with_queries_focused,
    expand_deferred_traced, expand_deferred_traced_with_queries,
    expand_deferred_traced_with_queries_focused, expand_deferred_with_queries, plan, plan_traced,
    plan_traced_with_queries, plan_traced_with_queries_focused, plan_with_queries,
};
pub use trace::*;
pub use types::*;
/// Independent from host persistence and content formats.
pub const COMPATIBILITY_ID: &str = "htn-planner/schema-2/semantics-2";

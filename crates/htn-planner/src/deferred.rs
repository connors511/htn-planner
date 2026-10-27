use crate::{Action, DefinitionIdentity, Locator, Value};
use serde::{Deserialize, Serialize};

/// A fully bound future invocation. Deserialized tokens remain untrusted until expansion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeferredTask {
    pub(crate) definition: DefinitionIdentity,
    pub(crate) semantics: String,
    pub(crate) task: String,
    pub(crate) arguments: Vec<Value>,
    pub(crate) locator: Locator,
}
impl DeferredTask {
    pub const fn definition_identity(&self) -> DefinitionIdentity {
        self.definition
    }
    pub fn semantics(&self) -> &str {
        &self.semantics
    }
    pub fn task(&self) -> &str {
        &self.task
    }
    pub fn arguments(&self) -> &[Value] {
        &self.arguments
    }
    pub const fn locator(&self) -> &Locator {
        &self.locator
    }
}
/// Execution belongs to the host; both variants are inert plan data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanEntry {
    Action(Action),
    Deferred(DeferredTask),
}
impl PlanEntry {
    pub const fn as_action(&self) -> Option<&Action> {
        match self {
            Self::Action(action) => Some(action),
            Self::Deferred(_) => None,
        }
    }
}

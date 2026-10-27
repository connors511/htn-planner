//! A deliberately small warehouse host, not a reusable execution framework.
use htn_planner::{
    Action, COMPATIBILITY_ID, CompiledDomain, DefinitionIdentity, Fact, FactSnapshot, Limits,
    Outcome, PlanEntry, Schema, Signature, Type, Value, expand_deferred, plan,
};
use serde::{Deserialize, Serialize};

pub fn schema() -> Schema {
    Schema {
        predicates: vec![Signature {
            name: "ready".into(),
            arguments: vec![Type::Int, Type::Int],
        }],
        actions: vec![
            Signature {
                name: "prepare".into(),
                arguments: vec![Type::Int],
            },
            Signature {
                name: "work".into(),
                arguments: vec![Type::Int, Type::Int],
            },
        ],
        ..Schema::default()
    }
}
pub fn admit(source: &str, limits: Limits) -> Result<CompiledDomain, htn_planner::Error> {
    htn_planner_ron::compile("warehouse-lifecycle", source, schema(), limits)
}
pub fn domain() -> Result<CompiledDomain, htn_planner::Error> {
    admit(include_str!("lifecycle.ron"), Limits::small())
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observation {
    pub available: Vec<i64>,
    pub positions: Vec<(i64, i64)>,
    pub can_start: bool,
    pub fail_execution: bool,
}
impl Observation {
    pub fn ready(target: i64, position: i64) -> Self {
        Self {
            available: vec![target],
            positions: vec![(target, position)],
            can_start: true,
            fail_execution: false,
        }
    }
    fn facts(&self) -> Result<FactSnapshot, String> {
        let rows = self
            .positions
            .iter()
            .filter(|(target, _)| self.available.contains(target))
            .map(|(target, position)| Fact {
                predicate: "ready".into(),
                arguments: vec![Value::Int(*target), Value::Int(*position)],
            })
            .collect();
        FactSnapshot::new(&schema(), rows)
    }
    fn validate(&self, action: &Action) -> Result<i64, Rejection> {
        let target = match (action.name.as_str(), action.arguments.as_slice()) {
            ("prepare", [Value::Int(target)]) | ("work", [Value::Int(target), Value::Int(_)]) => {
                *target
            }
            _ => return Err(Rejection::Malformed),
        };
        if !self.available.contains(&target) {
            return Err(Rejection::Unavailable);
        }
        if !self.can_start {
            return Err(Rejection::Preconditions);
        }
        if let ("work", [Value::Int(_), Value::Int(position)]) =
            (action.name.as_str(), action.arguments.as_slice())
            && !self.positions.contains(&(target, *position))
        {
            return Err(Rejection::StalePosition);
        }
        Ok(target)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rejection {
    Unavailable,
    Preconditions,
    StalePosition,
    Malformed,
    ExecutionFailed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Accepted,
    Retained,
    Busy,
    Started(i64),
    Progress(i64, u8),
    Completed(i64),
    RejectedStart(Rejection),
    FailedExecution(Rejection),
    Expanded,
    CompletedEmpty,
    Finished,
    Suppressed,
    Waiting,
    NoPlan,
    BudgetExhausted,
    CapacityExceeded,
    InvalidInput,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Active {
    action: Action,
    progress: u8,
    last_progress_tick: u64,
}
/// Host-owned portable checkpoint: no native pointers or inferred hidden progress.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    definition: DefinitionIdentity,
    semantics: String,
    entries: Vec<PlanEntry>,
    accepted: Option<Vec<PlanEntry>>,
    cursor: usize,
    active: Option<Active>,
    last_attempt_tick: Option<u64>,
}
pub struct Host {
    state: Checkpoint,
}
impl Host {
    pub fn new(domain: &CompiledDomain) -> Self {
        Self {
            state: Checkpoint {
                definition: domain.identity(),
                semantics: COMPATIBILITY_ID.into(),
                entries: vec![],
                accepted: None,
                cursor: 0,
                active: None,
                last_attempt_tick: None,
            },
        }
    }
    pub fn checkpoint(&self) -> Checkpoint {
        self.state.clone()
    }
    pub fn resume(domain: &CompiledDomain, state: Checkpoint) -> Result<Self, &'static str> {
        if state.definition != domain.identity() || state.semantics != COMPATIBILITY_ID {
            return Err("incompatible checkpoint identity");
        }
        if state.entries.len() > domain.limits().plan
            || state
                .accepted
                .as_ref()
                .is_some_and(|entries| entries.len() > domain.limits().plan)
            || state.cursor > state.entries.len()
        {
            return Err("invalid checkpoint cursor or capacity");
        }
        for entry in state.entries.iter().chain(state.accepted.iter().flatten()) {
            if let PlanEntry::Deferred(token) = entry
                && (token.definition_identity() != domain.identity()
                    || token.semantics() != COMPATIBILITY_ID)
            {
                return Err("incompatible deferred identity");
            }
        }
        if let Some(active) = &state.active
            && (active.progress >= 2
                || state
                    .entries
                    .get(state.cursor)
                    .and_then(PlanEntry::as_action)
                    != Some(&active.action))
        {
            return Err("invalid checkpoint active progress");
        }
        Ok(Self { state })
    }
    fn attempt(&mut self, tick: u64) -> bool {
        if self
            .state
            .last_attempt_tick
            .is_some_and(|last| tick <= last)
        {
            return false;
        }
        self.state.last_attempt_tick = Some(tick);
        true
    }
    fn compatible(&self, domain: &CompiledDomain) -> bool {
        self.state.definition == domain.identity() && self.state.semantics == COMPATIBILITY_ID
    }
    pub fn decide(
        &mut self,
        domain: &CompiledDomain,
        observation: &Observation,
        tick: u64,
        inputs: &[Value],
    ) -> Event {
        if !self.compatible(domain) {
            return Event::InvalidInput;
        }
        if !self.attempt(tick) {
            return Event::Suppressed;
        }
        let Ok(facts) = observation.facts() else {
            return Event::InvalidInput;
        };
        let outcome = plan(domain, &facts, "entry", inputs).outcome;
        let Outcome::Success(entries) = outcome else {
            return status(&outcome);
        };
        if self
            .state
            .accepted
            .as_ref()
            .is_some_and(|accepted| equivalent(accepted, &entries))
        {
            return Event::Retained;
        }
        if self.state.active.is_some() {
            return if equivalent(&self.state.entries[self.state.cursor..], &entries) {
                self.state.accepted = Some(entries);
                Event::Retained
            } else {
                Event::Busy
            };
        }
        self.state.accepted = Some(entries.clone());
        self.state.entries = entries;
        self.state.cursor = 0;
        if self.state.entries.is_empty() {
            Event::CompletedEmpty
        } else {
            Event::Accepted
        }
    }
    pub fn advance(
        &mut self,
        domain: &CompiledDomain,
        observation: &Observation,
        tick: u64,
    ) -> Event {
        if !self.compatible(domain) {
            return Event::InvalidInput;
        }
        if let Some(active) = &mut self.state.active {
            let validity = if observation.fail_execution {
                Err(Rejection::ExecutionFailed)
            } else {
                observation.validate(&active.action)
            };
            let target = match validity {
                Ok(target) => target,
                Err(reason) => {
                    self.clear();
                    return Event::FailedExecution(reason);
                }
            };
            if tick <= active.last_progress_tick {
                return Event::Waiting;
            }
            active.last_progress_tick = tick;
            active.progress += 1;
            if active.progress < 2 {
                return Event::Progress(target, active.progress);
            }
            self.state.active = None;
            self.state.cursor += 1;
            return Event::Completed(target);
        }
        let Some(entry) = self.state.entries.get(self.state.cursor) else {
            return Event::Finished;
        };
        if let PlanEntry::Action(action) = entry {
            let target = match observation.validate(action) {
                Ok(target) => target,
                Err(reason) => {
                    self.clear();
                    return Event::RejectedStart(reason);
                }
            };
            self.state.active = Some(Active {
                action: action.clone(),
                progress: 0,
                last_progress_tick: tick,
            });
            return Event::Started(target);
        }
        // Deferred work only expands at this idle host boundary.
        if !self.attempt(tick) {
            return Event::Suppressed;
        }
        let Ok(facts) = observation.facts() else {
            return Event::InvalidInput;
        };
        let Some(PlanEntry::Deferred(token)) = self.state.entries.get(self.state.cursor) else {
            return Event::InvalidInput;
        };
        let outcome = expand_deferred(domain, &facts, token).outcome;
        let Outcome::Success(mut entries) = outcome else {
            return status(&outcome);
        };
        let tail = &self.state.entries[self.state.cursor + 1..];
        if entries.len().saturating_add(tail.len()) > domain.limits().plan {
            return Event::CapacityExceeded;
        }
        entries.extend_from_slice(tail);
        self.state.entries = entries;
        self.state.cursor = 0;
        if self.state.entries.is_empty() {
            Event::CompletedEmpty
        } else {
            Event::Expanded
        }
    }
    fn clear(&mut self) {
        self.state.accepted = None;
        self.state.entries.clear();
        self.state.cursor = 0;
        self.state.active = None;
    }
    /// Clear encounter work while retaining the scheduling guard for this logical tick.
    pub fn reset(&mut self) {
        self.clear();
    }
}
const fn status(outcome: &Outcome) -> Event {
    match outcome {
        Outcome::Success(_) => Event::Accepted,
        Outcome::NoPlan => Event::NoPlan,
        Outcome::BudgetExhausted => Event::BudgetExhausted,
        Outcome::CapacityExceeded => Event::CapacityExceeded,
        Outcome::InvalidInput => Event::InvalidInput,
    }
}

fn equivalent(left: &[PlanEntry], right: &[PlanEntry]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| match (left, right) {
                (PlanEntry::Action(left), PlanEntry::Action(right)) => {
                    left.name == right.name && left.arguments == right.arguments
                }
                (PlanEntry::Deferred(left), PlanEntry::Deferred(right)) => {
                    left.definition_identity() == right.definition_identity()
                        && left.semantics() == right.semantics()
                        && left.task() == right.task()
                        && left.arguments() == right.arguments()
                }
                _ => false,
            })
}

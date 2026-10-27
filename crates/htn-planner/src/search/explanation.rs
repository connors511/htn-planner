//! Independently bounded evaluation evidence, separate from search and trace allowances.
mod text;
use super::state::{Branch, Item, Meter};
use crate::{
    CompiledDomain, Locator, Outcome, PlanningResult, Trace, TraceOutcome, TraceRejection,
};
use std::collections::VecDeque;
pub(super) use text::{arguments, values};

const RETAINED: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchResolution {
    Planned,
    AuthoredEmpty,
    EmptyDecomposition,
    Exhausted,
    Interrupted(TraceOutcome),
}

/// Host observations attached only to an actually rejected empty query batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbsenceExplanation {
    pub code: String,
    pub values: Vec<(String, i64)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PredicateFailure {
    /// Declaration identifies the authored method, independently of the predicate.
    pub locator: Locator,
    pub predicate: String,
    /// Bounded rendering of values resolved at rejection, never reevaluated later.
    pub arguments: String,
    pub rejection: TraceRejection,
    pub absence: Option<AbsenceExplanation>,
}

/// A bounded sample of exploration; it never grants permission to execute.
/// An empty selected branch is distinct from exhaustion of all admitted choices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchExplanation {
    pub resolution: SearchResolution,
    pub selected_empty: Option<Locator>,
    pub failures: Vec<PredicateFailure>,
    pub explored_rejections: u64,
    pub recovered_rejections: u64,
    pub omitted_rejections: u64,
    pub exhausted_queries: u64,
    pub first_success_cuts: u64,
    pub truncated: bool,
}
impl Default for SearchExplanation {
    fn default() -> Self {
        Self {
            resolution: SearchResolution::Interrupted(TraceOutcome::InvalidInput),
            selected_empty: None,
            failures: Vec::new(),
            explored_rejections: 0,
            recovered_rejections: 0,
            omitted_rejections: 0,
            exhausted_queries: 0,
            first_success_cuts: 0,
            truncated: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplainedPlanningResult {
    pub result: PlanningResult,
    pub trace: Option<Trace>,
    pub explanation: SearchExplanation,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct HostAbsence {
    pub code: &'static str,
    pub values: [Option<(&'static str, i64)>; 3],
    pub omitted: bool,
}

#[derive(Clone, Copy)]
pub(super) struct EmptySite {
    pub invocation: u64,
    pub task: usize,
    pub method: usize,
}
#[derive(Clone, Copy)]
pub(super) struct Site {
    pub invocation: u64,
    pub task: usize,
    pub method: usize,
    pub step: Option<usize>,
}
impl Site {
    pub fn of(item: &Item) -> Self {
        Self {
            invocation: item.explanation_invocation,
            task: item.task,
            method: item.method,
            step: item.step.and_then(|step| step.checked_sub(1)),
        }
    }
}
#[derive(Clone, Copy)]
pub(super) enum Predicate {
    Fact(usize),
    Derived(usize),
    Control(&'static str),
}
struct Rejected {
    site: Site,
    predicate: Predicate,
    arguments: text::Rendered,
    rejection: TraceRejection,
    absence: Option<HostAbsence>,
}

#[derive(Default)]
pub(super) struct Collector {
    next_invocation: u64,
    rejected: VecDeque<Rejected>,
    notes: VecDeque<(usize, HostAbsence)>,
    explored: u64,
    pub selected: Option<EmptySite>,
    pub exhausted_queries: u64,
    pub first_success_cuts: u64,
    clipped: bool,
}
impl Collector {
    pub const fn invocation(&mut self) -> u64 {
        self.next_invocation = self.next_invocation.saturating_add(1);
        self.next_invocation
    }
    pub fn note(&mut self, batch: usize, absence: HostAbsence) {
        if self.notes.len() == RETAINED {
            self.notes.pop_front();
            self.clipped = true;
        }
        self.notes.push_back((batch, absence));
    }
    pub fn reject(
        &mut self,
        site: Site,
        predicate: Predicate,
        arguments: text::Rendered,
        rejection: TraceRejection,
        batch: Option<usize>,
    ) {
        self.explored = self.explored.saturating_add(1);
        if self.rejected.len() == RETAINED {
            self.rejected.pop_front();
            self.clipped = true;
        }
        let absence = batch
            .filter(|_| rejection == TraceRejection::NoMatchingTuple)
            .and_then(|batch| {
                self.notes
                    .iter()
                    .find(|(id, _)| *id == batch)
                    .map(|(_, note)| *note)
            });
        self.rejected.push_back(Rejected {
            site,
            predicate,
            arguments,
            rejection,
            absence,
        });
    }
    pub fn finish(self, domain: &CompiledDomain, outcome: &Outcome) -> SearchExplanation {
        let planned = matches!(outcome, Outcome::Success(entries) if !entries.is_empty());
        let empty = matches!(outcome, Outcome::Success(entries) if entries.is_empty());
        let selected = self.selected.filter(|_| empty);
        let mut result = SearchExplanation {
            resolution: match outcome {
                Outcome::Success(_) if planned => SearchResolution::Planned,
                Outcome::Success(_) if selected.is_some() => SearchResolution::AuthoredEmpty,
                Outcome::Success(_) => SearchResolution::EmptyDecomposition,
                Outcome::NoPlan => SearchResolution::Exhausted,
                _ => SearchResolution::Interrupted(TraceOutcome::from(outcome)),
            },
            selected_empty: selected
                .map(|site| bounded_locator(domain, site.task, site.method, None)),
            explored_rejections: self.explored,
            recovered_rejections: if planned { self.explored } else { 0 },
            exhausted_queries: self.exhausted_queries,
            first_success_cuts: self.first_success_cuts,
            truncated: self.clipped,
            ..SearchExplanation::default()
        };
        if let Some(site) = selected {
            result.truncated |= locator_clipped(domain, site.task, site.method);
        }
        for rejected in self.rejected {
            if planned {
                continue;
            }
            // A selected method may backtrack over witnesses or condition arms
            // before succeeding empty. Those failures recovered inside that
            // successful method; only its rejected sibling methods remain samples.
            if selected.is_some_and(|selected| {
                selected.invocation != rejected.site.invocation
                    || selected.method == rejected.site.method
            }) {
                result.recovered_rejections = result.recovered_rejections.saturating_add(1);
                continue;
            }
            let predicate = match rejected.predicate {
                Predicate::Fact(id) => domain.schema.predicates[id].name.as_str(),
                Predicate::Derived(id) => domain.definition.queries[id].name.as_str(),
                Predicate::Control(name) => name,
            };
            result.truncated |= predicate.len() > 48 || rejected.arguments.truncated;
            let absence = rejected.absence.map(|note| {
                result.truncated |= note.omitted || note.code.len() > 48;
                AbsenceExplanation {
                    code: text::bounded(note.code, 48),
                    values: note
                        .values
                        .into_iter()
                        .flatten()
                        .map(|(name, value)| {
                            result.truncated |= name.len() > 16;
                            (text::bounded(name, 16), value)
                        })
                        .collect(),
                }
            });
            let site = rejected.site;
            result.truncated |= locator_clipped(domain, site.task, site.method);
            result.failures.push(PredicateFailure {
                locator: bounded_locator(domain, site.task, site.method, site.step),
                predicate: text::bounded(predicate, 48),
                arguments: rejected.arguments.text,
                rejection: rejected.rejection,
                absence,
            });
        }
        result.omitted_rejections = self.explored.saturating_sub(
            result
                .recovered_rejections
                .saturating_add(u64::try_from(result.failures.len()).unwrap_or(u64::MAX)),
        );
        result.truncated |= result.omitted_rejections != 0;
        result
    }
}
fn locator_clipped(domain: &CompiledDomain, task: usize, method: usize) -> bool {
    domain.source.len() > 32
        || domain.definition.tasks[task].name.len() > 32
        || domain.tasks[task]
            .methods
            .get(method)
            .is_some_and(|method| method.name.len() > 32)
}
fn bounded_locator(
    domain: &CompiledDomain,
    task: usize,
    method: usize,
    step: Option<usize>,
) -> Locator {
    Locator {
        source: text::bounded(&domain.source, 32),
        task: Some(text::bounded(&domain.definition.tasks[task].name, 32)),
        declaration: text::bounded(
            domain.tasks[task]
                .methods
                .get(method)
                .map_or(domain.definition.tasks[task].name.as_str(), |method| {
                    method.name.as_str()
                }),
            32,
        ),
        step,
    }
}

pub(super) fn returned(domain: &CompiledDomain, item: &Item, active: &mut Branch, meter: &Meter) {
    if meter.explanation.is_some()
        && domain.tasks[item.task].methods[item.method]
            .steps
            .iter()
            .all(|step| {
                matches!(
                    step,
                    crate::compiler::Instruction::Condition { .. }
                        | crate::compiler::Instruction::Require { .. }
                )
            })
    {
        active.selected_empty = Some(EmptySite {
            invocation: item.explanation_invocation,
            task: item.task,
            method: item.method,
        });
    }
}

pub(super) fn reject_expressions(
    meter: &mut Meter,
    item: &Item,
    predicate: Predicate,
    args: &[crate::compiler::CompiledExpression],
    rejection: TraceRejection,
    batch: Option<usize>,
) {
    if item
        .control
        .as_ref()
        .is_some_and(super::condition::Control::negated_probe)
    {
        return;
    }
    if let Some(collector) = &mut meter.explanation {
        collector.reject(
            Site::of(item),
            predicate,
            arguments(args, item.environment()),
            rejection,
            batch,
        );
    }
}
pub(super) fn reject_values(
    meter: &mut Meter,
    item: &Item,
    predicate: Predicate,
    args: &[crate::Value],
    rejection: TraceRejection,
    batch: Option<usize>,
) {
    if item
        .control
        .as_ref()
        .is_some_and(super::condition::Control::negated_probe)
    {
        return;
    }
    if let Some(collector) = &mut meter.explanation {
        collector.reject(Site::of(item), predicate, values(args), rejection, batch);
    }
}

pub(super) fn reject_numeric(
    meter: &mut Meter,
    item: &Item,
    name: &'static str,
    left: &crate::compiler::CompiledExpression,
    right: &crate::compiler::CompiledExpression,
    rejection: TraceRejection,
) {
    if rejection != TraceRejection::ArithmeticOverflow
        && item
            .control
            .as_ref()
            .is_some_and(super::condition::Control::negated_probe)
    {
        return;
    }
    if let Some(collector) = &mut meter.explanation {
        collector.reject(
            Site::of(item),
            Predicate::Control(name),
            text::numeric(left, right, item.environment()),
            rejection,
            None,
        );
    }
}

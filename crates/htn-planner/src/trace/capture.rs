use super::{
    Snapshot, Trace, TraceArguments, TraceEvent, TraceFocus, TraceId, TraceNode, TraceOutcome,
};
use crate::{CompiledDomain, Locator, WorkReport};
#[derive(Clone, Copy)]
pub(crate) struct Site<'a> {
    pub source: &'a str,
    pub declaration: &'a str,
    pub task: Option<&'a str>,
    pub step: Option<usize>,
}
pub(crate) struct Capture {
    pub trace: Trace,
    enabled: bool,
    focus_valid: bool,
    pub root: Option<TraceId>,
}
impl Capture {
    pub fn new(domain: &CompiledDomain, requested: Option<usize>) -> Self {
        Self::new_focused(domain, requested, None)
    }
    pub fn new_focused(
        domain: &CompiledDomain,
        requested: Option<usize>,
        focus: Option<TraceFocus<'_>>,
    ) -> Self {
        let focus_valid = focus.is_none_or(|focus| {
            focus.task.len() <= 1024 && focus.declaration.is_none_or(|name| name.len() <= 1024)
        });
        let bounded = |name: &str| {
            if name.len() <= 1024 {
                name
            } else {
                "<oversized-task-focus-omitted>"
            }
            .to_owned()
        };
        let focus_task = focus.map(|focus| bounded(focus.task));
        let focus_declaration = focus.and_then(|focus| focus.declaration.map(bounded));
        Self {
            trace: Trace {
                domain: domain.identity(),
                focus_task,
                focus_declaration,
                capacity: requested.unwrap_or(0).min(domain.limits().diagnostics),
                retained_units: 0,
                nodes: vec![],
                truncated: false,
                dropped_emissions: 0,
                dropped_saturated: false,
                final_outcome: TraceOutcome::InvalidInput,
                final_work: WorkReport::default(),
                work_failure: None,
                provider_work: super::TraceProviderWork::default(),
            },
            enabled: requested.is_some(),
            focus_valid,
            root: None,
        }
    }
    fn matches(&self, site: Site<'_>) -> bool {
        self.focus_valid
            && self
                .trace
                .focus_task
                .as_deref()
                .is_none_or(|task| site.task == Some(task))
            && self
                .trace
                .focus_declaration
                .as_deref()
                .is_none_or(|declaration| site.declaration == declaration)
    }
    pub fn observations_enabled(&self, site: Site<'_>) -> bool {
        self.enabled
            && self.focus_valid
            && !self.trace.truncated
            && self
                .trace
                .focus_task
                .as_deref()
                .is_none_or(|task| site.task == Some(task))
    }
    pub fn omit_observation(&mut self, site: Site<'_>) {
        if self.enabled && self.matches(site) {
            self.trace.truncated = true;
            self.dropped();
        }
    }
    pub fn provider_work(&mut self, predicate: usize, before: u64, after: u64) {
        if !self.enabled {
            return;
        }
        let work = after.saturating_sub(before);
        let report = &mut self.trace.provider_work;
        report.calls = report.calls.saturating_add(1);
        report.consumed = report.consumed.saturating_add(work);
        if report
            .maximum_call
            .is_none_or(|(_, previous)| work > previous)
        {
            report.maximum_call = Some((predicate, work));
        }
    }
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
    const fn ready(&mut self) -> bool {
        if !self.enabled {
            return false;
        }
        if self.trace.truncated {
            self.dropped();
            return false;
        }
        true
    }
    const fn dropped(&mut self) {
        let (next, overflow) = self.trace.dropped_emissions.overflowing_add(1);
        self.trace.dropped_emissions = if overflow { u64::MAX } else { next };
        self.trace.dropped_saturated |= overflow;
    }
    const fn reserve(&mut self, cost: usize) -> bool {
        if cost
            > self
                .trace
                .capacity
                .saturating_sub(self.trace.retained_units)
        {
            self.trace.truncated = true;
            self.dropped();
            false
        } else {
            self.trace.retained_units += cost;
            true
        }
    }
    pub fn emit(
        &mut self,
        parent: Option<TraceId>,
        event: TraceEvent,
        locator: Site<'_>,
        arguments: Option<Snapshot<'_>>,
        work: &WorkReport,
    ) -> Option<TraceId> {
        if !self.matches(locator) {
            return None;
        }
        if !self.ready() {
            return None;
        }
        let mut cost = 1usize
            .saturating_add(locator.source.len())
            .saturating_add(locator.declaration.len())
            .saturating_add(locator.task.map_or(0, |name| 1 + name.len()));
        if let Some(arguments) = arguments {
            cost = cost.saturating_add(
                arguments.cost(
                    self.trace
                        .capacity
                        .saturating_sub(self.trace.retained_units)
                        .saturating_sub(cost),
                ),
            );
        }
        if !self.reserve(cost) {
            return None;
        }
        let id = u32::try_from(self.trace.nodes.len()).ok()?;
        self.trace.nodes.push(TraceNode {
            id,
            parent,
            event,
            locator: Locator {
                source: locator.source.into(),
                declaration: locator.declaration.into(),
                task: locator.task.map(str::to_owned),
                step: locator.step,
            },
            arguments: arguments.map_or(TraceArguments::NotApplicable, |values| {
                TraceArguments::Slots(values.copy())
            }),
            work: work.clone(),
        });
        Some(id)
    }
    pub fn inherit(
        &mut self,
        parent: Option<TraceId>,
        event: TraceEvent,
        arguments: Option<Snapshot<'_>>,
        work: &WorkReport,
    ) -> Option<TraceId> {
        if !self.focus_valid
            || self.trace.focus_task.is_some()
                && parent
                    .and_then(|id| {
                        usize::try_from(id)
                            .ok()
                            .and_then(|index| self.trace.nodes.get(index))
                    })
                    .is_none_or(|node| {
                        !self.matches(Site {
                            source: &node.locator.source,
                            declaration: &node.locator.declaration,
                            task: node.locator.task.as_deref(),
                            step: node.locator.step,
                        })
                    })
        {
            return None;
        }
        if !self.ready() {
            return None;
        }
        let parent = parent?;
        let node = self.trace.nodes.get(usize::try_from(parent).ok()?)?;
        let mut cost = 1
            + node.locator.source.len()
            + node.locator.declaration.len()
            + node.locator.task.as_ref().map_or(0, |name| 1 + name.len());
        if let Some(arguments) = arguments {
            cost = cost.saturating_add(
                arguments.cost(
                    self.trace
                        .capacity
                        .saturating_sub(self.trace.retained_units)
                        .saturating_sub(cost),
                ),
            );
        }
        if !self.reserve(cost) {
            return None;
        }
        let locator = self
            .trace
            .nodes
            .get(usize::try_from(parent).ok()?)?
            .locator
            .clone();
        let id = u32::try_from(self.trace.nodes.len()).ok()?;
        self.trace.nodes.push(TraceNode {
            id,
            parent: Some(parent),
            event,
            locator,
            arguments: arguments.map_or(TraceArguments::NotApplicable, |values| {
                TraceArguments::Slots(values.copy())
            }),
            work: work.clone(),
        });
        Some(id)
    }
    pub fn attempt(&mut self, domain: &CompiledDomain, deferred: bool, work: &WorkReport) {
        if self.trace.focus_task.is_some() {
            return;
        }
        if !self.ready() {
            return;
        }
        let locator = Locator {
            source: String::new(),
            task: None,
            declaration: String::new(),
            step: None,
        };
        // Borrow admitted strings until independent preflight has succeeded.
        let cost = 1 + domain.source.len() + domain.name().len();
        if !self.reserve(cost) {
            return;
        }
        let id = 0;
        self.trace.nodes.push(TraceNode {
            id,
            parent: None,
            event: TraceEvent::Attempt { deferred },
            locator: Locator {
                source: domain.source.clone(),
                declaration: domain.name().into(),
                ..locator
            },
            arguments: TraceArguments::UnvalidatedOmitted,
            work: work.clone(),
        });
        self.root = Some(id);
    }
}

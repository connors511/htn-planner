use crate::{Limits, Outcome, PlanEntry, Value, WorkReport};

#[derive(Clone)]
pub(super) struct Bindings {
    pub values: Vec<Option<Value>>,
    pub costs: Vec<usize>,
    pub size: usize,
}
impl Bindings {
    pub fn locals(&mut self, count: usize) {
        self.values.resize_with(self.values.len() + count, || None);
        self.costs.resize(self.costs.len() + count, 0);
        self.size += 2 * count;
    }
}
#[derive(Clone)]
pub(super) struct Item {
    pub explanation_invocation: u64,
    pub trace_task: Option<crate::TraceId>,
    pub trace_method: Option<crate::TraceId>,
    pub trace_caller: Option<crate::TraceId>,
    pub invocation_entered: bool,
    pub control: Option<super::condition::Control>,
    pub task: usize,
    pub method: usize,
    pub step: Option<usize>,
    pub args: Bindings,
    pub depth: usize,
}
impl Item {
    pub fn cost(&self) -> usize {
        1 + self.args.size + self.control.as_ref().map_or(0, |control| control.size)
    }
    pub fn slots(&self) -> usize {
        self.args.values.len() + self.control.as_ref().map_or(0, |control| control.bindings)
    }
    pub fn environment(&self) -> &Bindings {
        self.control
            .as_ref()
            .and_then(|control| control.scopes.last())
            .unwrap_or(&self.args)
    }
    pub fn environment_mut(&mut self) -> Result<&mut Bindings, Outcome> {
        match &mut self.control {
            Some(control) if !control.scopes.is_empty() => {
                control.scopes.last_mut().ok_or(Outcome::InvalidInput)
            }
            _ => Ok(&mut self.args),
        }
    }
    pub const fn growth(&mut self, size: usize) {
        if let Some(control) = &mut self.control
            && !control.scopes.is_empty()
        {
            control.size += size;
        }
    }
}
#[derive(Clone)]
pub(super) struct Branch {
    pub selected_empty: Option<super::explanation::EmptySite>,
    pub overlay: super::overlay::Overlay,
    pub pending: Vec<Item>,
    pub actions: Vec<PlanEntry>,
    pub cost: usize,
    pub bindings: usize,
}
impl Branch {
    pub fn push(&mut self, item: Item) {
        self.cost += item.cost();
        self.bindings += item.slots();
        self.pending.push(item);
    }
    pub fn pop(&mut self) -> Option<Item> {
        let item = self.pending.pop()?;
        self.cost -= item.cost();
        self.bindings -= item.slots();
        Some(item)
    }
}
#[derive(Default)]
pub(super) struct Choices {
    branches: Vec<(Branch, Option<u64>, Option<crate::TraceId>)>,
    pub cost: usize,
    pub bindings: usize,
}
impl Choices {
    pub fn retain(&mut self, active: &Branch, meter: &mut Meter) -> Result<(), Outcome> {
        meter.reserve(
            self.cost.saturating_add(active.cost.saturating_mul(2)),
            self.bindings
                .saturating_add(active.bindings.saturating_mul(2)),
        )?;
        meter.charge(active.cost)?;
        self.cost += active.cost;
        self.bindings += active.bindings;
        self.branches.push((active.clone(), None, None));
        Ok(())
    }
    pub const fn count(&self) -> usize {
        self.branches.len()
    }
    pub fn mark_absence(&mut self, parent: Option<crate::TraceId>) {
        if let Some(choice) = self.branches.last_mut() {
            choice.2 = parent;
        }
    }
    pub fn retain_tagged(
        &mut self,
        active: &Branch,
        tag: Option<u64>,
        meter: &mut Meter,
    ) -> Result<(), Outcome> {
        self.retain(active, meter)?;
        if let Some(choice) = self.branches.last_mut() {
            choice.1 = tag;
        }
        Ok(())
    }
    pub fn commit(&mut self, id: u64, meter: &mut Meter) -> Result<(), Outcome> {
        let mut cost = 0;
        let mut bindings = 0;
        for (branch, tag, _) in &self.branches {
            meter.charge(1)?;
            if *tag == Some(id) {
                cost += branch.cost;
                bindings += branch.bindings;
            }
        }
        meter.charge(self.branches.len().saturating_add(cost))?;
        self.branches.retain(|(_, tag, _)| *tag != Some(id));
        self.cost -= cost;
        self.bindings -= bindings;
        Ok(())
    }
    pub fn truncate(&mut self, base: usize, meter: &mut Meter) -> Result<(), Outcome> {
        let removed = self.branches.get(base..).ok_or(Outcome::InvalidInput)?;
        let mut cost = 0;
        let mut bindings = 0;
        for (branch, _, _) in removed {
            meter.charge(1)?;
            cost += branch.cost;
            bindings += branch.bindings;
        }
        meter.charge(cost)?;
        self.branches.truncate(base);
        self.cost -= cost;
        self.bindings -= bindings;
        Ok(())
    }
    pub fn restore(&mut self, active: &mut Branch, meter: &mut Meter) -> Result<bool, Outcome> {
        meter.charge(1usize.saturating_add(active.cost))?;
        let Some((branch, _, absence)) = self.branches.pop() else {
            return Ok(false);
        };
        self.cost -= branch.cost;
        self.bindings -= branch.bindings;
        *active = branch;
        meter.report.stages.record(crate::counters::Stage::Restore);
        if let Some(parent) = absence {
            meter.trace.inherit(
                Some(parent),
                crate::TraceEvent::Witness,
                None,
                &meter.report,
            );
        } else if let Some(item) = active.pending.last() {
            let parent = item
                .control
                .as_ref()
                .and_then(|control| control.frames.last().and_then(|frame| frame.parent))
                .or(item.trace_method)
                .or(item.trace_task);
            meter
                .trace
                .inherit(parent, crate::TraceEvent::Restored, None, &meter.report);
        }
        Ok(true)
    }
    pub fn reserve(
        &self,
        active: &Branch,
        extra: usize,
        bindings: usize,
        meter: &mut Meter,
    ) -> Result<(), Outcome> {
        meter.reserve(
            self.cost.saturating_add(active.cost).saturating_add(extra),
            self.bindings
                .saturating_add(active.bindings)
                .saturating_add(bindings),
        )
    }
}
pub(super) struct Meter {
    pub explanation: Option<super::explanation::Collector>,
    pub trace: crate::trace::Capture,
    pub limits: Limits,
    pub report: WorkReport,
    pub site: Option<(usize, usize, Option<usize>)>,
    pub provider_predicate: Option<usize>,
    pub reason: Option<&'static str>,
    pub next_boundary: u64,
    pub query_storage: usize,
}
impl Meter {
    pub fn charge(&mut self, units: usize) -> Result<(), Outcome> {
        let units = u64::try_from(units).map_err(|_| Outcome::CapacityExceeded)?;
        if units > self.limits.work - self.report.consumed {
            if self.trace.enabled() && self.trace.trace.work_failure.is_none() {
                self.trace.trace.work_failure = Some(crate::TraceWorkFailure {
                    consumed_before: self.report.consumed,
                    requested: units,
                    limit: self.limits.work,
                    site: self.site,
                    provider_predicate: self.provider_predicate,
                });
            }
            self.report.consumed = self.limits.work;
            return Err(Outcome::BudgetExhausted);
        }
        self.report.consumed += units;
        Ok(())
    }
    pub fn reserve(&mut self, total: usize, bindings: usize) -> Result<(), Outcome> {
        let total = total.saturating_add(self.query_storage);
        if total > self.limits.storage || bindings > self.limits.bindings {
            return Err(Outcome::CapacityExceeded);
        }
        self.report.peak_storage = self.report.peak_storage.max(total);
        Ok(())
    }
}
#[derive(Clone)]
pub(super) struct Arguments {
    pub values: Vec<Value>,
    pub costs: Vec<usize>,
    pub size: usize,
}

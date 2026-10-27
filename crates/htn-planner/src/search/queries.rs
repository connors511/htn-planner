//! A provider is borrowed for one synchronous attempt; its world must stay immutable.
mod cache;
use super::state::Meter;
pub(super) use cache::{Batch, Queries};

/// A provider failure is fatal for this complete decision, never ordinary absence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryError {
    BudgetExhausted,
    CapacityExceeded,
    InvalidInput,
}
impl From<QueryError> for crate::Outcome {
    fn from(error: QueryError) -> Self {
        match error {
            QueryError::BudgetExhausted => Self::BudgetExhausted,
            QueryError::CapacityExceeded => Self::CapacityExceeded,
            QueryError::InvalidInput => Self::InvalidInput,
        }
    }
}
/// Shared decision allowance. Failures are sticky even if a provider ignores them.
///
/// `reserve(total_live_units)` declares a conservative peak for provider-owned rows
/// and scratch, not an incremental allocation. Include row headers, two units per
/// argument (Value/cached-cost slots), recursive Value nodes and UTF-8 string bytes.
/// Charge before scans, comparisons, copying and allocating. No hidden unmetered
/// host work or allocation is permitted. Independently bounded diagnostic
/// bookkeeping is outside that allowance, as with trace collection; it must not
/// perform extra world queries. Arbitrary third-party code cannot be made pure
/// or memory-safe by this cooperative interface alone.
pub struct QueryBudget<'a> {
    observation_site: crate::trace::Site<'a>,
    observation_parent: Option<crate::TraceId>,
    absence: Option<super::explanation::HostAbsence>,
    meter: &'a mut Meter,
    base: usize,
    bindings: usize,
    reserved: usize,
    failed: Option<QueryError>,
}
impl QueryBudget<'_> {
    /// Whether this actual host task may retain observations under the current focus/cap.
    /// This best-effort preflight cannot know a later static observation code; exact
    /// declaration matching occurs in `observe`. Hosts may skip diagnostic-only emission
    /// loops when false, never host calculations.
    pub fn observations_enabled(&self) -> bool {
        self.meter.trace.observations_enabled(self.observation_site)
    }
    /// Observe an actual host calculation without allocating temporary Values or changing work.
    /// At most ten integers are copied, only after independent trace cap/focus preflight.
    /// An overlong actual matching observation is omitted and marks the trace clipped.
    /// The static code is retained as the locator declaration and charged to the trace cap.
    pub fn observe(&mut self, code: &'static str, values: &[i64]) {
        let site = crate::trace::Site {
            declaration: code,
            ..self.observation_site
        };
        if values.len() > 10 {
            self.meter.trace.omit_observation(site);
            return;
        }
        self.meter.trace.emit(
            self.observation_parent,
            crate::TraceEvent::HostObservation,
            site,
            Some(crate::trace::Snapshot::Integers(values)),
            &self.meter.report,
        );
    }

    /// Retain a bounded explanation for an actually empty batch, when requested.
    /// This observation cannot change search allowances or turn absence into failure.
    /// Codes and up to three integer facts must describe the branch just evaluated.
    pub fn explain_absence(&mut self, code: &'static str, values: &[(&'static str, i64)]) {
        if self.meter.explanation.is_none() {
            return;
        }
        let mut note = super::explanation::HostAbsence {
            code,
            omitted: values.len() > 3,
            ..super::explanation::HostAbsence::default()
        };
        for (slot, value) in note.values.iter_mut().zip(values) {
            *slot = Some(*value);
        }
        self.absence = Some(note);
    }
    /// Charge deterministic work before performing it.
    /// # Errors
    /// Returns and retains exhaustion for the whole decision.
    pub fn charge(&mut self, units: usize) -> Result<(), QueryError> {
        if let Some(error) = self.failed {
            return Err(error);
        }
        self.meter.charge(units).map_err(|outcome| {
            let error = match outcome {
                crate::Outcome::BudgetExhausted => QueryError::BudgetExhausted,
                _ => QueryError::CapacityExceeded,
            };
            self.failed = Some(error);
            error
        })
    }
    /// Reserve the complete simultaneous provider-owned logical payload before allocation.
    /// # Errors
    /// Returns and retains capacity exhaustion, including live search and query cache.
    pub fn reserve(&mut self, total_live_units: usize) -> Result<(), QueryError> {
        if let Some(error) = self.failed {
            return Err(error);
        }
        let total = self.base.saturating_add(total_live_units);
        self.meter.reserve(total, self.bindings).map_err(|_| {
            self.failed = Some(QueryError::CapacityExceeded);
            QueryError::CapacityExceeded
        })?;
        self.reserved = self.reserved.max(total_live_units);
        Ok(())
    }
}
/// Pure stable row enumeration for a declared predicate and fixed bound input prefix.
///
/// Return complete tuples in deterministic host order, preserving every input value.
/// The same immutable world/prefix must mean the same rows for an attempt. Empty
/// rows mean absence. No I/O, clock, RNG, reentrant planning or host-state mutation.
/// Returned data and private scratch must be reserved and work charged before use.
pub trait QueryProvider {
    /// # Errors
    /// A query failure aborts search without an executable prefix.
    fn query(
        &self,
        predicate: &str,
        inputs: &[crate::Value],
        budget: &mut QueryBudget<'_>,
    ) -> Result<Vec<Vec<crate::Value>>, QueryError>;
}

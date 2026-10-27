use serde::{Deserialize, Serialize};
/// Always-on deterministic stage instrumentation; fields saturate at `u64::MAX`.
///
/// Entries count successful precharged stage entry, not a completed plan/query.
/// Rows count completed candidate inspection; effects/restores count completed operations.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageCounters {
    pub tasks_entered: u64,
    pub methods_entered: u64,
    pub queries_entered: u64,
    pub conditions_entered: u64,
    pub rows_inspected: u64,
    pub effect_operations_completed: u64,
    pub choices_restored: u64,
    pub saturated: bool,
}
#[derive(Clone, Copy)]
pub(crate) enum Stage {
    Task,
    Method,
    Query,
    Condition,
    Row,
    Effect,
    Restore,
}
impl StageCounters {
    pub(crate) const fn record(&mut self, stage: Stage) {
        let count = match stage {
            Stage::Task => &mut self.tasks_entered,
            Stage::Method => &mut self.methods_entered,
            Stage::Query => &mut self.queries_entered,
            Stage::Condition => &mut self.conditions_entered,
            Stage::Row => &mut self.rows_inspected,
            Stage::Effect => &mut self.effect_operations_completed,
            Stage::Restore => &mut self.choices_restored,
        };
        let (next, overflow) = count.overflowing_add(1);
        *count = if overflow { u64::MAX } else { next };
        self.saturated |= overflow;
    }
}

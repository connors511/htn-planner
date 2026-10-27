//! Attempt-local batches survive branch rollback; only the overlay is speculative.
use super::{QueryBudget, QueryProvider};
use crate::compiler::CompiledExpression;
use crate::rejected_input::Owned;
use crate::search::{
    expressions,
    state::{Arguments, Bindings, Meter},
};
use crate::{CompiledDomain, Outcome};

pub(crate) struct Batch {
    pub id: usize,
    pub predicate: usize,
    pub rows: Vec<Arguments>,
}
struct Entry {
    predicate: usize,
    input: Arguments,
    batch: Batch,
}
pub(crate) struct Queries<'a> {
    provider: Option<&'a dyn QueryProvider>,
    entries: Vec<Entry>,
}
impl<'a> Queries<'a> {
    pub const fn new(provider: Option<&'a dyn QueryProvider>) -> Self {
        Self {
            provider,
            entries: Vec::new(),
        }
    }
    pub fn get(&self, id: Option<usize>) -> Option<&Batch> {
        id.and_then(|id| self.entries.get(id))
            .map(|entry| &entry.batch)
    }
    pub fn ensure(
        &mut self,
        domain: &CompiledDomain,
        predicate: usize,
        (arguments, env): (&[CompiledExpression], &Bindings),
        (live, bindings, parent): (usize, usize, Option<crate::TraceId>),
        meter: &mut Meter,
    ) -> Result<Option<usize>, Outcome> {
        if self.provider.is_none() {
            return Ok(None);
        }
        let Some(bound) = domain.query_bounds[predicate] else {
            return Ok(None);
        };
        let arguments = arguments.get(..bound).ok_or(Outcome::InvalidInput)?;
        let size = expressions::size(arguments, env, meter)?;
        meter.reserve(live.saturating_add(size).saturating_add(1), bindings)?;
        // Preserve the old logical input-copy charge before lookup, including hits.
        meter.charge(size)?;
        let input = expressions::Input::Expressions {
            expressions: arguments,
            env,
            size,
        };
        self.materialize(domain, predicate, input, (live, bindings, parent), meter)
    }
    pub fn ensure_values(
        &mut self,
        domain: &CompiledDomain,
        predicate: usize,
        arguments: &Arguments,
        (live, bindings, parent): (usize, usize, Option<crate::TraceId>),
        meter: &mut Meter,
    ) -> Result<Option<usize>, Outcome> {
        if self.provider.is_none() {
            return Ok(None);
        }
        let Some(bound) = domain.query_bounds[predicate] else {
            return Ok(None);
        };
        let values = arguments.values.get(..bound).ok_or(Outcome::InvalidInput)?;
        let costs = arguments.costs.get(..bound).ok_or(Outcome::InvalidInput)?;
        meter.charge(costs.len())?;
        let size = costs
            .iter()
            .fold(2 * costs.len(), |n, c| n.saturating_add(*c));
        meter.reserve(live.saturating_add(size).saturating_add(1), bindings)?;
        meter.charge(size)?;
        let input = expressions::Input::Values {
            values,
            costs,
            size,
        };
        self.materialize(domain, predicate, input, (live, bindings, parent), meter)
    }
    fn materialize(
        &mut self,
        domain: &CompiledDomain,
        predicate: usize,
        input: expressions::Input<'_>,
        (live, bindings, parent): (usize, usize, Option<crate::TraceId>),
        meter: &mut Meter,
    ) -> Result<Option<usize>, Outcome> {
        let provider = self.provider.ok_or(Outcome::InvalidInput)?;
        let bound = domain.query_bounds[predicate].ok_or(Outcome::InvalidInput)?;
        for (index, entry) in self.entries.iter().enumerate() {
            meter.charge(1)?;
            if entry.predicate == predicate {
                meter.charge(entry.input.size.saturating_add(input.size()))?;
                if input.matches(&entry.input.values)? {
                    meter.charge(input.size())?;
                    return Ok(Some(index));
                }
            }
        }
        let input = input.own()?;
        let key_cost = 1usize.saturating_add(input.size);
        meter.reserve(live.saturating_add(key_cost), bindings)?;
        let consumed_before = meter.report.consumed;
        meter.provider_predicate = Some(predicate);
        let observation_site = crate::trace::Site {
            source: &domain.source,
            declaration: &domain.schema.predicates[predicate].name,
            task: meter
                .site
                .map(|(task, _, _)| domain.definition.tasks[task].name.as_str()),
            step: meter.site.and_then(|(_, _, step)| step),
        };
        let mut budget = QueryBudget {
            observation_site,
            observation_parent: parent,
            absence: None,
            meter,
            base: live.saturating_add(key_cost),
            bindings,
            reserved: 0,
            failed: None,
        };
        let returned = provider.query(
            &domain.schema.predicates[predicate].name,
            &input.values,
            &mut budget,
        );
        // Install iterative disposal before inspecting malformed provider-owned values.
        let rows = returned.map(Owned::new);
        let failed = budget.failed;
        let reserved = budget.reserved;
        let absence = budget.absence;
        meter.provider_predicate = None;
        meter
            .trace
            .provider_work(predicate, consumed_before, meter.report.consumed);
        if let Some(failed) = failed {
            return Err(failed.into());
        }
        let rows = rows.map_err(Outcome::from)?;
        let base = live.saturating_add(key_cost);
        let mut stored = 0usize;
        let mut validated = Vec::new();
        for row in rows.iter() {
            meter.charge(1)?;
            if stored
                .saturating_add(1)
                .saturating_add(2usize.saturating_mul(row.len()))
                > reserved
            {
                return Err(Outcome::CapacityExceeded);
            }
            let mut scratch = 0;
            let costs = crate::values::validate_values(
                &domain.schema,
                &domain.schema.predicates[predicate].arguments,
                row,
                meter
                    .limits
                    .storage
                    .saturating_sub(meter.query_storage)
                    .saturating_sub(base)
                    .saturating_sub(reserved),
                &mut scratch,
                |units| meter.charge(units),
            )?;
            meter.reserve(
                base.saturating_add(reserved).saturating_add(scratch),
                bindings,
            )?;
            meter.charge(costs.len())?;
            let row_cost = costs
                .iter()
                .fold(2 * costs.len(), |total, cost| total.saturating_add(*cost));
            stored = stored.saturating_add(1).saturating_add(row_cost);
            if stored > reserved {
                return Err(Outcome::CapacityExceeded);
            }
            meter.charge(input.size.saturating_add(row_cost))?;
            if row.get(..bound) != Some(input.values.as_slice()) {
                return Err(Outcome::InvalidInput);
            }
            // Costs are validated before the vector slot is retained.
            meter.reserve(
                base.saturating_add(reserved).saturating_add(stored),
                bindings,
            )?;
            validated.push(costs);
        }
        meter.charge(stored.saturating_add(key_cost))?;
        meter.reserve(
            base.saturating_add(reserved).saturating_add(stored),
            bindings,
        )?;
        let id = self.entries.len();
        let rows: Vec<Arguments> = rows
            .take()
            .into_iter()
            .zip(validated)
            .map(|(values, costs)| {
                let size = costs
                    .iter()
                    .fold(2 * costs.len(), |total, cost| total.saturating_add(*cost));
                Arguments {
                    values,
                    costs,
                    size,
                }
            })
            .collect();
        meter.query_storage = meter
            .query_storage
            .saturating_add(key_cost)
            .saturating_add(stored);
        if rows.is_empty()
            && let Some(absence) = absence
            && let Some(explanation) = &mut meter.explanation
        {
            explanation.note(id, absence);
        }
        self.entries.push(Entry {
            predicate,
            input,
            batch: Batch {
                id,
                predicate,
                rows,
            },
        });
        if meter.trace.enabled() {
            for row in &self.entries[id].batch.rows {
                meter.trace.emit(
                    parent,
                    crate::TraceEvent::QueryCandidate,
                    crate::trace::Site {
                        source: &domain.source,
                        declaration: &domain.schema.predicates[predicate].name,
                        task: meter
                            .site
                            .map(|(task, _, _)| domain.definition.tasks[task].name.as_str()),
                        step: meter.site.and_then(|(_, _, step)| step),
                    },
                    Some(crate::trace::Snapshot::Values(&row.values, &row.costs)),
                    &meter.report,
                );
            }
        }
        Ok(Some(id))
    }
}

use super::queries::Batch;
use super::state::{Arguments, Meter};
use crate::{FactSnapshot, Outcome, Value};
#[derive(Clone, Default)]
pub(super) struct Overlay {
    hidden: Vec<(Option<usize>, usize)>,
    inserted: Vec<Row>,
    pub size: usize,
}
#[derive(Clone)]
struct Row {
    predicate: usize,
    args: Arguments,
    visible: bool,
}
type VisibleRow<'a> = (usize, &'a [Value], &'a [usize], usize);
impl Overlay {
    pub fn count(&self, facts: &FactSnapshot, batch: Option<&Batch>) -> usize {
        facts.facts.len() + batch.map_or(0, |b| b.rows.len()) + self.inserted.len()
    }
    pub fn row<'a>(
        &'a self,
        facts: &'a FactSnapshot,
        batch: Option<&'a Batch>,
        index: usize,
        meter: &mut Meter,
    ) -> Result<Option<VisibleRow<'a>>, Outcome> {
        meter.charge(1)?;
        if index < facts.facts.len() {
            for hidden in &self.hidden {
                meter.charge(1)?;
                if *hidden == (None, index) {
                    return Ok(None);
                }
            }
            return Ok(Some((
                facts.predicates[index],
                &facts.facts[index].arguments,
                &facts.argument_costs[index],
                facts.costs[index],
            )));
        }
        let relative = index - facts.facts.len();
        let length = batch.map_or(0, |b| b.rows.len());
        if let Some(batch) = batch
            && relative < length
        {
            for hidden in &self.hidden {
                meter.charge(1)?;
                if *hidden == (Some(batch.id), relative) {
                    return Ok(None);
                }
            }
            let row = &batch.rows[relative];
            return Ok(Some((batch.predicate, &row.values, &row.costs, row.size)));
        }
        let row = self
            .inserted
            .get(relative - length)
            .ok_or(Outcome::InvalidInput)?;
        Ok(row.visible.then_some((
            row.predicate,
            row.args.values.as_slice(),
            row.args.costs.as_slice(),
            row.args.size,
        )))
    }
    pub fn matches(
        &self,
        facts: &FactSnapshot,
        batch: Option<&Batch>,
        index: usize,
        predicate: usize,
        args: &Arguments,
        meter: &mut Meter,
    ) -> Result<bool, Outcome> {
        let Some((row, values, _, cost)) = self.row(facts, batch, index, meter)? else {
            return Ok(false);
        };
        if row != predicate {
            return Ok(false);
        }
        meter.charge(cost.saturating_add(args.size))?;
        Ok(values == args.values)
    }
    pub fn insert(&mut self, predicate: usize, args: Arguments) {
        self.size += 1 + args.size;
        self.inserted.push(Row {
            predicate,
            args,
            visible: true,
        });
    }
    pub fn remove(&mut self, facts: &FactSnapshot, batch: Option<&Batch>, index: usize) {
        if index < facts.facts.len() {
            self.hidden.push((None, index));
            self.size += 1;
        } else if let Some(batch) = batch
            && index - facts.facts.len() < batch.rows.len()
        {
            self.hidden
                .push((Some(batch.id), index - facts.facts.len()));
            self.size += 1;
        } else {
            self.inserted[index - facts.facts.len() - batch.map_or(0, |b| b.rows.len())].visible =
                false;
        }
    }
    pub fn removal_cost(facts: &FactSnapshot, batch: Option<&Batch>, index: usize) -> usize {
        usize::from(index < facts.facts.len() + batch.map_or(0, |b| b.rows.len()))
    }
}

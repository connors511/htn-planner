use super::syntax::Node;
use htn_planner::{Error, Type};

/// Cold-only unification variables. Their allocation order is source traversal order.
#[derive(Default)]
pub(super) struct Types {
    parents: Vec<usize>,
    values: Vec<Option<Type>>,
    ranks: Vec<u8>,
}
impl Types {
    pub fn fresh(&mut self) -> usize {
        let id = self.parents.len();
        self.parents.push(id);
        self.values.push(None);
        self.ranks.push(0);
        id
    }
    fn root(&self, mut id: usize) -> usize {
        while self.parents[id] != id {
            id = self.parents[id];
        }
        id
    }
    pub fn bind(&mut self, id: usize, ty: &Type, at: &Node) -> Result<(), Error> {
        let root = self.root(id);
        if self.values[root]
            .as_ref()
            .is_some_and(|previous| previous != ty)
        {
            return Err(at.error(format!(
                "conflicting binding types: {:?} and {ty:?}",
                self.values[root]
            )));
        }
        self.values[root] = Some(ty.clone());
        Ok(())
    }
    pub fn unify(&mut self, left: usize, right: usize, at: &Node) -> Result<(), Error> {
        let mut left = self.root(left);
        let mut right = self.root(right);
        if left == right {
            return Ok(());
        }
        if self.ranks[left] < self.ranks[right] {
            std::mem::swap(&mut left, &mut right);
        }
        if let Some(ty) = self.values[right].clone() {
            self.bind(left, &ty, at)?;
        }
        self.parents[right] = left;
        if self.ranks[left] == self.ranks[right] {
            self.ranks[left] += 1;
        }
        Ok(())
    }
    pub fn resolve(&self, id: usize, at: &Node) -> Result<Type, Error> {
        self.values[self.root(id)].clone().ok_or_else(|| at.error("ambiguous binding type; supply a host entry signature or constrain it through the schema"))
    }
}

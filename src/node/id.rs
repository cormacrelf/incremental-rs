use std::cell::Cell;
use std::fmt;

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct NodeId(pub(crate) usize);
impl NodeId {
    pub(super) fn next() -> Self {
        thread_local! {
            static NODE_ID: Cell<usize> = Cell::new(0);
        }

        NODE_ID.with(|x| {
            let next = x.get() + 1;
            x.set(next);
            NodeId(next)
        })
    }
}
impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

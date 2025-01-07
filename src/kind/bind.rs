use std::cell::RefCell;
use std::{cell::Cell, fmt};

use crate::boxes::SmallBox;
use crate::incrsan::NotObserver;
use crate::node::NodeId;
use crate::scope::{BindScope, Scope};
use crate::ValueInternal;
use crate::{NodeRef, WeakNode};

pub(crate) struct BindNode {
    pub id_lhs_change: Cell<NodeId>,
    pub lhs_change: RefCell<WeakNode>,
    pub main: RefCell<WeakNode>,
    pub lhs: NodeRef,
    pub mapper: RefCell<SmallBox<dyn LhsChangeFn>>,
    pub rhs: RefCell<Option<NodeRef>>,
    pub rhs_scope: RefCell<Scope>,
    pub all_nodes_created_on_rhs: RefCell<Vec<WeakNode>>,
}

impl BindScope for BindNode {
    fn id(&self) -> NodeId {
        self.id_lhs_change.get()
    }
    fn is_valid(&self) -> bool {
        let main_ = self.main.borrow();
        let Some(main) = main_.upgrade() else {
            return false;
        };
        main.is_valid()
    }
    fn is_necessary(&self) -> bool {
        let main_ = self.main.borrow();
        let Some(main) = main_.upgrade() else {
            return false;
        };
        main.is_necessary()
    }
    fn height(&self) -> i32 {
        let lhs_change_ = self.lhs_change.borrow();
        let lhs_change = lhs_change_.upgrade().unwrap();
        lhs_change.height()
    }
    fn add_node(&self, node: WeakNode) {
        tracing::info!(
            "added node to BindScope({:?}): {:?}",
            self.id(),
            node.upgrade()
        );
        let mut all = self.all_nodes_created_on_rhs.borrow_mut();
        all.push(node);
    }
}

pub(crate) trait LhsChangeFn:
    FnMut(&dyn ValueInternal) -> NodeRef + 'static + NotObserver
{
}
impl<F> LhsChangeFn for F where F: FnMut(&dyn ValueInternal) -> NodeRef + 'static + NotObserver {}

impl fmt::Debug for BindNode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BindNode")
            // .field("output", &self.rhs.borrow().as_ref().map(|x| &x.node))
            .finish()
    }
}

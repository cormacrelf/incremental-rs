use std::any::Any;
use std::cell::RefCell;
use std::rc::Weak;
use std::{cell::Cell, fmt};

use refl::Id;

use super::NodeGenerics;
use crate::incrsan::NotObserver;
use crate::node::{Node, NodeId};
use crate::scope::{BindScope, Scope};
use crate::{Incr, Value};
use crate::{NodeRef, WeakNode};

pub(crate) struct BindNode<R>
where
    R: Value,
{
    pub id_lhs_change: Cell<NodeId>,
    pub lhs_change: RefCell<Weak<Node<BindLhsChangeGen<R>>>>,
    pub main: RefCell<WeakNode>,
    pub lhs: NodeRef,
    pub mapper: RefCell<Box<dyn LhsChangeFn<R>>>,
    pub rhs: RefCell<Option<NodeRef>>,
    pub rhs_scope: RefCell<Scope>,
    pub all_nodes_created_on_rhs: RefCell<Vec<WeakNode>>,
}

impl<R> BindScope for BindNode<R>
where
    R: Value,
{
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
        lhs_change.height.get()
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

pub(crate) struct BindLhsChangeGen<R> {
    _phantom: std::marker::PhantomData<R>,
}

pub(crate) trait LhsChangeFn<R>: FnMut(&dyn Any) -> Incr<R> + 'static + NotObserver {}
impl<F, R> LhsChangeFn<R> for F where F: FnMut(&dyn Any) -> Incr<R> + 'static + NotObserver {}

impl<R> NodeGenerics for BindLhsChangeGen<R>
where
    R: Value,
{
    // lhs change's Node stores () nothing. just a sentinel.
    type R = ();
    // type BindLhs = T;
    type BindRhs = R;
    // swap order so we can use as_parent with I1
    type I1 = ();
    // type I2 = T;
    node_generics_default! { I2, I3, I4, I5, I6 }
    node_generics_default! { F1, F2, F3, F4, F5, F6 }
    node_generics_default! { Fold, Update, WithOld, FRef, Recompute, ObsChange }
}

pub(crate) struct BindNodeMainGen<R> {
    _phantom: std::marker::PhantomData<R>,
}

impl<R> NodeGenerics for BindNodeMainGen<R>
where
    R: Value,
{
    // We copy the output of the Rhs
    type R = R;
    // type BindLhs = T;
    type BindRhs = R;
    /// Rhs
    type I1 = R;
    // BindLhsChange (a sentinel)
    // type I2 = ();
    node_generics_default! { I3, I4, I5, I6 }
    node_generics_default! { F1, F2, F3, F4, F5, F6 }
    node_generics_default! { Fold, Update, WithOld, FRef, Recompute, ObsChange }
}

impl<R> fmt::Debug for BindNode<R>
where
    R: Value,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BindNode")
            // .field("output", &self.rhs.borrow().as_ref().map(|x| &x.node))
            .finish()
    }
}

pub(crate) struct BindLhsId<G: NodeGenerics> {
    pub(crate) r_unit: Id<(), G::R>,
}

impl<G: NodeGenerics> fmt::Debug for BindLhsId<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BindLhsId").finish()
    }
}

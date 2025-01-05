use std::cell::RefCell;
use std::rc::Weak;
use std::{cell::Cell, fmt};

use refl::Id;

use super::NodeGenerics;
use crate::incrsan::NotObserver;
use crate::node::{ErasedNode, Input, Node, NodeId};
use crate::scope::{BindScope, Scope};
use crate::WeakNode;
use crate::{Incr, Value};

pub(crate) struct BindNode<T, R>
where
    R: Value,
    T: Value,
{
    pub id_lhs_change: Cell<NodeId>,
    pub lhs_change: RefCell<Weak<Node<BindLhsChangeGen<T, R>>>>,
    pub main: RefCell<Weak<Node<BindNodeMainGen<T, R>>>>,
    pub lhs: Input<T>,
    pub mapper: RefCell<Box<dyn LhsChangeFn<T, R>>>,
    pub rhs: RefCell<Option<Incr<R>>>,
    pub rhs_scope: RefCell<Scope>,
    pub all_nodes_created_on_rhs: RefCell<Vec<WeakNode>>,
}

impl<T, R> BindScope for BindNode<T, R>
where
    R: Value,
    T: Value,
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

pub(crate) struct BindLhsChangeGen<T, R> {
    _phantom: std::marker::PhantomData<(T, R)>,
}

pub(crate) trait LhsChangeFn<T, R>: FnMut(&T) -> Incr<R> + 'static + NotObserver {}
impl<F, T, R> LhsChangeFn<T, R> for F where F: FnMut(&T) -> Incr<R> + 'static + NotObserver {}

impl<T, R> NodeGenerics for BindLhsChangeGen<T, R>
where
    T: Value,
    R: Value,
{
    // lhs change's Node stores () nothing. just a sentinel.
    type R = ();
    type BindLhs = T;
    type BindRhs = R;
    // swap order so we can use as_parent with I1
    type I1 = ();
    type I2 = T;
    node_generics_default! { I3, I4, I5, I6 }
    node_generics_default! { F1, F2, F3, F4, F5, F6 }
    node_generics_default! { Fold, Update, WithOld, FRef, Recompute, ObsChange }
}

pub(crate) struct BindNodeMainGen<T, R> {
    _phantom: std::marker::PhantomData<(T, R)>,
}

impl<T, R> NodeGenerics for BindNodeMainGen<T, R>
where
    T: Value,
    R: Value,
{
    // We copy the output of the Rhs
    type R = R;
    type BindLhs = T;
    type BindRhs = R;
    /// Rhs
    type I1 = R;
    /// BindLhsChange (a sentinel)
    type I2 = ();
    node_generics_default! { I3, I4, I5, I6 }
    node_generics_default! { F1, F2, F3, F4, F5, F6 }
    node_generics_default! { Fold, Update, WithOld, FRef, Recompute, ObsChange }
}

impl<T, R> fmt::Debug for BindNode<T, R>
where
    R: Value,
    T: Value,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BindNode")
            .field("output", &self.rhs.borrow().as_ref().map(|x| &x.node))
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

pub(crate) struct BindMainId<G: NodeGenerics> {
    pub(crate) rhs_r: Id<G::BindRhs, G::R>,
    pub(crate) input_rhs_i1: Id<Input<G::BindRhs>, Input<G::I1>>,
    pub(crate) input_lhs_i2: Id<Input<()>, Input<G::I2>>,
}

impl<G: NodeGenerics> fmt::Debug for BindMainId<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BindMainId")
            .field("d_eq_r", &self.rhs_r)
            .field("input_lhs_change_unit", &self.input_lhs_i2)
            .finish()
    }
}

// use enum_dispatch::enum_dispatch;

use crate::v_rch::MapWithOld;
use crate::Value;

use super::adjust_heights_heap::AdjustHeightsHeap;
use super::cutoff::Cutoff;
use super::internal_observer::{InternalObserver, ObserverId};
use super::kind::{Kind, NodeGenerics};
use super::node_update::NodeUpdateDelayed;
use super::scope::Scope;
use super::state::State;
use super::{Incr, Map2Node, MapNode, NodeRef, WeakNode};
use core::fmt::Debug;
use std::cell::Ref;
use std::collections::HashMap;
use std::rc::Weak;

use super::stabilisation_num::StabilisationNum;
use refl::Id;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct NodeId(usize);
impl NodeId {
    fn next() -> Self {
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

/// Needs a better name but ok
#[derive(Debug)]
pub(crate) struct NodeInner<'a> {
    pub(crate) state: Weak<State<'a>>,
    // TODO: optimise the one parent case
    // pub(crate) num_parents: i32,
    // parent0
    // parent1_and_beyond
    pub(crate) my_parent_index_in_child_at_index: Vec<i32>,
    pub(crate) my_child_index_in_parent_at_index: Vec<i32>,
    pub(crate) force_necessary: bool,
}

pub(crate) struct Node<'a, G: NodeGenerics<'a>> {
    pub(crate) id: NodeId,
    pub(crate) inner: RefCell<NodeInner<'a>>,
    pub(crate) kind: RefCell<Kind<'a, G>>,
    pub(crate) value_opt: RefCell<Option<G::R>>,
    pub(crate) old_value_opt: RefCell<Option<G::R>>,
    pub height: Cell<i32>,
    pub old_height: Cell<i32>,
    pub height_in_recompute_heap: Cell<i32>,
    pub height_in_adjust_heights_heap: Cell<i32>,
    pub is_in_handle_after_stabilisation: Cell<bool>,
    pub num_on_update_handlers: Cell<i32>,
    pub(crate) created_in: Scope<'a>,
    pub recomputed_at: Cell<StabilisationNum>,
    pub changed_at: Cell<StabilisationNum>,
    weak_self: Weak<Self>,
    pub(crate) parents: RefCell<Vec<ParentRef<'a, G::R>>>,
    pub(crate) observers: RefCell<HashMap<ObserverId, Weak<InternalObserver<'a, G::R>>>>,
    pub(crate) cutoff: Cell<Cutoff<G::R>>,
}

pub(crate) type Input<'a, R> = Rc<dyn Incremental<'a, R> + 'a>;

pub(crate) trait Incremental<'a, R>: ErasedNode<'a> + Debug {
    fn as_input(&self) -> Input<'a, R>;
    fn latest(&self) -> R;
    fn value_opt(&self) -> Option<R>;
    fn add_parent_without_adjusting_heights(&self, child_index: i32, parent_weak: ParentRef<'a, R>);
    fn state_add_parent(&self, child_index: i32, parent_weak: ParentRef<'a, R>);
    fn remove_parent(&self, child_index: i32, parent_weak: ParentRef<'a, R>);
    fn set_cutoff(&self, cutoff: Cutoff<R>);
    fn value_as_ref(&self) -> Option<Ref<R>>;
    fn add_observer(&self, id: ObserverId, weak: Weak<InternalObserver<'a, R>>);
    fn remove_observer(&self, id: ObserverId);
}

impl<'a, G: NodeGenerics<'a> + 'a> Incremental<'a, G::R> for Node<'a, G> {
    fn as_input(&self) -> Input<'a, G::R> {
        self.weak_self.upgrade().unwrap() as Input<G::R>
    }
    fn latest(&self) -> G::R {
        self.value_opt.borrow().clone().unwrap()
    }
    fn value_as_ref(&self) -> Option<Ref<G::R>> {
        let v = self.value_opt.borrow();
        Ref::filter_map(v, |v| v.as_ref()).ok()
    }
    fn value_opt(&self) -> Option<G::R> {
        self.value_opt.borrow().clone()
    }

    // #[tracing::instrument]
    fn add_parent_without_adjusting_heights(
        &self,
        child_index: i32,
        parent_weak: ParentRef<'a, G::R>,
    ) {
        let Some(p) = parent_weak.upgrade_erased() else { panic!() };
        debug_assert!(p.is_necessary());
        let was_necessary = self.is_necessary();
        self.add_parent(child_index, parent_weak);
        if !self.is_valid() {
            let t = self.state();
            let mut pi = t.propagate_invalidity.borrow_mut();
            pi.push(p.weak());
        }
        if !was_necessary {
            self.became_necessary();
        }
    }
    fn state_add_parent(&self, child_index: i32, parent_weak: ParentRef<'a, G::R>) {
        let parent = parent_weak.upgrade_erased().unwrap();
        debug_assert!(parent.is_necessary());
        self.add_parent_without_adjusting_heights(child_index, parent_weak);
        if self.height() >= parent.height() {
            // This happens when change_child whacks a neew child in
            // What's happening here is that
            tracing::debug!(
                "self.height() = {:?}, parent.height() = {:?}",
                self.height(),
                parent.height()
            );
            let t = self.state();
            let mut ahh = t.adjust_heights_heap.borrow_mut();
            let mut rch = t.recompute_heap.borrow_mut();
            ahh.adjust_heights(&mut rch, self.packed(), parent.packed());
        }
        self.state().propagate_invalidity();
        debug_assert!(parent.is_necessary());
        /* we only add necessary parents */
        if !parent.is_in_recompute_heap()
            && (parent.recomputed_at().get().is_none() || self.edge_is_stale(parent.weak()))
        {
            let t = self.state();
            let mut rch = t.recompute_heap.borrow_mut();
            rch.insert(parent.packed());
        }
    }
    fn remove_parent(&self, child_index: i32, parent_weak: ParentRef<'a, G::R>) {
        let child = self;
        let child_inner = child.inner();
        let mut ci = child_inner.borrow_mut();
        let mut child_parents = child.parents.borrow_mut();
        let parent = parent_weak.upgrade_erased().unwrap();
        let parent_inner = parent.inner();
        let mut parent_i = parent_inner.borrow_mut();

        debug_assert!(child_parents.len() >= 1);
        let parent_index = parent_i.my_parent_index_in_child_at_index[child_index as usize];
        debug_assert!(parent_weak.ptr_eq(&child_parents[parent_index as usize].clone()));
        let last_parent_index = child_parents.len() - 1;
        if (parent_index as usize) < last_parent_index {
            // we swap the parent the end of the array into this one's position. This keeps the array
            // small.
            let end_p_weak = child_parents[last_parent_index].clone();
            if let Some(end_p) = end_p_weak.upgrade_erased() {
                let end_p_inner = end_p.inner();
                let mut end_p_i = end_p_inner.borrow_mut();
                let end_child_index = ci.my_child_index_in_parent_at_index[last_parent_index];
                // link parent_index & end_child_index
                end_p_i.my_parent_index_in_child_at_index[end_child_index as usize] = parent_index;
                ci.my_child_index_in_parent_at_index[parent_index as usize] = end_child_index;
            }
        }
        // unlink last_parent_index & child_index
        parent_i.my_parent_index_in_child_at_index[child_index as usize] = -1;
        ci.my_child_index_in_parent_at_index[last_parent_index] = -1;

        // now do what we just did but super easily in the actual Vec
        child_parents.swap_remove(parent_index as usize);
    }
    fn set_cutoff(&self, cutoff: Cutoff<G::R>) {
        self.cutoff.set(cutoff)
    }
    fn add_observer(&self, id: ObserverId, weak: Weak<InternalObserver<'a, G::R>>) {
        let mut os = self.observers.borrow_mut();
        os.insert(id, weak);
    }
    fn remove_observer(&self, id: ObserverId) {
        let mut os = self.observers.borrow_mut();
        os.remove(&id);
    }
}

pub(crate) trait ParentNode<'a, I> {
    fn child_changed(&self, child: &Input<'a, I>, child_index: i32, old_value_opt: Option<I>);
}
pub(crate) trait Parent1<'a, I>: Debug + ErasedNode<'a> {
    fn child_changed(&self, child: &Input<'a, I>, child_index: i32, old_value_opt: Option<I>);
}
pub(crate) trait Parent2<'a, I>: Debug + ErasedNode<'a> {}

#[derive(Clone, Debug)]
pub(crate) enum ParentRef<'a, T> {
    I1(Weak<dyn Parent1<'a, T> + 'a>),
    I2(Weak<dyn Parent2<'a, T> + 'a>),
}

impl<'a, T: Value<'a>> ParentRef<'a, T> {
    fn upgrade_erased(&self) -> Option<NodeRef<'a>> {
        match self {
            Self::I1(w) => w.upgrade().map(|x| x.packed()),
            Self::I2(w) => w.upgrade().map(|x| x.packed()),
        }
    }
    fn ptr_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::I1(a), Self::I1(b)) => a.ptr_eq(b),
            (Self::I2(a), Self::I2(b)) => a.ptr_eq(b),
            _ => false,
        }
    }
}

impl<'a, T: Value<'a>> ParentNode<'a, T> for ParentRef<'a, T> {
    fn child_changed(&self, child: &Input<'a, T>, child_index: i32, old_value_opt: Option<T>) {
        match self {
            Self::I1(i1) => {
                let i1 = i1.upgrade().unwrap();
                i1.child_changed(child, child_index, old_value_opt)
            }
            // We don't need this for i2
            _ => {}
        }
    }
}
impl<'a, G: NodeGenerics<'a>> Parent2<'a, G::I2> for Node<'a, G> {}
impl<'a, G: NodeGenerics<'a>> Parent1<'a, G::I1> for Node<'a, G> {
    fn child_changed(
        &self,
        child: &Input<'a, G::I1>,
        child_index: i32,
        old_value_opt: Option<G::I1>,
    ) {
        let k = self.kind.borrow();
        match &*k {
            // Kind::Expert => ...,
            Kind::UnorderedArrayFold(uaf) => {
                let new_value = child.value_as_ref().unwrap();
                uaf.child_changed(child, child_index, old_value_opt, &*new_value)
            }
            _ => {}
        }
    }
}

pub(crate) trait ErasedNode<'a>: Debug {
    fn id(&self) -> NodeId;
    fn is_valid(&self) -> bool;
    fn should_be_invalidated(&self) -> bool;
    fn propagate_invalidity_helper(&self);
    fn has_invalid_child(&self) -> bool;
    fn height_in_recompute_heap(&self) -> &Cell<i32>;
    fn height_in_adjust_heights_heap(&self) -> &Cell<i32>;
    fn is_in_handle_after_stabilisation(&self) -> &Cell<bool>;
    fn ensure_parent_height_requirements(
        &self,
        ahh: &mut AdjustHeightsHeap<'a>,
        original_child: &NodeRef<'a>,
        original_parent: &NodeRef<'a>,
    );
    fn height(&self) -> i32;
    /// Only for use from AdjustHeightsHeap.
    fn set_height(&self, height: i32);
    fn old_height(&self) -> i32;
    fn set_old_height(&self, height: i32);
    fn is_stale(&self) -> bool;
    fn is_stale_with_respect_to_a_child(&self) -> bool;
    fn edge_is_stale(&self, parent: WeakNode<'a>) -> bool;
    fn is_necessary(&self) -> bool;
    fn needs_to_be_computed(&self) -> bool;
    fn became_necessary(&self);
    fn became_necessary_propagate(&self);
    fn became_unnecessary(&self);
    fn check_if_unnecessary(&self);
    fn is_in_recompute_heap(&self) -> bool;
    fn recompute(&self);
    fn inner(&self) -> &RefCell<NodeInner<'a>>;
    fn state_opt(&self) -> Option<Rc<State<'a>>>;
    fn state(&self) -> Rc<State<'a>>;
    fn weak(&self) -> WeakNode<'a>;
    fn packed(&self) -> NodeRef<'a>;
    fn foreach_child(&self, f: &mut dyn FnMut(i32, NodeRef<'a>) -> ());
    fn recomputed_at(&self) -> &Cell<StabilisationNum>;
    fn invalidate(&self);
    fn invalidate_node(&self);
    #[cfg(debug_assertions)]
    fn assert_currently_running_node_is_child(&self, name: &'static str);
    #[cfg(debug_assertions)]
    fn assert_currently_running_node_is_parent(&self, name: &'static str);
    fn has_child(&self, child: &WeakNode<'a>) -> bool;
    fn adjust_heights_bind_lhs_change(
        &self,
        ahh: &mut AdjustHeightsHeap<'a>,
        oc: &NodeRef<'a>,
        op: &NodeRef<'a>,
    );
    fn created_in(&self) -> Scope<'a>;
    fn run_on_update_handlers(&self, node_update: NodeUpdateDelayed, now: StabilisationNum);
    fn maybe_handle_after_stabilisation(&self);
    fn handle_after_stabilisation(&self);
    fn num_on_update_handlers(&self) -> &Cell<i32>;
    fn node_update(&self) -> NodeUpdateDelayed;
}

impl<'a, G: NodeGenerics<'a>> Debug for Node<'a, G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("id", &self.id)
            // .field("height", &self.height.get())
            .field("kind", &self.kind.borrow())
            // .field("inner", &self.inner)
            .finish()
    }
}

impl<'a, G: NodeGenerics<'a>> ErasedNode<'a> for Node<'a, G> {
    fn id(&self) -> NodeId {
        self.id
    }
    fn weak(&self) -> WeakNode<'a> {
        self.weak_self.clone() as WeakNode<'a>
    }
    fn packed(&self) -> NodeRef<'a> {
        self.weak_self.upgrade().unwrap() as NodeRef<'a>
    }
    fn inner(&self) -> &RefCell<NodeInner<'a>> {
        &self.inner
    }
    fn state_opt(&self) -> Option<Rc<State<'a>>> {
        self.inner().borrow().state.upgrade()
    }
    fn state(&self) -> Rc<State<'a>> {
        self.inner().borrow().state.upgrade().unwrap()
    }
    fn is_valid(&self) -> bool {
        let k = self.kind.borrow();
        match &*k {
            Kind::Invalid => false,
            _ => true,
        }
    }
    fn should_be_invalidated(&self) -> bool {
        let k = self.kind.borrow();
        match &*k {
            Kind::Uninitialised => panic!(),
            Kind::Invalid | Kind::Constant(_) | Kind::Var(_) => false,
            Kind::ArrayFold(..)
            | Kind::UnorderedArrayFold(..)
            | Kind::Map(..)
            | Kind::MapWithOld(..)
            | Kind::Map2(..) => self.has_invalid_child(),
            /* A *_change node is invalid if the node it is watching for changes is invalid (same
            reason as above).  This is equivalent to [has_invalid_child t]. */
            Kind::BindLhsChange(_, bind) => {
                let Some(bind) = bind.upgrade() else { return false };
                !bind.lhs.is_valid()
            }
            /* [Bind_main], [If_then_else], and [Join_main] are invalid if their *_change child is,
            but not necessarily if their other children are -- the graph may be restructured to
            avoid the invalidity of those. */
            Kind::BindMain(_, bind) => !bind.lhs_change.is_valid(),
        }
    }
    fn propagate_invalidity_helper(&self) {
        let k = self.kind.borrow();
        match &*k {
            // Kind::Expert => ...
            kind => {
                #[cfg(debug_assertions)]
                match kind {
                    Kind::BindMain(..) => (), // and IfThenElse, JoinMain
                    _ => panic!("nodes with no children are never pushed on the stack"),
                }
            }
        }
    }
    fn has_invalid_child(&self) -> bool {
        let mut any = false;
        self.foreach_child(&mut |ix, child| {
            any = any || !child.is_valid();
        });
        any
    }
    fn height(&self) -> i32 {
        self.height.get()
    }
    fn height_in_recompute_heap(&self) -> &Cell<i32> {
        &self.height_in_recompute_heap
    }
    fn height_in_adjust_heights_heap(&self) -> &Cell<i32> {
        &self.height_in_adjust_heights_heap
    }
    fn is_in_handle_after_stabilisation(&self) -> &Cell<bool> {
        &self.is_in_handle_after_stabilisation
    }
    fn ensure_parent_height_requirements(
        &self,
        ahh: &mut AdjustHeightsHeap<'a>,
        original_child: &NodeRef<'a>,
        original_parent: &NodeRef<'a>,
    ) {
        let ps = self.parents.borrow();
        for parent in ps.iter() {
            let parent = parent.upgrade_erased().unwrap().packed();
            ahh.ensure_height_requirement(
                &original_child,
                &original_parent,
                &self.packed(),
                &parent,
            );
        }
    }
    fn set_height(&self, height: i32) {
        tracing::trace!("{:?} set height to {height}", self.id);
        self.height.set(height);
    }
    fn old_height(&self) -> i32 {
        self.old_height.get()
    }
    fn set_old_height(&self, height: i32) {
        self.old_height.set(height);
    }
    fn is_stale(&self) -> bool {
        let k = self.kind.borrow();
        match &*k {
            Kind::Invalid | Kind::Uninitialised => panic!(),
            Kind::Var(var) => {
                let set_at = var.set_at.get();
                let recomputed_at = self.recomputed_at.get();
                set_at > recomputed_at
            }
            Kind::Constant(v) => self.recomputed_at.get() == StabilisationNum(-1),
            Kind::ArrayFold(_)
            | Kind::UnorderedArrayFold(_)
            | Kind::Map(_)
            | Kind::MapWithOld(_)
            | Kind::Map2(_)
            | Kind::BindLhsChange(..)
            | Kind::BindMain(..) => {
                self.recomputed_at.get() == StabilisationNum(-1)
                    || self.is_stale_with_respect_to_a_child()
            }
        }
    }
    fn is_stale_with_respect_to_a_child(&self) -> bool {
        let mut is_stale = false;
        let parent = self.weak();
        // TODO: make a version of try_fold for this, to short-circuit it
        self.foreach_child(&mut |ix, child| {
            if child.edge_is_stale(parent.clone()) {
                is_stale = true;
            }
        });
        is_stale
    }
    fn edge_is_stale(&self, parent: WeakNode<'a>) -> bool {
        let Some(parent) = parent.upgrade() else { return false };
        self.changed_at.get() > parent.recomputed_at().get()
    }
    fn is_necessary(&self) -> bool {
        !self.parents.borrow().is_empty()
            || !self.observers.borrow().is_empty()
            // || kind is freeze
            || self.inner().borrow().force_necessary
    }
    fn needs_to_be_computed(&self) -> bool {
        self.is_necessary() && self.is_stale()
    }
    // Not used in add_parent_without_adjusting_heights, I think.
    // Used for `set_freeze`, `add_observers`
    fn became_necessary_propagate(&self) {
        self.became_necessary();
        let state = self.state();
        state.propagate_invalidity();
    }
    // #[tracing::instrument]
    fn became_necessary(&self) {
        if self.is_valid() && !self.created_in.is_necessary() {
            panic!("trying to make a node necessary whose defining bind is not necessary");
        }
        let t = self.state();
        t.num_nodes_became_necessary
            .set(t.num_nodes_became_necessary.get() + 1);
        self.maybe_handle_after_stabilisation();
        /* Since [node] became necessary, to restore the invariant, we need to:
        - add parent pointers to [node] from its children.
        - set [node]'s height.
        - add [node] to the recompute heap, if necessary. */
        let weak = self.clone().weak();
        let as_parent = self.as_parent();
        t.set_height(self.packed(), self.created_in.height() + 1);
        let h = &Cell::new(self.height());
        let p1 = self.as_parent();
        let p2 = self.as_parent2();
        self.foreach_child_typed(ForeachChild {
            i1: &mut move |index, child| {
                child.add_parent_without_adjusting_heights(index, p1.clone());
                if child.height() >= h.get() {
                    h.set(child.height() + 1);
                }
            },
            i2: &mut move |index, child| {
                child.add_parent_without_adjusting_heights(index, p2.clone());
                if child.height() >= h.get() {
                    h.set(child.height() + 1);
                }
            },
        });
        t.set_height(self.packed(), h.get());
        debug_assert!(!self.is_in_recompute_heap());
        debug_assert!(self.is_necessary());
        if self.is_stale() {
            let mut rch = t.recompute_heap.borrow_mut();
            rch.insert(self.packed());
        }
    }
    fn check_if_unnecessary(&self) {
        if !self.is_necessary() {
            self.became_unnecessary();
        }
    }
    fn became_unnecessary(&self) {
        let t = self.state();
        t.num_nodes_became_unnecessary
            .set(t.num_nodes_became_unnecessary.get() + 1);
        self.maybe_handle_after_stabilisation();
        t.set_height(self.packed(), -1);
        self.remove_children();
        let kind = self.kind.borrow();
        match &*kind {
            Kind::UnorderedArrayFold(uaf) => uaf.force_full_compute(),
            _ => {}
        }
        debug_assert!(!self.needs_to_be_computed());
        if self.is_in_recompute_heap() {
            let t = self.state();
            let mut rch = t.recompute_heap.borrow_mut();
            rch.remove(self.packed());
        }
    }
    fn foreach_child(&self, f: &mut dyn FnMut(i32, NodeRef<'a>) -> ()) {
        let k = self.kind.borrow();
        match &*k {
            Kind::Invalid => {}
            Kind::Uninitialised => {}
            Kind::Constant(_) => {}
            Kind::Map(MapNode { input, .. }) => f(0, input.packed()),
            Kind::MapWithOld(MapWithOld { input, .. }) => f(0, input.packed()),
            Kind::Map2(Map2Node { one, two, .. }) => {
                f(0, one.clone().packed());
                f(1, two.clone().packed());
            }
            Kind::BindLhsChange(_, bind) => {
                let Some(bind) = bind.upgrade() else { return };
                f(0, bind.lhs.packed())
            }
            Kind::BindMain(_, bind) => {
                f(0, bind.lhs_change.packed());
                if let Some(rhs) = bind.rhs.borrow().as_ref() {
                    f(1, rhs.node.packed())
                }
            }
            Kind::ArrayFold(af) => {
                for (ix, child) in af.children.iter().enumerate() {
                    f(ix as i32, child.node.packed())
                }
            }
            Kind::UnorderedArrayFold(uaf) => {
                for (ix, child) in uaf.children.iter().enumerate() {
                    f(ix as i32, child.node.packed())
                }
            }
            Kind::Var(var) => {}
        }
    }
    fn is_in_recompute_heap(&self) -> bool {
        self.height_in_recompute_heap.get() >= 0
    }
    fn recomputed_at(&self) -> &Cell<StabilisationNum> {
        &self.recomputed_at
    }

    #[tracing::instrument]
    fn recompute(&self) {
        let t = self.state();
        t.num_nodes_recomputed.set(t.num_nodes_recomputed.get() + 1);
        self.recomputed_at.set(t.stabilisation_num.get());
        let k = self.kind.borrow();
        let id = self.id;
        let height = self.height();
        match &*k {
            Kind::Constant(v) => {
                self.maybe_change_value(v.clone());
            }
            Kind::Var(var) => {
                let value = var.value.borrow();
                let v = value.clone();
                tracing::debug!("recomputing Var(id={id:?}) <- {v:?}");
                drop(value);
                self.maybe_change_value(v);
            }
            Kind::Map(map) => {
                let map: &MapNode<G::F1, G::I1, G::R> = map;
                let input = map.input.value_as_ref().unwrap();
                let mut f = map.mapper.borrow_mut();
                let new_value = f(&input);
                tracing::debug!("<- {new_value:?}");
                self.maybe_change_value(new_value);
            }
            Kind::MapWithOld(map) => {
                let map: &MapWithOld<G::WithOld, G::I1, G::R> = map;
                let input = map.input.value_as_ref().unwrap();
                let mut f = map.mapper.borrow_mut();
                let old_value = self.value_opt.take();
                let old_value_is_none = old_value.is_none();
                let (new_value, did_change) = f(old_value, &input);
                tracing::debug!("<- {new_value:?}");
                self.maybe_change_value_manual(None, new_value, old_value_is_none || did_change);
            }
            Kind::Map2(map2) => {
                let map2: &Map2Node<'a, G::F2, G::I1, G::I2, G::R> = map2;
                let i1 = map2.one.value_as_ref().unwrap();
                let i2 = map2.two.value_as_ref().unwrap();
                let mut f = map2.mapper.borrow_mut();
                let new_value = f(&i1, &i2);
                tracing::debug!("recomputing Map2(id={id:?}) <- {new_value:?}");
                self.maybe_change_value(new_value);
            }
            Kind::BindLhsChange(id, bind) => {
                let Some(bind) = bind.upgrade() else { return };
                // leaves an empty vec for next time
                let mut old_all_nodes_created_on_rhs = bind.all_nodes_created_on_rhs.take();
                let t = self.state();
                let lhs = bind.lhs.value_as_ref().unwrap();
                let rhs = {
                    let old_scope = t.current_scope();
                    *t.current_scope.borrow_mut() = bind.rhs_scope.borrow().clone();
                    tracing::debug!("-- recomputing BindLhsChange(id={id:?}, {lhs:?})");
                    let mut f = bind.mapper.borrow_mut();
                    let rhs = f(&lhs);
                    *t.current_scope.borrow_mut() = old_scope;
                    tracing::debug!("-- recomputing BindLhsChange(id={id:?}) <- {rhs:?}");
                    // Check that the returned RHS node is from the same world.
                    assert!(Rc::ptr_eq(&rhs.node.state(), &t));
                    rhs
                };

                let mut old_rhs = Some(rhs.clone());
                {
                    let mut bind_rhs = bind.rhs.borrow_mut();
                    core::mem::swap(&mut *bind_rhs, &mut old_rhs);
                }
                /* Anticipate what [maybe_change_value] will do, to make sure Bind_main is stale
                right away. This way, if the new child is invalid, we'll satisfy the invariant
                saying that [needs_to_be_computed bind_main] in [propagate_invalidity] */
                self.changed_at.set(t.stabilisation_num.get());
                if let Some(main) = bind.main.upgrade() {
                    main.change_child_bind_rhs(
                        old_rhs.clone(),
                        rhs,
                        Kind::<G>::BIND_RHS_CHILD_INDEX,
                    );
                }
                if old_rhs.is_some() {
                    /* We invalidate after [change_child], because invalidation changes the [kind] of
                    nodes to [Invalid], which means that we can no longer visit their children.
                    Also, the [old_rhs] nodes are typically made unnecessary by [change_child], and
                    so by invalidating afterwards, we will not waste time adding them to the
                    recompute heap and then removing them. */
                    // We don't support this config option
                    const BIND_LHS_CHANGE_SHOULD_INVALIDATE_RHS: bool = true;
                    if BIND_LHS_CHANGE_SHOULD_INVALIDATE_RHS {
                        invalidate_nodes_created_on_rhs(&mut old_all_nodes_created_on_rhs)
                    } else {
                        panic!();
                    }
                    t.propagate_invalidity();
                }
                /* [node] was valid at the start of the [Bind_lhs_change] branch, and invalidation
                only visits higher nodes, so [node] is still valid. */
                debug_assert!(self.is_valid());
                self.maybe_change_value(id.r_unit.cast(()));
            }
            Kind::BindMain(id, bind) => {
                let rhs = bind.rhs.borrow().as_ref().unwrap().clone();
                tracing::debug!("-- recomputing BindMain(id={id:?}, h={height:?}) <- {rhs:?}");
                self.copy_child_bindrhs(&rhs.node, id.rhs_r);
            }
            Kind::ArrayFold(af) => self.maybe_change_value(af.compute()),
            Kind::UnorderedArrayFold(uaf) => {
                tracing::debug!("-- recomputing UAF {uaf:?}");
                self.maybe_change_value(uaf.compute())
            }
            Kind::Invalid => panic!("should not have Kind::Invalid nodes in the recompute heap"),
            Kind::Uninitialised => panic!("recomputing uninitialised node"),
        }
    }
    /* Note that the two following functions are not symmetric of one another: in [let y =
    map x], [x] is always a child of [y] (assuming [x] doesn't become invalid) but [y] in
    only a parent of [x] if y is necessary. */
    // let assert_currently_running_node_is_child state node name =
    //   let (T current) = currently_running_node_exn state name in
    //   if not (Node.has_child node ~child:current)
    //   then
    //     raise_s
    //       [%sexp
    //         ("can only call " ^ name ^ " on parent nodes" : string)
    //       , ~~(node.kind : _ Kind.t)
    //       , ~~(current.kind : _ Kind.t)]
    // ;;
    //
    // let assert_currently_running_node_is_parent state node name =
    //   let (T current) = currently_running_node_exn state name in
    //   if not (Node.has_parent ~parent:current node)
    //   then
    //     raise_s
    //       [%sexp
    //         ("can only call " ^ name ^ " on children nodes" : string)
    //       , ~~(node.kind : _ Kind.t)
    //       , ~~(current.kind : _ Kind.t)]
    // ;;
    #[cfg(debug_assertions)]
    fn assert_currently_running_node_is_child(&self, name: &'static str) {
        let t = self.state();
        let current = t.only_in_debug.currently_running_node_exn(name);
        assert!(
            self.has_child(&current),
            "({name}) currently running node was not a child"
        );
    }
    #[cfg(debug_assertions)]
    fn assert_currently_running_node_is_parent(&self, name: &'static str) {
        let t = self.state();
        let current = t.only_in_debug.currently_running_node_exn(name);
        let Some(current) = current.upgrade() else { return };
        assert!(
            current.has_child(&self.weak()),
            "({name}) currently running node was not a parent"
        );
    }
    fn has_child(&self, child: &WeakNode<'a>) -> bool {
        let Some(upgraded) = child.upgrade() else { return false };
        let mut any = false;
        self.foreach_child(&mut |_ix, child| {
            any = any || Rc::ptr_eq(&child, &upgraded);
        });
        any
    }
    fn invalidate(&self) {
        let state = self.state();
        #[cfg(debug_assertions)]
        self.assert_currently_running_node_is_child("invalidate");
        self.invalidate_node();
        state.propagate_invalidity();
    }
    fn invalidate_node(&self) {
        if self.is_valid() {
            let t = self.state();
            self.maybe_handle_after_stabilisation();
            *self.value_opt.borrow_mut() = None;
            debug_assert!(self.old_value_opt.borrow().is_none());
            self.changed_at.set(t.stabilisation_num.get());
            self.recomputed_at.set(t.stabilisation_num.get());
            t.num_nodes_invalidated
                .set(t.num_nodes_invalidated.get() + 1);
            if self.is_necessary() {
                self.remove_children();
                /* The self doesn't have children anymore, so we can lower its height as much as
                possible, to one greater than the scope it was created in.  Also, because we
                are lowering the height, we don't need to adjust any of its ancestors' heights.
                We could leave the height alone, but we may as well lower it as much as
                possible to avoid making the heights of any future ancestors unnecessarily
                large. */
                let h = self.created_in.height() + 1;
                t.set_height(self.packed(), h);
                /* We don't set [node.created_in] or [node.next_node_in_same_scope]; we leave [node]
                in the scope it was created in.  If that scope is ever invalidated, then that
                will clear [node.next_node_in_same_scope] */
            }
            // (match node.kind with
            //  | At at -> remove_alarm at.clock at.alarm
            //  | At_intervals at_intervals -> remove_alarm at_intervals.clock at_intervals.alarm
            //  | Bind_main bind -> invalidate_nodes_created_on_rhs bind.all_nodes_created_on_rhs
            //  | Step_function { alarm; clock; _ } -> remove_alarm clock alarm
            //  | _ -> ());
            let mut kind = self.kind.borrow_mut();
            match &*kind {
                Kind::BindMain(_, bind) => {
                    let mut all = bind.all_nodes_created_on_rhs.borrow_mut();
                    invalidate_nodes_created_on_rhs(&mut all);
                }
                _ => {}
            }
            *kind = Kind::Invalid;
        }
    }

    fn adjust_heights_bind_lhs_change(
        &self,
        ahh: &mut AdjustHeightsHeap<'a>,
        oc: &NodeRef<'a>,
        op: &NodeRef<'a>,
    ) {
        let kind = self.kind.borrow();
        match &*kind {
            Kind::BindLhsChange(_, bind) => {
                let Some(bind) = bind.upgrade() else { return };
                let span = tracing::debug_span!("adjust_heights_bind_lhs_change");
                span.in_scope(|| {
                    let all = bind.all_nodes_created_on_rhs.borrow();
                    for rnode_weak in all.iter() {
                        let rnode = rnode_weak.upgrade().unwrap();
                        tracing::debug!("all_nodes_created_on_rhs: {:?}", rnode);
                        if rnode.is_necessary() {
                            ahh.ensure_height_requirement(oc, op, &self.packed(), &rnode)
                        }
                    }
                })
            }
            _ => {}
        }
    }

    fn created_in(&self) -> Scope<'a> {
        self.created_in.clone()
    }
    fn run_on_update_handlers(&self, node_update: NodeUpdateDelayed, now: StabilisationNum) {
        let input = self.as_input();
        let observers = self.observers.borrow();
        for (_id, obs) in observers.iter() {
            let Some(obs) = obs.upgrade() else { continue };
            obs.run_all(&input, node_update, now)
        }
    }
    fn maybe_handle_after_stabilisation(&self) {
        if self.num_on_update_handlers.get() > 0 {
            self.handle_after_stabilisation();
        }
    }

    fn handle_after_stabilisation(&self) {
        let is_in_stack = &self.is_in_handle_after_stabilisation;
        if !is_in_stack.get() {
            let t = self.state();
            is_in_stack.set(true);
            let mut stack = t.handle_after_stabilisation.borrow_mut();
            stack.push(self.weak());
        }
    }

    fn num_on_update_handlers(&self) -> &Cell<i32> {
        &self.num_on_update_handlers
    }

    fn node_update(&self) -> NodeUpdateDelayed {
        if !self.is_valid() {
            NodeUpdateDelayed::Invalidated
        } else if !self.is_necessary() {
            NodeUpdateDelayed::Unnecessary
        } else {
            let val = self.value_as_ref();
            match val {
                Some(_) => NodeUpdateDelayed::Changed,
                None => NodeUpdateDelayed::Necessary,
            }
        }
    }
}

fn invalidate_nodes_created_on_rhs(all_nodes_created_on_rhs: &mut Vec<WeakNode<'_>>) {
    for node in all_nodes_created_on_rhs.drain(..) {
        if let Some(node) = node.upgrade() {
            node.invalidate_node();
        }
    }
}

impl<'a, G: NodeGenerics<'a>> Node<'a, G> {
    pub fn into_rc(mut self) -> Rc<Self> {
        let rc = Rc::<Self>::new_cyclic(|weak| {
            self.weak_self = weak.clone();
            self
        });
        rc.created_in.add_node(rc.clone());
        rc
    }

    pub fn create(state: Weak<State<'a>>, created_in: Scope<'a>, kind: Kind<'a, G>) -> Rc<Self> {
        let t = state.upgrade().unwrap();
        t.num_nodes_created.set(t.num_nodes_created.get() + 1);
        Node {
            id: NodeId::next(),
            weak_self: Weak::<Self>::new(),
            inner: RefCell::new(NodeInner {
                state,
                my_parent_index_in_child_at_index: Vec::with_capacity(kind.initial_num_children()),
                my_child_index_in_parent_at_index: vec![-1],
                force_necessary: false,
            }),
            created_in,
            changed_at: Cell::new(StabilisationNum::init()),
            height: Cell::new(-1),
            old_height: Cell::new(-1),
            height_in_recompute_heap: Cell::new(-1),
            height_in_adjust_heights_heap: Cell::new(-1),
            is_in_handle_after_stabilisation: false.into(),
            num_on_update_handlers: 0.into(),
            recomputed_at: Cell::new(StabilisationNum::init()),
            value_opt: RefCell::new(None),
            old_value_opt: RefCell::new(None),
            kind: RefCell::new(kind),
            parents: vec![].into(),
            observers: HashMap::new().into(),
            cutoff: Cutoff::PartialEq.into(),
        }
        .into_rc()
    }

    fn maybe_change_value(&self, value: G::R) {
        let old_value_opt = self.value_opt.take();
        let cutoff = self.cutoff.get();
        let should_change = old_value_opt.is_none()
            || !cutoff.should_cutoff(old_value_opt.as_ref().unwrap(), &value);
        self.maybe_change_value_manual(old_value_opt, value, should_change)
    }

    fn maybe_change_value_manual(
        &self,
        old_value_opt: Option<G::R>,
        value: G::R,
        should_change: bool,
    ) {
        if should_change {
            let inner = self.inner.borrow();
            let istate = inner.state.upgrade().unwrap();
            self.value_opt.replace(Some(value));
            self.changed_at.set(istate.stabilisation_num.get());
            istate
                .num_nodes_changed
                .set(istate.num_nodes_changed.get() + 1);
            self.maybe_handle_after_stabilisation();
            let parents = self.parents.borrow();
            for (parent_index, parent) in parents.iter().enumerate() {
                let Some(p) = parent.upgrade_erased() else { continue };
                let i = self.inner().borrow();
                let child_index = i.my_child_index_in_parent_at_index[parent_index];
                {
                    parent.child_changed(&self.as_child(), child_index, old_value_opt.clone());
                }
                debug_assert!(p.needs_to_be_computed());
                if !p.is_in_recompute_heap() {
                    tracing::debug!(
                        "inserting parent into recompute heap at height {:?}",
                        p.height()
                    );
                    let t = self.state();
                    let mut rch = t.recompute_heap.borrow_mut();
                    rch.insert(p.packed());
                }
            }
        } else if old_value_opt.is_some() {
            tracing::info!("cutoff applied to value change");
            self.value_opt.replace(old_value_opt);
        }
    }

    fn change_child_bind_rhs(
        &self,
        old_child: Option<Incr<'a, G::BindRhs>>,
        new_child: Incr<'a, G::BindRhs>,
        child_index: i32,
    ) {
        let bind_main = self;
        let k = bind_main.kind.borrow();
        let id = match &*k {
            Kind::BindMain(id, _) => id,
            _ => return,
        };
        let new_child_node = id.input_rhs_i1.cast_ref(&new_child.node);
        match old_child {
            None => {
                tracing::debug!(
                    "change_child simply adding parent to {:?} at child_index {child_index}",
                    new_child_node
                );
                new_child_node.state_add_parent(child_index, bind_main.as_parent());
            }
            Some(old_child) => {
                // ptr_eq is better than ID checking -- no vtable call,
                if old_child.ptr_eq(&new_child) {
                    // nothing to do! nothing changed!
                    return;
                }
                let old_child_node = id.input_rhs_i1.cast_ref(&old_child.node);
                old_child_node.remove_parent(child_index, bind_main.as_parent());
                /* We force [old_child] to temporarily be necessary so that [add_parent] can't
                mistakenly think it is unnecessary and transition it to necessary (which would
                add duplicate edges and break things horribly). */
                let oc_inner = old_child_node.inner();
                let mut oci = oc_inner.borrow_mut();
                oci.force_necessary = true;
                {
                    new_child_node.state_add_parent(child_index, bind_main.as_parent());
                }
                oci.force_necessary = false;
                drop(oci);
                old_child_node.check_if_unnecessary();
            }
        }
    }

    fn copy_child_bindrhs(&self, child: &Input<G::BindRhs>, token: Id<G::BindRhs, G::R>) {
        if child.is_valid() {
            let latest = child.latest();
            self.maybe_change_value(token.cast(latest));
        } else {
            self.invalidate();
            self.state().propagate_invalidity();
        }
    }

    fn add_parent(&self, child_index: i32, parent_weak: ParentRef<'a, G::R>) {
        let child = self;
        let child_inner = self.inner();
        let mut ci = child_inner.borrow_mut();
        let mut child_parents = child.parents.borrow_mut();

        // we're appending here
        let parent = parent_weak.upgrade_erased().unwrap();
        let parent_index = child_parents.len() as i32;
        let parent_inner = parent.inner();
        let mut parent_i = parent_inner.borrow_mut();

        while ci.my_child_index_in_parent_at_index.len() <= parent_index as usize {
            ci.my_child_index_in_parent_at_index.push(-1);
        }
        ci.my_child_index_in_parent_at_index[parent_index as usize] = child_index;

        while parent_i.my_parent_index_in_child_at_index.len() <= child_index as usize {
            parent_i.my_parent_index_in_child_at_index.push(-1);
        }
        parent_i.my_parent_index_in_child_at_index[child_index as usize] = parent_index;

        child_parents.push(parent_weak);
    }

    fn as_parent(&self) -> ParentRef<'a, G::I1> {
        ParentRef::I1(self.weak_self.clone())
    }
    fn as_parent2(&self) -> ParentRef<'a, G::I2> {
        ParentRef::I2(self.weak_self.clone())
    }
    fn as_child(&self) -> Input<'a, G::R> {
        self.weak_self.clone().upgrade().unwrap()
    }
    fn foreach_child_typed<'b>(&'b self, f: ForeachChild<'a, 'b, G>) {
        let k = self.kind.borrow();
        match &*k {
            Kind::Invalid => {}
            Kind::Uninitialised => {}
            Kind::Constant(_) => {}
            Kind::Map(MapNode { input, .. }) => (f.i1)(0, input.clone().as_input()),
            Kind::MapWithOld(MapWithOld { input, .. }) => (f.i1)(0, input.clone().as_input()),
            Kind::Map2(Map2Node { one, two, .. }) => {
                (f.i1)(0, one.clone().as_input());
                (f.i2)(1, two.clone().as_input());
            }
            Kind::BindLhsChange(id, bind) => {
                let Some(bind) = bind.upgrade() else { return };
                let input = id.input_lhs_i2.cast(bind.lhs.as_input());
                (f.i2)(0, input)
            }
            Kind::BindMain(id, bind) => {
                let input = id.input_lhs_i2.cast(bind.lhs_change.as_input());
                (f.i2)(0, input);
                if let Some(rhs) = bind.rhs.borrow().as_ref() {
                    let input = id.input_rhs_i1.cast(rhs.node.as_input());
                    (f.i1)(1, input)
                }
            }
            Kind::ArrayFold(af) => {
                for (ix, child) in af.children.iter().enumerate() {
                    (f.i1)(ix as i32, child.node.as_input())
                }
            }
            Kind::UnorderedArrayFold(uaf) => {
                for (ix, child) in uaf.children.iter().enumerate() {
                    (f.i1)(ix as i32, child.node.as_input());
                }
            }
            Kind::Var(var) => {}
        }
    }

    fn remove_children(&self) {
        self.foreach_child_typed(ForeachChild {
            i1: &mut |index, child| {
                child.remove_parent(index, self.as_parent());
                child.check_if_unnecessary();
            },
            i2: &mut |index, child| {
                child.remove_parent(index, self.as_parent2());
                child.check_if_unnecessary();
            },
        })
    }
}

struct ForeachChild<'a: 'b, 'b, G: NodeGenerics<'a>> {
    i1: &'b mut dyn FnMut(i32, Input<'a, G::I1>),
    i2: &'b mut dyn FnMut(i32, Input<'a, G::I2>),
}

#[test]
fn test_node_size() {
    use super::kind::Constant;
    let state = State::new();
    let node =
        Node::<Constant<i32>>::create(state.weak(), state.current_scope(), Kind::Constant(5i32));
    assert_eq!(core::mem::size_of_val(&*node), 376);
}

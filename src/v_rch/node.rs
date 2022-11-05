use crate::v_rch::MapWithOld;
use crate::Value;

use super::adjust_heights_heap::AdjustHeightsHeap;
use super::cutoff::Cutoff;
use super::internal_observer::{InternalObserver, ObserverId};
use super::kind::{Kind, NodeGenerics};
use super::node_update::NodeUpdateDelayed;
use super::scope::Scope;
use super::state::State;
use super::{Incr, Map2Node, MapNode, NodeRef, WeakNode, MapRefNode};
use core::fmt::Debug;
use std::cell::Ref;
use std::collections::{HashMap, HashSet};
use std::fmt::{self, Display, Write};
use std::rc::Weak;

use super::stabilisation_num::StabilisationNum;
use refl::Id;
use smallvec::{smallvec, SmallVec};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct NodeId(pub(crate) usize);
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
impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "id={}", self.0)
    }
}

pub(crate) struct Node<G: NodeGenerics> {
    pub id: NodeId,

    /// A handy reference to ourself, which we can use to generate Weak<dyn Trait> versions of our
    /// node's reference-counted pointer at any time.
    weak_self: Weak<Self>,

    /// A reference to the incremental State we're part of.
    pub state: Weak<State>,

    /* The fields from [recomputed_at] to [created_in] are grouped together and are in the
    same order as they are used by [State.recompute] This has a positive performance
    impact due to cache effects.  Don't change the order of these nodes without
    performance testing. */
    /// The time at which we were last recomputed. -1 if never.
    pub recomputed_at: Cell<StabilisationNum>,
    pub value_opt: RefCell<Option<G::R>>,
    pub is_valid: Cell<bool>,
    pub kind: Kind<G>,
    /// The cutoff function. This determines whether we set `changed_at = recomputed_at` during
    /// recomputation, which in turn helps determine if our parents are stale & need recomputing
    /// themselves.
    pub cutoff: Cell<Cutoff<G::R>>,
    /// The time at which our value changed. -1 if never.
    ///
    /// Set to self.recomputed_at when the node's value changes. (Cutoff = don't set changed_at).
    pub changed_at: Cell<StabilisationNum>,
    /// Used during stabilisation to decide whether to handle_after_stabilisation at all (if > 0)
    pub num_on_update_handlers: Cell<i32>,
    /// ParentRef knows which type it will receive. We know all our node's parents are going to have an
    /// input of *our* G::R, so all our parents will have provided us with a ParentRef that
    /// receives G::R.
    ///
    /// Most of the time, a node only has one parent. So we will use a SmallVec with space enough
    /// for one without hitting the allocator.
    pub parents: RefCell<SmallVec<[ParentRef<G::R>; 1]>>,
    /// Scope the node was created in. Never modified.
    pub created_in: Scope,

    pub parent_child_indices: RefCell<ParentChildIndices>,
    pub old_value_opt: RefCell<Option<G::R>>,
    pub height: Cell<i32>,
    /// Set from RecomputeHeap, and AdjustHeightsHeap via increase_height
    pub height_in_recompute_heap: Cell<i32>,
    /// Used to avoid inserting into AHH twice.
    /// -1 when not in AHH.
    pub height_in_adjust_heights_heap: Cell<i32>,
    /// Used during stabilisation to add self to handle_after_stabilisation only once per stabilise cycle.
    pub is_in_handle_after_stabilisation: Cell<bool>,
    pub force_necessary: Cell<bool>,

    /// A node knows its own observers. This way, in order to schedule a notification at the end
    /// of stabilisation, all you need to do is add the node to a queue.
    pub observers: RefCell<HashMap<ObserverId, Weak<InternalObserver<G::R>>>>,
}

#[derive(Debug)]
pub(crate) struct ParentChildIndices {
    pub my_parent_index_in_child_at_index: SmallVec<[i32; 2]>,
    pub my_child_index_in_parent_at_index: SmallVec<[i32; 1]>,
}

pub(crate) type Input<R> = Rc<dyn Incremental<R>>;

pub(crate) trait Incremental<R>: ErasedNode + Debug {
    fn as_input(&self) -> Input<R>;
    #[deprecated = "doesn't work with Kind::MapRef"]
    fn latest(&self) -> R;
    #[deprecated = "doesn't work with Kind::MapRef"]
    fn value_opt(&self) -> Option<R>;
    fn add_parent_without_adjusting_heights(&self, child_index: i32, parent_weak: ParentRef<R>);
    fn state_add_parent(&self, child_index: i32, parent_weak: ParentRef<R>);
    fn remove_parent(&self, child_index: i32, parent_weak: ParentRef<R>);
    fn set_cutoff(&self, cutoff: Cutoff<R>);
    fn value_as_ref(&self) -> Option<Ref<R>>;
    fn add_observer(&self, id: ObserverId, weak: Weak<InternalObserver<R>>);
    fn remove_observer(&self, id: ObserverId);
}

impl<G: NodeGenerics> Incremental<G::R> for Node<G> {
    fn as_input(&self) -> Input<G::R> {
        self.weak_self.upgrade().unwrap() as Input<G::R>
    }
    fn latest(&self) -> G::R {
        self.value_opt.borrow().clone().unwrap()
    }
    fn value_as_ref(&self) -> Option<Ref<G::R>> {
        match &self.kind {
            Kind::MapRef(mapref) => {
                let mapper = &mapref.mapper;
                let input = mapref.input.value_as_ref()?;
                let mapped = Ref::filter_map(input, |iref| Some(mapper(&iref))).ok();
                return mapped
            }
            _ => {}
        }
        let v = self.value_opt.borrow();
        Ref::filter_map(v, |o| o.as_ref()).ok()
    }
    fn value_opt(&self) -> Option<G::R> {
        self.value_opt.borrow().clone()
    }

    // #[tracing::instrument]
    fn add_parent_without_adjusting_heights(&self, child_index: i32, parent_weak: ParentRef<G::R>) {
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
    fn state_add_parent(&self, child_index: i32, parent_weak: ParentRef<G::R>) {
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
            let mut ah_heap = t.adjust_heights_heap.borrow_mut();
            let rch = &t.recompute_heap;
            ah_heap.adjust_heights(rch, self.packed(), parent.packed());
        }
        self.state().propagate_invalidity();
        /* we only add necessary parents */
        debug_assert!(parent.is_necessary());
        if !parent.is_in_recompute_heap()
            && (parent.recomputed_at().get().is_never() || self.edge_is_stale(parent.weak()))
        {
            let t = self.state();
            t.recompute_heap.insert(parent.packed());
        }
    }
    fn remove_parent(&self, child_index: i32, parent_weak: ParentRef<G::R>) {
        let child = self;
        let mut child_indices = child.parent_child_indices.borrow_mut();
        let mut child_parents = child.parents.borrow_mut();
        let parent = parent_weak.upgrade_erased().unwrap();
        let parent_indices_cell = parent.parent_child_indices();
        let mut parent_indices = parent_indices_cell.borrow_mut();

        debug_assert!(child_parents.len() >= 1);
        let parent_index = parent_indices.my_parent_index_in_child_at_index[child_index as usize];
        debug_assert!(parent_weak.ptr_eq(&child_parents[parent_index as usize].clone()));
        let last_parent_index = child_parents.len() - 1;
        if (parent_index as usize) < last_parent_index {
            // we swap the parent the end of the array into this one's position. This keeps the array
            // small.
            let end_p_weak = child_parents[last_parent_index].clone();
            if let Some(end_p) = end_p_weak.upgrade_erased() {
                let end_p_indices_cell = end_p.parent_child_indices();
                let mut end_p_indices = end_p_indices_cell.borrow_mut();
                let end_child_index =
                    child_indices.my_child_index_in_parent_at_index[last_parent_index];
                // link parent_index & end_child_index
                end_p_indices.my_parent_index_in_child_at_index[end_child_index as usize] =
                    parent_index;
                child_indices.my_child_index_in_parent_at_index[parent_index as usize] =
                    end_child_index;
            }
        }
        // unlink last_parent_index & child_index
        parent_indices.my_parent_index_in_child_at_index[child_index as usize] = -1;
        child_indices.my_child_index_in_parent_at_index[last_parent_index] = -1;

        // now do what we just did but super easily in the actual Vec
        child_parents.swap_remove(parent_index as usize);
    }
    fn set_cutoff(&self, cutoff: Cutoff<G::R>) {
        self.cutoff.set(cutoff)
    }
    fn add_observer(&self, id: ObserverId, weak: Weak<InternalObserver<G::R>>) {
        let mut os = self.observers.borrow_mut();
        os.insert(id, weak);
    }
    fn remove_observer(&self, id: ObserverId) {
        let mut os = self.observers.borrow_mut();
        os.remove(&id);
    }
}

pub(crate) trait ParentNode<I> {
    fn child_changed(&self, child: &Input<I>, child_index: i32, old_value_opt: Option<&I>);
}
pub(crate) trait Parent1<I>: Debug + ErasedNode {
    fn child_changed(&self, child: &Input<I>, child_index: i32, old_value_opt: Option<&I>);
}
pub(crate) trait Parent2<I>: Debug + ErasedNode {}

/// Each node can be a parent for up to a dozen or so (map12) different types.
/// ParentRef is an enum that knows, by the tag, which kind of parent this node is behaving as,
/// and the various Parent1/Parent2 traits embed that static, type-level information in their
/// implementation. So all a *child* node has to do is pass through its G::R, and the parent
/// will make sure it passes the correct kind of ParentRef that has an implementation for that
/// type.
///
/// Ultimately, we only need one type-aware method from these parent nodes. That's
/// `child_changed`. And we only need for UnorderedArrayFold & eventually Expert. Lot of work
/// for one method. But there you go.
#[derive(Clone, Debug)]
pub(crate) enum ParentRef<T> {
    I1(Weak<dyn Parent1<T>>),
    I2(Weak<dyn Parent2<T>>),
}

impl<T: Value> ParentRef<T> {
    fn upgrade_erased(&self) -> Option<NodeRef> {
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

impl<T: Value> ParentNode<T> for ParentRef<T> {
    fn child_changed(&self, child: &Input<T>, child_index: i32, old_value_opt: Option<&T>) {
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
impl<G: NodeGenerics> Parent2<G::I2> for Node<G> {}
impl<G: NodeGenerics> Parent1<G::I1> for Node<G> {
    fn child_changed(&self, child: &Input<G::I1>, child_index: i32, old_value_opt: Option<&G::I1>) {
        match &self.kind {
            // Kind::Expert => ...,
            Kind::UnorderedArrayFold(uaf) => {
                let new_value = child.value_as_ref().unwrap();
                uaf.child_changed(child, child_index, old_value_opt, &new_value)
            }
            Kind::MapRef(mapref) => {
                let self_old = old_value_opt.map(|v| (mapref.mapper)(v));
                let child_new = child.value_as_ref().unwrap();
                let self_new = (mapref.mapper)(&child_new);

                let did_change =
                    self_old.is_none() || self.cutoff.get().should_cutoff(self_old.unwrap(), self_new);
                // now we propagate to parent
                // we can do this because we know we don't have to wait for "all our children"
                // i.e. we know there is only one that could have fired child_change on us in the
                // first place.
                self.maybe_change_value_manual(self_old, None, did_change)
            }
            // TODO: same for MapWithOld. While we still have access to the old value?
            _ => {}
        }
    }
}

pub(crate) trait ErasedNode: Debug {
    fn id(&self) -> NodeId;
    fn is_valid(&self) -> bool;
    fn dot_label(&self, f: &mut dyn Write) -> fmt::Result;
    fn dot_write(&self, f: &mut dyn Write) -> fmt::Result;
    fn dot_add_bind_edges(&self, bind_edges: &mut Vec<(NodeRef, NodeRef)>);
    fn should_be_invalidated(&self) -> bool;
    fn propagate_invalidity_helper(&self);
    fn has_invalid_child(&self) -> bool;
    fn height_in_recompute_heap(&self) -> &Cell<i32>;
    fn height_in_adjust_heights_heap(&self) -> &Cell<i32>;
    fn is_in_handle_after_stabilisation(&self) -> &Cell<bool>;

    /// AdjustHeightsHeap::ensure_height_requirements
    fn ensure_parent_height_requirements(
        &self,
        ahh: &mut AdjustHeightsHeap,
        original_child: &NodeRef,
        original_parent: &NodeRef,
    );
    fn height(&self) -> i32;
    /// Only for use from AdjustHeightsHeap.
    fn set_height(&self, height: i32);
    fn is_stale(&self) -> bool;
    fn is_stale_with_respect_to_a_child(&self) -> bool;
    fn edge_is_stale(&self, parent: WeakNode) -> bool;
    fn is_necessary(&self) -> bool;
    fn force_necessary(&self) -> &Cell<bool>;
    fn needs_to_be_computed(&self) -> bool;
    fn became_necessary(&self);
    fn became_necessary_propagate(&self);
    fn became_unnecessary(&self);
    fn check_if_unnecessary(&self);
    fn is_in_recompute_heap(&self) -> bool;
    fn recompute(&self);
    fn parent_child_indices(&self) -> &RefCell<ParentChildIndices>;
    fn state_opt(&self) -> Option<Rc<State>>;
    fn state(&self) -> Rc<State>;
    fn weak(&self) -> WeakNode;
    fn packed(&self) -> NodeRef;
    fn foreach_child(&self, f: &mut dyn FnMut(i32, NodeRef) -> ());
    fn iter_descendants(&self, f: &mut dyn FnMut(&NodeRef));
    fn iter_descendants_internal(&self, seen: &mut HashSet<NodeId>, f: &mut dyn FnMut(&NodeRef));
    fn recomputed_at(&self) -> &Cell<StabilisationNum>;
    fn invalidate(&self);
    fn invalidate_node(&self);
    #[cfg(debug_assertions)]
    fn assert_currently_running_node_is_child(&self, name: &'static str);
    #[cfg(debug_assertions)]
    fn assert_currently_running_node_is_parent(&self, name: &'static str);
    fn has_child(&self, child: &WeakNode) -> bool;
    fn adjust_heights_bind_lhs_change(
        &self,
        ahh: &mut AdjustHeightsHeap,
        oc: &NodeRef,
        op: &NodeRef,
    );
    fn created_in(&self) -> Scope;
    fn run_on_update_handlers(&self, node_update: NodeUpdateDelayed, now: StabilisationNum);
    fn maybe_handle_after_stabilisation(&self);
    fn handle_after_stabilisation(&self);
    fn num_on_update_handlers(&self) -> &Cell<i32>;
    fn node_update(&self) -> NodeUpdateDelayed;
}

impl<G: NodeGenerics> Debug for Node<G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Node")
            .field("id", &self.id)
            .field("kind", &self.kind)
            // .field("height", &self.height.get())
            .finish()
    }
}

impl<G: NodeGenerics> ErasedNode for Node<G> {
    fn id(&self) -> NodeId {
        self.id
    }
    fn weak(&self) -> WeakNode {
        self.weak_self.clone() as WeakNode
    }
    fn packed(&self) -> NodeRef {
        self.weak_self.upgrade().unwrap() as NodeRef
    }
    fn parent_child_indices(&self) -> &RefCell<ParentChildIndices> {
        &self.parent_child_indices
    }
    fn state_opt(&self) -> Option<Rc<State>> {
        self.state.upgrade()
    }
    fn state(&self) -> Rc<State> {
        self.state.upgrade().unwrap()
    }
    fn is_valid(&self) -> bool {
        self.is_valid.get()
    }
    fn should_be_invalidated(&self) -> bool {
        match &self.kind {
            Kind::Constant(_) | Kind::Var(_) => false,
            Kind::ArrayFold(..)
            | Kind::UnorderedArrayFold(..)
            | Kind::Map(..)
            | Kind::MapWithOld(..)
            | Kind::MapRef(..)
            | Kind::Map2(..) => self.has_invalid_child(),
            /* A *_change node is invalid if the node it is watching for changes is invalid (same
            reason as above).  This is equivalent to [has_invalid_child t]. */
            Kind::BindLhsChange(_, bind) => {
                !bind.lhs.is_valid()
            }
            /* [Bind_main], [If_then_else], and [Join_main] are invalid if their *_change child is,
            but not necessarily if their other children are -- the graph may be restructured to
            avoid the invalidity of those. */
            Kind::BindMain(_, _, lhs_change) => !lhs_change.is_valid(),
        }
    }
    fn propagate_invalidity_helper(&self) {
        match &self.kind {
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
        ahh: &mut AdjustHeightsHeap,
        original_child: &NodeRef,
        original_parent: &NodeRef,
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
    fn is_stale(&self) -> bool {
        match &self.kind {
            Kind::Var(var) => {
                let set_at = var.set_at.get();
                let recomputed_at = self.recomputed_at.get();
                set_at > recomputed_at
            }
            Kind::Constant(v) => self.recomputed_at.get() == StabilisationNum(-1),
            Kind::ArrayFold(_)
            | Kind::UnorderedArrayFold(_)
            | Kind::Map(_)
            // for MapRef, we never "recompute" ourselves. the action happens in
            // child_changed/value_as_ref
            | Kind::MapRef(_)
            | Kind::MapWithOld(_)
            | Kind::Map2(_)
            | Kind::BindLhsChange(..)
            | Kind::BindMain(..) => {
                // i.e. never recomputed, or a child has changed more recently than we have been
                // recomputed
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
    fn edge_is_stale(&self, parent: WeakNode) -> bool {
        let Some(parent) = parent.upgrade() else { return false };
        self.changed_at.get() > parent.recomputed_at().get()
    }
    fn is_necessary(&self) -> bool {
        !self.parents.borrow().is_empty()
            || !self.observers.borrow().is_empty()
            // || kind is freeze
            || self.force_necessary.get()
    }
    fn needs_to_be_computed(&self) -> bool {
        self.is_necessary() && self.is_stale()
    }
    fn force_necessary(&self) -> &Cell<bool> {
        &self.force_necessary
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
            t.recompute_heap.insert(self.packed());
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
        match &self.kind {
            Kind::UnorderedArrayFold(uaf) => uaf.force_full_compute(),
            _ => {}
        }
        debug_assert!(!self.needs_to_be_computed());
        if self.is_in_recompute_heap() {
            let t = self.state();
            t.recompute_heap.remove(self.packed());
        }
    }
    fn foreach_child(&self, f: &mut dyn FnMut(i32, NodeRef) -> ()) {
        match &self.kind {
            Kind::Constant(_) => {}
            Kind::Map(MapNode { input, ..})
            | Kind::MapRef(MapRefNode { input, .. })
            | Kind::MapWithOld(MapWithOld { input, .. }) => f(0, input.packed()),
            Kind::Map2(Map2Node { one, two, .. }) => {
                f(0, one.clone().packed());
                f(1, two.clone().packed());
            }
            Kind::BindLhsChange(_, bind) => {
                f(0, bind.lhs.packed())
            }
            Kind::BindMain(_, bind, lhs_change) => {
                f(0, lhs_change.packed());
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
        let id = self.id;
        let height = self.height();
        match &self.kind {
            Kind::Constant(v) => {
                self.maybe_change_value(v.clone());
            }
            Kind::Var(var) => {
                let new_value = {
                    let value = var.value.borrow();
                    value.clone()
                };
                tracing::debug!("recomputing Var({id:?}) <- {new_value:?}");
                self.maybe_change_value(new_value);
            }
            Kind::Map(map) => {
                let map: &MapNode<G::F1, G::I1, G::R> = map;
                let new_value = {
                    let mut f = map.mapper.borrow_mut();
                    let input = map.input.value_as_ref().unwrap();
                    f(&input)
                };
                tracing::debug!("<- {new_value:?}");
                self.maybe_change_value(new_value);
            }
            Kind::MapRef(MapRefNode { input, mapper, did_change }) => {
                // do nothing! all the action happens in child_changed and value_as_ref
            }
            Kind::MapWithOld(map) => {
                let map: &MapWithOld<G::WithOld, G::I1, G::R> = map;
                let input = map.input.value_as_ref().unwrap();
                let mut f = map.mapper.borrow_mut();
                let old_value = self.value_opt.take();
                let old_value_is_none = old_value.is_none();
                let (new_value, did_change) = f(old_value, &input);
                tracing::debug!("<- {new_value:?}");
                // WARN: None for old_value_opt is wrong
                self.maybe_change_value_manual(None, Some(new_value), old_value_is_none || did_change);
            }
            Kind::Map2(map2) => {
                let map2: &Map2Node<G::F2, G::I1, G::I2, G::R> = map2;
                let i1 = map2.one.value_as_ref().unwrap();
                let i2 = map2.two.value_as_ref().unwrap();
                let mut f = map2.mapper.borrow_mut();
                let new_value = f(&i1, &i2);
                tracing::debug!("recomputing Map2(id={id:?}) <- {new_value:?}");
                self.maybe_change_value(new_value);
            }
            Kind::BindLhsChange(casts, bind) => {
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
                let main_ = bind.main.borrow();
                if let Some(main) = main_.upgrade() {
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
                self.maybe_change_value(casts.r_unit.cast(()));
            }
            Kind::BindMain(casts, bind, _) => {
                let rhs = bind.rhs.borrow().as_ref().unwrap().clone();
                tracing::debug!("-- recomputing BindMain(id={id:?}, h={height:?}) <- {rhs:?}");
                self.copy_child_bindrhs(&rhs.node, casts.rhs_r);
            }
            Kind::ArrayFold(af) => self.maybe_change_value(af.compute()),
            Kind::UnorderedArrayFold(uaf) => {
                tracing::debug!("-- recomputing UAF {uaf:?}");
                self.maybe_change_value(uaf.compute())
            }
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
    fn has_child(&self, child: &WeakNode) -> bool {
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
            match &self.kind {
                Kind::BindMain(_, bind, _) => {
                    let mut all = bind.all_nodes_created_on_rhs.borrow_mut();
                    invalidate_nodes_created_on_rhs(&mut all);
                }
                _ => {}
            }
            self.is_valid.set(false)
        }
    }

    fn adjust_heights_bind_lhs_change(
        &self,
        ahh: &mut AdjustHeightsHeap,
        oc: &NodeRef,
        op: &NodeRef,
    ) {
        match &self.kind {
            Kind::BindLhsChange(_, bind) => {
                tracing::debug_span!("adjust_heights_bind_lhs_change").in_scope(|| {
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

    fn created_in(&self) -> Scope {
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
            match self.value_as_ref().is_some() {
                true => NodeUpdateDelayed::Changed,
                false => NodeUpdateDelayed::Necessary,
            }
        }
    }
    fn iter_descendants(&self, f: &mut dyn FnMut(&NodeRef)) {
        let mut seen = HashSet::new();
        self.iter_descendants_internal(&mut seen, f)
    }
    fn iter_descendants_internal(&self, seen: &mut HashSet<NodeId>, f: &mut dyn FnMut(&NodeRef)) {
        if !seen.contains(&self.id) {
            seen.insert(self.id);
            f(&self.packed());
            self.foreach_child(&mut |_ix, child| child.iter_descendants_internal(seen, f))
        }
    }
    fn dot_label(&self, f: &mut dyn Write) -> fmt::Result {
        let id = self.id;
        match &self.kind {
            Kind::Constant(v) => return write!(f, "Constant({id:?}, {v:?})"),
            Kind::ArrayFold(_) => write!(f, "ArrayFold"),
            Kind::UnorderedArrayFold(_) => write!(f, "UnorderedArrayFold"),
            Kind::Var(_) => write!(f, "Var"),
            Kind::Map(_) => write!(f, "Map"),
            Kind::MapRef(_) => write!(f, "MapRef"),
            Kind::MapWithOld(_) => write!(f, "MapWithOld"),
            Kind::Map2(_) => write!(f, "Map2"),
            Kind::BindLhsChange(_, _) => return write!(f, "BindLhsChange"),
            Kind::BindMain(..) => write!(f, "BindMain"),
        }?;
        write!(f, "({id:?})")?;
        if let Some(val) =  self.value_as_ref() {
            write!(f, " => {:?}", val)?;
        }
        Ok(())
    }

    fn dot_add_bind_edges(&self, bind_edges: &mut Vec<(NodeRef, NodeRef)>) {
        match &self.kind {
            Kind::BindLhsChange(_, bind) => {
                tracing::warn!("about to check bindlhschange.bind {:?}", bind);
                let all = bind.all_nodes_created_on_rhs.borrow();
                tracing::warn!("graphviz:all_nodes_created_on_rhs = {:?}", &*all);
                for rhs in all.iter().filter_map(Weak::upgrade) {
                    tracing::info!("bind created on RHS: {rhs:?}");
                    bind_edges.push((self.packed(), rhs.clone()));
                }
            }
            _ => {}
        }
    }
    fn dot_write(&self, f: &mut dyn Write) -> fmt::Result {
        fn node_name(node: &NodeRef) -> String {
            node.id().0.to_string()
        }
        fn print_node(f: &mut dyn Write, node: &NodeRef, name: &str) -> fmt::Result {
            write!(f, "  {} [", name)?;
            write!(f, "label=")?;
            let t = node.state();
            let r = t.stabilisation_num.get();
            let mut buf = String::new();
            node.dot_label(&mut buf)?;
            write!(f, "{:?}", buf)?;
            if node.recomputed_at().get().add1() < r {
                write!(f, ", color=grey, fontcolor=grey")?;
            }
            writeln!(f, "]")?;
            Ok(())
        }
        writeln!(f, "digraph G {{")?;
        writeln!(f, "  rankdir = BT")?;
        let mut bind_edges = vec![];
        let mut seen = HashSet::new();
        self.iter_descendants_internal(&mut seen, &mut |node| {
            tracing::warn!("graphviz:doing a node {:?}", node);
            let name = node_name(node);
            print_node(f, node, &name).unwrap();
            node.foreach_child(&mut |_, child| {
                writeln!(f, "  {} -> {}", node_name(&child.packed()), name).unwrap();
            });
            node.dot_add_bind_edges(&mut bind_edges);
        });
        for (bind, rhs) in bind_edges {
            if seen.contains(&rhs.id()) {
                writeln!(
                    f,
                    "  {} -> {} [style=dashed]",
                    node_name(&bind),
                    node_name(&rhs)
                )?;
            }
        }
        writeln!(f, "}}")?;
        Ok(())
    }
}

fn invalidate_nodes_created_on_rhs(all_nodes_created_on_rhs: &mut Vec<WeakNode>) {
    tracing::warn!("draining all_nodes_created_on_rhs for invalidation");
    for node in all_nodes_created_on_rhs.drain(..) {
        if let Some(node) = node.upgrade() {
            node.invalidate_node();
        }
    }
}

impl<G: NodeGenerics> Node<G> {
    pub fn into_rc(mut self) -> Rc<Self> {
        let rc = Rc::<Self>::new_cyclic(|weak| {
            self.weak_self = weak.clone();
            self
        });
        rc.created_in.add_node(rc.clone());
        rc
    }

    pub fn create(state: Weak<State>, created_in: Scope, kind: Kind<G>) -> Rc<Self> {
        let t = state.upgrade().unwrap();
        t.num_nodes_created.set(t.num_nodes_created.get() + 1);
        Node {
            id: NodeId::next(),
            weak_self: Weak::<Self>::new(),
            state,
            parent_child_indices: RefCell::new(ParentChildIndices {
                my_parent_index_in_child_at_index: SmallVec::with_capacity(
                    kind.initial_num_children(),
                ),
                my_child_index_in_parent_at_index: smallvec![-1],
            }),
            created_in,
            changed_at: Cell::new(StabilisationNum::init()),
            height: Cell::new(-1),
            height_in_recompute_heap: Cell::new(-1),
            height_in_adjust_heights_heap: Cell::new(-1),
            is_in_handle_after_stabilisation: false.into(),
            force_necessary: false.into(),
            num_on_update_handlers: 0.into(),
            recomputed_at: Cell::new(StabilisationNum::init()),
            value_opt: RefCell::new(None),
            old_value_opt: RefCell::new(None),
            kind,
            parents: smallvec::smallvec![].into(),
            observers: HashMap::new().into(),
            cutoff: Cutoff::PartialEq.into(),
            is_valid: true.into(),
        }
        .into_rc()
    }

    fn maybe_change_value(&self, value: G::R) {
        let old_value_opt = self.value_opt.take();
        let cutoff = self.cutoff.get();
        let should_change = old_value_opt.is_none()
            || !cutoff.should_cutoff(old_value_opt.as_ref().unwrap(), &value);
        self.maybe_change_value_manual(old_value_opt.as_ref(), Some(value), should_change)
    }

    fn maybe_change_value_manual(
        &self,
        old_value_opt: Option<&G::R>,
        value_opt: Option<G::R>,
        should_change: bool,
    ) {
        if should_change {
            let pci = self.parent_child_indices.borrow();
            let t = self.state();
            self.value_opt.replace(value_opt);
            self.changed_at.set(t.stabilisation_num.get());
            t.num_nodes_changed.set(t.num_nodes_changed.get() + 1);
            self.maybe_handle_after_stabilisation();
            let parents = self.parents.borrow();
            for (parent_index, parent) in parents.iter().enumerate() {
                let Some(p) = parent.upgrade_erased() else { continue };
                let child_index = pci.my_child_index_in_parent_at_index[parent_index];
                {
                    parent.child_changed(&self.as_child(), child_index, old_value_opt);
                }
                debug_assert!(p.needs_to_be_computed());
                if !p.is_in_recompute_heap() {
                    tracing::debug!(
                        "inserting parent into recompute heap at height {:?}",
                        p.height()
                    );
                    let t = self.state();
                    t.recompute_heap.insert(p.packed());
                }
            }
        } else if old_value_opt.is_some() {
            tracing::info!("cutoff applied to value change");
            self.value_opt.replace(old_value_opt.cloned());
        }
    }

    fn change_child_bind_rhs(
        &self,
        old_child: Option<Incr<G::BindRhs>>,
        new_child: Incr<G::BindRhs>,
        child_index: i32,
    ) {
        let bind_main = self;
        let id = match &bind_main.kind {
            Kind::BindMain(id, ..) => id,
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
                old_child_node.force_necessary().set(true);
                {
                    new_child_node.state_add_parent(child_index, bind_main.as_parent());
                }
                old_child_node.force_necessary().set(false);
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

    fn add_parent(&self, child_index: i32, parent_weak: ParentRef<G::R>) {
        let child = self;
        let mut child_indices = child.parent_child_indices.borrow_mut();
        let mut child_parents = child.parents.borrow_mut();

        // we're appending here
        let parent = parent_weak.upgrade_erased().unwrap();
        let parent_index = child_parents.len() as i32;
        let parent_indices_cell = parent.parent_child_indices();
        let mut parent_indices = parent_indices_cell.borrow_mut();

        while child_indices.my_child_index_in_parent_at_index.len() <= parent_index as usize {
            child_indices.my_child_index_in_parent_at_index.push(-1);
        }
        child_indices.my_child_index_in_parent_at_index[parent_index as usize] = child_index;

        while parent_indices.my_parent_index_in_child_at_index.len() <= child_index as usize {
            parent_indices.my_parent_index_in_child_at_index.push(-1);
        }
        parent_indices.my_parent_index_in_child_at_index[child_index as usize] = parent_index;

        child_parents.push(parent_weak);
    }

    fn as_parent(&self) -> ParentRef<G::I1> {
        ParentRef::I1(self.weak_self.clone())
    }
    fn as_parent2(&self) -> ParentRef<G::I2> {
        ParentRef::I2(self.weak_self.clone())
    }
    fn as_child(&self) -> Input<G::R> {
        self.weak_self.clone().upgrade().unwrap()
    }
    fn foreach_child_typed<'b>(&'b self, f: ForeachChild<'b, G>) {
        match &self.kind {
            Kind::Constant(_) => {}
            Kind::Map(MapNode { input, ..})
            | Kind::MapRef(MapRefNode { input, .. })
            | Kind::MapWithOld(MapWithOld { input, .. }) => (f.i1)(0, input.clone().as_input()),
            Kind::Map2(Map2Node { one, two, .. }) => {
                (f.i1)(0, one.clone().as_input());
                (f.i2)(1, two.clone().as_input());
            }
            Kind::BindLhsChange(id, bind) => {
                let input = id.input_lhs_i2.cast(bind.lhs.as_input());
                (f.i2)(0, input)
            }
            Kind::BindMain(id, bind, lhs_change) => {
                let input = id.input_lhs_i2.cast(lhs_change.as_input());
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

struct ForeachChild<'a, G: NodeGenerics> {
    i1: &'a mut dyn FnMut(i32, Input<G::I1>),
    i2: &'a mut dyn FnMut(i32, Input<G::I2>),
}

#[test]
fn test_node_size() {
    use super::kind::Constant;
    let state = State::new();
    let node =
        Node::<Constant<i32>>::create(state.weak(), state.current_scope(), Kind::Constant(5i32));
    assert_eq!(core::mem::size_of_val(&*node), 400);
}

pub struct GraphvizDot(NodeRef);

impl GraphvizDot {
    pub(crate) fn new_erased(erased: NodeRef) -> Self {
        Self(erased)
    }
    pub fn new<T>(incr: &Incr<T>) -> Self {
        Self::new_erased(incr.node.packed())
    }
    pub fn save_to_file(&self, named: &str) -> std::io::Result<()> {
        use std::fs::File;
        use std::io::Write;
        let mut file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(named)?;
        write!(file, "{}", self)
    }
}

impl Display for GraphvizDot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.dot_write(f)
    }
}

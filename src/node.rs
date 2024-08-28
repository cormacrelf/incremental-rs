use super::CellIncrement;
use crate::node_update::OnUpdateHandler;
use crate::Value;

use super::adjust_heights_heap::AdjustHeightsHeap;
use super::cutoff::Cutoff;
use super::internal_observer::{InternalObserver, ObserverId};
use super::kind::expert::{Invalid, IsEdge, MakeStale, PackedEdge};
use super::kind::{self, Kind, NodeGenerics};
use super::node_update::NodeUpdateDelayed;
use super::scope::Scope;
use super::state::{IncrStatus, State};
use super::{Incr, NodeRef, WeakNode};
use core::fmt::Debug;
use std::any::{Any, TypeId};
use std::cell::Ref;
use std::collections::HashMap;
use std::fmt::{self, Write};
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

#[repr(C)]
pub(crate) struct Node<G: NodeGenerics> {
    pub id: NodeId,

    /* The fields from [recomputed_at] to [created_in] are grouped together and are in the
    same order as they are used by [State.recompute] This has a positive performance
    impact due to cache effects.  Don't change the order of these nodes without
    performance testing. */
    // {{{
    /// The time at which we were last recomputed. -1 if never.
    pub recomputed_at: Cell<StabilisationNum>,
    pub value_opt: RefCell<Option<G::R>>,
    /// We use this flag instead of making kind mutable with an Invalid variant. That makes it much
    /// easier to implement `value_as_ref` for `Kind::MapRef`.
    pub is_valid: Cell<bool>,
    // Don't use this field directly. Use `self::kind()` to be forced to confront
    // the !is_valid case.
    pub _kind: Kind<G>,
    /// The cutoff function. This determines whether we set `changed_at = recomputed_at` during
    /// recomputation, which in turn helps determine if our parents are stale & need recomputing
    /// themselves.
    pub cutoff: RefCell<Cutoff<G::R>>,
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
    /// for one without hitting the allocator. The original does this with a `parent0` and
    /// `parent1_and_beyond: Packed.t Option.t Uniform_array`, where the Option.nones are used to
    /// do what `vec.pop()` does and ignore
    ///
    pub parents: RefCell<SmallVec<[ParentWeak<G::R>; 1]>>,
    /// Scope the node was created in. Never modified.
    pub created_in: Scope,
    // }}}
    /// A handy reference to ourself, which we can use to generate Weak<dyn Trait> versions of our
    /// node's reference-counted pointer at any time.
    pub weak_self: Weak<Self>,

    /// A reference to the incremental State we're part of.
    pub weak_state: Weak<State>,

    pub parent_child_indices: RefCell<ParentChildIndices>,
    // this was for node-level subscriptions
    // pub old_value_opt: RefCell<Option<G::R>>,
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
    pub on_update_handlers: RefCell<Vec<OnUpdateHandler<G::R>>>,
    pub graphviz_user_data: RefCell<Option<Box<dyn Debug>>>,
}

/// Recall that parents and children feel a bit backwards in incremental.
/// A child == an input of self. A parent == a node derived from self.
#[derive(Debug)]
pub(crate) struct ParentChildIndices {
    /// For each input, which number do they know me by?
    pub my_parent_index_in_child_at_index: SmallVec<[i32; 2]>,
    /// For each derived node, which number do they know me by?
    pub my_child_index_in_parent_at_index: SmallVec<[i32; 1]>,
}

pub(crate) type Input<R> = Rc<dyn Incremental<R>>;

pub(crate) trait ErasedIncremental: Debug + ErasedNode {
    fn value_as_ref_any(&self) -> Option<Ref<dyn Any>>;
    fn dyn_remove_parent(&self, index_of_child_in_parent: i32, parent_dyn: &dyn ParentNodeDyn);
    fn dyn_add_parent_without_adjusting_heights(
        &self,
        index_of_child_in_parent: i32,
        parent_ref: &dyn ParentNodeDyn,
        state: &State,
    );
    fn dyn_state_add_parent(
        &self,
        index_of_child_in_parent: i32,
        parent_dyn: &dyn ParentNodeDyn,
        state: &State,
    );
}

impl<G: NodeGenerics> ErasedIncremental for Node<G> {
    fn value_as_ref_any(&self) -> Option<Ref<dyn Any>> {
        let ref_ = self.value_as_ref()?;
        fn g_r_as_any<'a, R: Any>(r: &'a R) -> &'a dyn Any {
            r
        }
        Some(Ref::map(ref_, g_r_as_any))
    }
    fn dyn_remove_parent(&self, index_of_child_in_parent: i32, parent_dyn: &dyn ParentNodeDyn) {
        let checked_ref: ParentRef<G::R> = parent_dyn
            .try_downcast_r_ref::<G::R>(index_of_child_in_parent)
            .unwrap();
        Incremental::remove_parent(self, index_of_child_in_parent, checked_ref)
    }

    fn dyn_add_parent_without_adjusting_heights(
        &self,
        index_of_child_in_parent: i32,
        parent_dyn: &dyn ParentNodeDyn,
        state: &State,
    ) {
        let checked_ref: ParentRef<G::R> = parent_dyn
            .try_downcast_r_ref::<G::R>(index_of_child_in_parent)
            .unwrap();
        Incremental::add_parent_without_adjusting_heights(
            self,
            index_of_child_in_parent,
            checked_ref,
            state,
        )
    }
    fn dyn_state_add_parent(
        &self,
        index_of_child_in_parent: i32,
        parent_dyn: &dyn ParentNodeDyn,
        state: &State,
    ) {
        let checked_ref: ParentRef<G::R> = parent_dyn
            .try_downcast_r_ref::<G::R>(index_of_child_in_parent)
            .unwrap();
        Incremental::state_add_parent(self, index_of_child_in_parent, checked_ref, state)
    }
}

pub(crate) trait Incremental<R>: ErasedNode + Debug {
    fn as_input(&self) -> Input<R>;
    fn erased_input(&self) -> &dyn ErasedIncremental;
    fn latest(&self) -> R;
    fn value_opt(&self) -> Option<R>;
    fn add_parent_without_adjusting_heights(
        &self,
        child_index: i32,
        parent_ref: ParentRef<R>,
        state: &State,
    );
    fn state_add_parent(&self, child_index: i32, parent_ref: ParentRef<R>, state: &State);
    fn remove_parent(&self, child_index: i32, parent_ref: ParentRef<R>);
    fn set_cutoff(&self, cutoff: Cutoff<R>);
    fn set_graphviz_user_data(&self, user_data: Box<dyn Debug>);
    fn value_as_ref(&self) -> Option<Ref<R>>;
    fn constant(&self) -> Option<&R>;
    fn add_observer(&self, id: ObserverId, weak: Weak<InternalObserver<R>>);
    fn remove_observer(&self, id: ObserverId);
    fn add_on_update_handler(&self, handler: OnUpdateHandler<R>);
}

impl<G: NodeGenerics> Incremental<G::R> for Node<G> {
    fn as_input(&self) -> Input<G::R> {
        self.weak_self.upgrade().unwrap() as Input<G::R>
    }
    fn erased_input(&self) -> &dyn ErasedIncremental {
        self
    }
    fn latest(&self) -> G::R {
        self.value_opt().unwrap()
    }
    fn value_opt(&self) -> Option<G::R> {
        self.value_as_ref().map(|x| G::R::clone(&x))
    }
    // #[tracing::instrument]
    fn add_parent_without_adjusting_heights(
        &self,
        child_index: i32,
        parent_ref: ParentRef<G::R>,
        state: &State,
    ) {
        let p = parent_ref.erased();
        debug_assert!(p.is_necessary());
        let was_necessary = self.is_necessary();
        self.add_parent(child_index, parent_ref.clone());
        if !self.is_valid() {
            let mut pi = state.propagate_invalidity.borrow_mut();
            pi.push(p.weak());
        }
        if !was_necessary {
            self.became_necessary(state);
        }
        if let Some(Kind::Expert(expert)) = self.kind() {
            expert.run_edge_callback(child_index)
        }
    }
    fn state_add_parent(&self, child_index: i32, parent_ref: ParentRef<G::R>, state: &State) {
        let parent = parent_ref.erased();
        tracing::debug!(child_id = ?self.id, child_index = %child_index, parent = %parent.kind_debug_ty(), "state_add_parent");
        debug_assert!(parent.is_necessary());
        self.add_parent_without_adjusting_heights(child_index, parent_ref.clone(), state);
        if self.height() >= parent.height() {
            // This happens when change_child whacks a neew child in
            // What's happening here is that
            tracing::debug!(
                "self.height() = {:?}, parent.height() = {:?}",
                self.height(),
                parent.height()
            );
            let mut ah_heap = state.adjust_heights_heap.borrow_mut();
            let rch = &state.recompute_heap;
            ah_heap.adjust_heights(rch, self.packed(), parent.packed());
        }
        state.propagate_invalidity();
        /* we only add necessary parents */
        debug_assert!(parent.is_necessary());
        if !parent.is_in_recompute_heap()
            && (parent.recomputed_at().get().is_never() || self.edge_is_stale(parent))
        {
            state.recompute_heap.insert(parent.packed());
        }
    }

    #[rustfmt::skip]
    fn remove_parent(&self, child_index: i32, parent_ref: ParentRef<G::R>) {
        let child = self;
        let mut child_indices = child.parent_child_indices.borrow_mut();
        let mut child_parents = child.parents.borrow_mut();
        let parent = parent_ref.erased();
        let parent_indices_cell = parent.parent_child_indices();
        let mut parent_indices = parent_indices_cell.borrow_mut();
        tracing::debug!(child_id = ?child.id, child_index = %child_index, parent = %parent.kind_debug_ty(), "remove_parent");

        let parent_index = parent_indices.my_parent_index_in_child_at_index[child_index as usize];

        debug_assert!(
            child_parents.len() >= 1 && parent_index >= 0,
            "my_parent_index_in_child_at_index[child_index] = {parent_index}, parent has already been removed?
            child_index = {child_index}
            my_parent_index_in_child_at_index = {mpi:?}
            my_child_index_in_parent_at_index = {mci:?}
            parent_index = {parent_index}
            child_parents = {child_parents:?}
            parent_type = {pty}
            child_type = {chty:?}
            ",
            mpi = &parent_indices.my_parent_index_in_child_at_index,
            mci = &child_indices.my_child_index_in_parent_at_index,
            child_parents = child_parents
                .iter()
                .map(|p| p.upgrade_erased().ok().map(|p| p.kind_debug_ty()))
                .collect::<Vec<_>>(),
            pty = parent.kind_debug_ty(),
            chty = child.kind().map(|k| k.debug_ty()),
        );
        debug_assert!(parent_ref.weak().ptr_eq(&child_parents[parent_index as usize].clone()));
        let last_parent_index = child_parents.len() - 1;
        if (parent_index as usize) < last_parent_index {
            // we swap the parent the end of the array into this one's position. This requires much fewer index twiddles than shifting
            // all subsequent indices back by one.
            let end_p_weak = child_parents[last_parent_index].clone();
            if let Ok(end_p) = end_p_weak.upgrade_erased() {
                let end_p_indices_cell = end_p.parent_child_indices();
                let mut end_p_indices = end_p_indices_cell.borrow_mut();
                let end_child_index = child_indices.my_child_index_in_parent_at_index[last_parent_index];
                // link parent_index & end_child_index
                end_p_indices.my_parent_index_in_child_at_index[end_child_index as usize] = parent_index;
                child_indices.my_child_index_in_parent_at_index[parent_index as usize] = end_child_index;
            } else {
                tracing::error!("end_p_weak pointer could not be upgraded (child_parents[{last_parent_index}])");
            }
        }
        // unlink last_parent_index & child_index
        parent_indices.my_parent_index_in_child_at_index[child_index as usize] = -1;
        child_indices.my_child_index_in_parent_at_index[last_parent_index] = -1;

        // now do what we just did but super easily in the actual Vec
        child_parents.swap_remove(parent_index as usize);
    }

    fn set_cutoff(&self, cutoff: Cutoff<G::R>) {
        self.cutoff.replace(cutoff);
    }

    fn set_graphviz_user_data(&self, user_data: Box<dyn Debug>) {
        let mut slot = self.graphviz_user_data.borrow_mut();
        slot.replace(user_data);
    }

    fn value_as_ref(&self) -> Option<Ref<G::R>> {
        if let Some(Kind::MapRef(mapref)) = self.kind() {
            let mapper = &mapref.mapper;
            let input = mapref.input.value_as_ref()?;
            let mapped = Ref::filter_map(input, |iref| Some(mapper(iref))).ok();
            return mapped;
        }
        let v = self.value_opt.borrow();
        Ref::filter_map(v, |o| o.as_ref()).ok()
    }
    fn constant(&self) -> Option<&G::R> {
        if let Some(Kind::Constant(value)) = self.kind() {
            Some(value)
        } else {
            None
        }
    }
    fn add_observer(&self, id: ObserverId, weak: Weak<InternalObserver<G::R>>) {
        let mut os = self.observers.borrow_mut();
        os.insert(id, weak);
    }

    fn remove_observer(&self, id: ObserverId) {
        let mut os = self.observers.borrow_mut();
        os.remove(&id);
    }
    fn add_on_update_handler(&self, handler: OnUpdateHandler<G::R>) {
        self.num_on_update_handlers.increment();
        let mut ouh = self.on_update_handlers.borrow_mut();
        ouh.push(handler);
    }
}

#[derive(Debug)]
pub(crate) enum ParentError {
    DowncastFailed,
    ParentInvalidated,
    ChildHasNoValue,
    ParentDeallocated,
    IndexMismatch,
}

impl fmt::Display for ParentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

pub(crate) trait ParentNode<I> {
    fn child_changed(
        &self,
        child: &dyn Incremental<I>,
        child_index: i32,
        old_value_opt: Option<&I>,
    ) -> Result<NodeRef, ParentError>;
}
pub(crate) trait ParentNodeDyn: Debug + Any {
    fn dyn_child_changed(
        &self,
        child: &dyn ErasedIncremental,
        child_index: i32,
        old_value_opt: Option<&dyn Any>,
    ) -> Result<(), ParentError>;
    fn dyn_packed(self: Rc<Self>) -> NodeRef;
    fn dyn_erased(&self) -> &dyn ErasedNode;
    fn dyn_parent_weak(&self) -> Weak<dyn ParentNodeDyn>;
    fn input_type_id(&self, index_of_child_in_parent: i32) -> TypeId;
}
impl dyn ParentNodeDyn {
    fn try_downcast_r_ref<R: Any>(
        &self,
        index_of_child_in_parent: i32,
    ) -> Result<ParentRef<R>, ParentError> {
        let r_id = TypeId::of::<R>();
        let self_i_id = self.input_type_id(index_of_child_in_parent);
        if r_id != self_i_id {
            return Err(ParentError::DowncastFailed);
        }
        Ok(ParentRef::Dyn(self, index_of_child_in_parent))
    }
}

pub(crate) trait Parent1<I>: Debug + ErasedNode {
    fn p1_child_changed(
        &self,
        child: &dyn Incremental<I>,
        child_index: i32,
        old_value_opt: Option<&I>,
    ) -> Result<(), ParentError>;
    fn p1_erased(&self) -> &dyn ErasedNode;
    fn p1_packed(self: Rc<Self>) -> NodeRef;
    fn p1_parent_weak(&self) -> ParentWeak<I>;
}
pub(crate) trait Parent2<I>: Debug + ErasedNode {
    fn p2_child_changed(
        &self,
        child: &dyn Incremental<I>,
        child_index: i32,
        old_value_opt: Option<&I>,
    ) -> Result<(), ParentError>;
    fn p2_erased(&self) -> &dyn ErasedNode;
    fn p2_packed(self: Rc<Self>) -> NodeRef;
    fn p2_parent_weak(&self) -> ParentWeak<I>;
}

/// Each node can be a parent for up to a dozen or so (map12) different types.
/// ParentRef is an enum that knows, by the tag, which kind of parent this node is behaving as,
/// and the various Parent1/Parent2 traits embed that static, type-level information in their
/// implementation. So all a *child* node has to do is pass through its G::R, and the parent
/// will make sure it passes the correct kind of ParentRef that has an implementation for that
/// type.
///
/// Ultimately, we only need one type-aware method from these parent nodes. That's
/// `child_changed`. And we only need for eventually Expert. Lot of work
/// for one method. But there you go.

/// This is what's passed down to a child, when the parent knows what type the child sees it as.
/// For child.remove_parent calls, basically.
#[derive(Debug, Clone)]
pub(crate) enum ParentRef<'a, T> {
    I1(&'a dyn Parent1<T>),
    I2(&'a dyn Parent2<T>),
    Dyn(&'a dyn ParentNodeDyn, i32),
}
impl<T: Value> ParentRef<'_, T> {
    fn erased(&self) -> &dyn ErasedNode {
        match self {
            Self::I1(w) => w.p1_erased(),
            Self::I2(w) => w.p2_erased(),
            Self::Dyn(w, _i) => w.dyn_erased(),
        }
    }
    fn weak(&self) -> ParentWeak<T> {
        match self {
            Self::I1(w) => w.p1_parent_weak(),
            Self::I2(w) => w.p2_parent_weak(),
            Self::Dyn(w, i) => ParentWeak::Dynamic(w.dyn_parent_weak(), *i),
        }
    }
}

// There's nothing stopping us from reimplementing Input1/Input2 by using dyn even for simple map1
// nodes, but the typed variants may be faster. Worth benchmarking.
#[derive(Debug, Clone)]
pub(crate) enum ParentWeak<T> {
    Input1(Weak<dyn Parent1<T>>),
    Input2(Weak<dyn Parent2<T>>),
    Dynamic(Weak<dyn ParentNodeDyn>, i32),
}

impl<T: Value> ParentWeak<T> {
    fn upgrade_erased(&self) -> Result<NodeRef, ParentError> {
        match self {
            Self::Input1(w) => w.upgrade().map(|x| x.p1_packed()),
            Self::Input2(w) => w.upgrade().map(|x| x.p2_packed()),
            Self::Dynamic(w, _index) => w.upgrade().map(|x| x.dyn_packed()),
        }
        .ok_or(ParentError::ParentDeallocated)
    }
    fn ptr_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Input1(a), Self::Input1(b)) => a.ptr_eq(b),
            (Self::Input2(a), Self::Input2(b)) => a.ptr_eq(b),
            (Self::Dynamic(a, ix_a), Self::Dynamic(b, ix_b)) => ix_a == ix_b && a.ptr_eq(b),
            _ => false,
        }
    }
}

impl<T: Value> ParentNode<T> for ParentWeak<T> {
    fn child_changed(
        &self,
        child: &dyn Incremental<T>,
        child_index: i32,
        old_value_opt: Option<&T>,
    ) -> Result<NodeRef, ParentError> {
        match self {
            Self::Input1(i1) => {
                let i1 = i1.upgrade().ok_or(ParentError::ParentDeallocated)?;
                i1.p1_child_changed(child, child_index, old_value_opt)?;
                Ok(i1.p1_packed())
            }
            Self::Input2(i2) => {
                let i2 = i2.upgrade().ok_or(ParentError::ParentDeallocated)?;
                i2.p2_child_changed(child, child_index, old_value_opt)?;
                Ok(i2.p2_packed())
            }
            Self::Dynamic(idyn, index_of_child_in_parent) => {
                // Maybe we don't actually need to store the index, since the child does know
                // what index it is in the parent. Alternatively, storing the
                // `index_of_child_in_parent` in the parent ref itself is probably more robust
                // than the array manipulation in add/remove parent, which took many tries
                // to get right.
                if *index_of_child_in_parent != child_index {
                    return Err(ParentError::IndexMismatch);
                }
                let dyn_parent = idyn.upgrade().ok_or(ParentError::ParentDeallocated)?;
                dyn_parent.dyn_child_changed(
                    child.erased_input(),
                    child_index,
                    old_value_opt.map(|t| t as &dyn Any),
                )?;
                Ok(dyn_parent.dyn_packed())
            }
        }
    }
}

impl<G: NodeGenerics> ParentNodeDyn for Node<G> {
    fn dyn_packed(self: Rc<Self>) -> NodeRef {
        self
    }
    fn dyn_erased(&self) -> &dyn ErasedNode {
        self
    }
    fn dyn_parent_weak(&self) -> Weak<dyn ParentNodeDyn> {
        self.weak_self.clone()
    }
    fn input_type_id(&self, index_of_child_in_parent: i32) -> TypeId {
        if let Some(Kind::Expert(e)) = self.kind() {
            return e.expert_input_type_id(index_of_child_in_parent);
        }
        match index_of_child_in_parent {
            0 => TypeId::of::<G::I1>(),
            1 => TypeId::of::<G::I2>(),
            2 => TypeId::of::<G::I3>(),
            3 => TypeId::of::<G::I4>(),
            4 => TypeId::of::<G::I5>(),
            5 => TypeId::of::<G::I6>(),
            _ => todo!("only implemented up to map5"),
        }
    }
    fn dyn_child_changed(
        &self,
        child: &dyn ErasedIncremental,
        child_index: i32,
        old_value_opt: Option<&dyn Any>,
    ) -> Result<(), ParentError> {
        let Some(kind) = self.kind() else {
            return Err(ParentError::ParentInvalidated);
        };
        match kind {
            Kind::Expert(expert) => expert.run_edge_callback(child_index),
            Kind::MapRef(mapref) => {
                // mapref is the only node that uses old_value_opt in child_changed.
                //
                let self_old = old_value_opt
                    .map(|v| v.downcast_ref::<G::I1>().ok_or(ParentError::DowncastFailed))
                    .transpose()?
                    .map(|v| (mapref.mapper)(v));
                let child_new = child
                    .value_as_ref_any()
                    .ok_or(ParentError::ChildHasNoValue)?;
                let child_downcast = child_new
                    .downcast_ref::<G::I1>()
                    .ok_or(ParentError::DowncastFailed)?;
                let self_new = (mapref.mapper)(child_downcast);

                let did_change = self_old.map_or(true, |old| {
                    !self.cutoff.borrow_mut().should_cutoff(old, self_new)
                });
                mapref.did_change.set(did_change);
                // now we propagate to parent
                // (but first, set the only_in_debug stuff & recomputed_at <- t.stabilisation_num)
                let pci = self.parent_child_indices.borrow();
                for (parent_index, parent) in self.parents.borrow().iter().enumerate() {
                    let child_index = pci.my_child_index_in_parent_at_index[parent_index];
                    parent.child_changed(self, child_index, self_old)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl<G: NodeGenerics> Parent2<G::I2> for Node<G> {
    fn p2_child_changed(
        &self,
        _child: &dyn Incremental<G::I2>,
        child_index: i32,
        _old_value_opt: Option<&G::I2>,
    ) -> Result<(), ParentError> {
        if let Kind::Expert(expert) = self.kind().ok_or(ParentError::ParentInvalidated)? {
            expert.run_edge_callback(child_index)
        }
        Ok(())
    }
    fn p2_erased(&self) -> &dyn ErasedNode {
        self
    }
    fn p2_packed(self: Rc<Self>) -> NodeRef {
        self
    }

    fn p2_parent_weak(&self) -> ParentWeak<G::I2> {
        self.as_parent2_weak()
    }
}

impl<G: NodeGenerics> Parent1<G::I1> for Node<G> {
    fn p1_child_changed(
        &self,
        child: &dyn Incremental<G::I1>,
        child_index: i32,
        old_value_opt: Option<&G::I1>,
    ) -> Result<(), ParentError> {
        let kind = self.kind().ok_or(ParentError::ParentInvalidated)?;
        match kind {
            Kind::Expert(expert) => expert.run_edge_callback(child_index),
            Kind::MapRef(mapref) => {
                // mapref is the only node that uses old_value_opt in child_changed.
                //
                let self_old = old_value_opt.map(|v| (mapref.mapper)(v));
                let child_new = child.value_as_ref().unwrap();
                let self_new = (mapref.mapper)(&child_new);

                let did_change = self_old.map_or(true, |old| {
                    !self.cutoff.borrow_mut().should_cutoff(old, self_new)
                });
                mapref.did_change.set(did_change);
                // now we propagate to parent
                // (but first, set the only_in_debug stuff & recomputed_at <- t.stabilisation_num)
                let pci = self.parent_child_indices.borrow();
                for (parent_index, parent) in self.parents.borrow().iter().enumerate() {
                    let child_index = pci.my_child_index_in_parent_at_index[parent_index];
                    parent.child_changed(self, child_index, self_old)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn p1_erased(&self) -> &dyn ErasedNode {
        self
    }
    fn p1_packed(self: Rc<Self>) -> NodeRef {
        self
    }

    fn p1_parent_weak(&self) -> ParentWeak<G::I1> {
        self.as_parent_weak()
    }
}

pub(crate) trait ErasedNode: Debug {
    fn id(&self) -> NodeId;
    fn ptr_eq(&self, other: &dyn ErasedNode) -> bool;
    fn kind_debug_ty(&self) -> String;
    fn weak_state(&self) -> &Weak<State>;
    fn is_valid(&self) -> bool;
    fn dot_label(&self, f: &mut dyn Write) -> fmt::Result;
    fn dot_node(&self, f: &mut dyn Write, name: &str) -> fmt::Result;
    fn dot_add_bind_edges(&self, bind_edges: &mut Vec<(NodeRef, NodeRef)>);
    fn dot_was_recomputed(&self, state: &State) -> bool;
    fn dot_was_changed(&self) -> bool;
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
    fn edge_is_stale(&self, parent: &dyn ErasedNode) -> bool;
    fn is_necessary(&self) -> bool;
    fn force_necessary(&self) -> &Cell<bool>;
    fn needs_to_be_computed(&self) -> bool;
    fn became_necessary(&self, state: &State);
    fn became_necessary_propagate(&self, state: &State);
    fn became_unnecessary(&self, state: &State);
    fn check_if_unnecessary(&self, state: &State);
    fn is_in_recompute_heap(&self) -> bool;
    fn recompute(&self, state: &State);
    fn recompute_one(&self, state: &State) -> Option<NodeRef>;
    fn parent_iter_can_recompute_now(&self, child: &dyn ErasedNode, state: &State) -> bool;
    fn parent_child_indices(&self) -> &RefCell<ParentChildIndices>;
    fn expert_swap_children_except_in_kind(
        &self,
        child1: &NodeRef,
        child_index: i32,
        child2: &NodeRef,
        child2_index: i32,
    );
    fn expert_remove_child(&self, packed_edge: &dyn IsEdge, child_index: i32, state: &State);
    fn state_opt(&self) -> Option<Rc<State>>;
    fn state(&self) -> Rc<State>;
    fn weak(&self) -> WeakNode;
    fn packed(&self) -> NodeRef;
    fn erased(&self) -> &dyn ErasedNode;
    fn foreach_child(&self, f: &mut dyn FnMut(i32, NodeRef));
    fn iter_descendants_internal_one(
        &self,
        seen: &mut HashMap<NodeId, i32>,
        f: &mut dyn FnMut(&NodeRef),
    );
    fn recomputed_at(&self) -> &Cell<StabilisationNum>;
    fn changed_at(&self) -> &Cell<StabilisationNum>;
    fn invalidate_node(&self, state: &State);
    #[cfg(debug_assertions)]
    #[allow(unused)]
    fn assert_currently_running_node_is_child(&self, name: &'static str);
    #[cfg(debug_assertions)]
    #[allow(unused)]
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
    fn maybe_handle_after_stabilisation(&self, state: &State);
    fn handle_after_stabilisation(&self, state: &State);
    fn num_on_update_handlers(&self) -> &Cell<i32>;
    fn node_update(&self) -> NodeUpdateDelayed;

    fn expert_make_stale(&self);
    fn expert_add_dependency(&self, packed_edge: PackedEdge);
    fn expert_remove_dependency(&self, dyn_edge: &dyn IsEdge);
}

impl<G: NodeGenerics> Debug for Node<G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Node")
            .field("id", &self.id)
            .field("type", &self.kind().map(|k| k.debug_ty()))
            .field("kind", &self.kind())
            // .field("height", &self.height.get())
            .finish()
    }
}

impl<G: NodeGenerics> ErasedNode for Node<G> {
    fn id(&self) -> NodeId {
        self.id
    }
    fn ptr_eq(&self, other: &dyn ErasedNode) -> bool {
        self.weak().ptr_eq(&other.weak())
    }
    fn kind_debug_ty(&self) -> String {
        let Some(dbg) = self.kind().map(|k| k.debug_ty()) else {
            if let Some(user) = self.graphviz_user_data.borrow().as_ref() {
                return format!("{user:?}: Invalid node");
            }
            return format!("Invalid node");
        };
        if let Some(user) = self.graphviz_user_data.borrow().as_ref() {
            return format!("{user:?}: {dbg:?}");
        }
        format!("{dbg:?}")
    }
    fn weak_state(&self) -> &Weak<State> {
        &self.weak_state
    }
    fn weak(&self) -> WeakNode {
        self.weak_self.clone() as WeakNode
    }
    fn packed(&self) -> NodeRef {
        self.weak_self.upgrade().unwrap() as NodeRef
    }
    fn erased(&self) -> &dyn ErasedNode {
        self
    }
    fn parent_child_indices(&self) -> &RefCell<ParentChildIndices> {
        &self.parent_child_indices
    }
    fn state_opt(&self) -> Option<Rc<State>> {
        self.weak_state.upgrade()
    }
    fn state(&self) -> Rc<State> {
        self.weak_state.upgrade().unwrap()
    }
    fn is_valid(&self) -> bool {
        self.is_valid.get()
    }
    fn should_be_invalidated(&self) -> bool {
        let Some(kind) = self.kind() else {
            return false;
        };
        match kind {
            Kind::Constant(_) | Kind::Var(_) => false,
            Kind::ArrayFold(..)
            | Kind::Map(..)
            | Kind::MapWithOld(..)
            | Kind::MapRef(..)
            | Kind::Map2(..)
            | Kind::Map3(..)
            | Kind::Map4(..)
            | Kind::Map5(..)
            | Kind::Map6(..) => self.has_invalid_child(),
            /* A *_change node is invalid if the node it is watching for changes is invalid (same
            reason as above).  This is equivalent to [has_invalid_child t]. */
            Kind::BindLhsChange { bind, .. } => !bind.lhs.is_valid(),
            /* [Bind_main], [If_then_else], and [Join_main] are invalid if their *_change child is,
            but not necessarily if their other children are -- the graph may be restructured to
            avoid the invalidity of those. */
            Kind::BindMain { lhs_change, .. } => !lhs_change.is_valid(),
            /* This is similar to what we do for bind above, except that any invalid child can be
            removed, so we can only tell if an expert node becomes invalid when all its
            dependencies have fired (which in practice means when we are about to run it). */
            Kind::Expert(_) => false,
        }
    }
    fn propagate_invalidity_helper(&self) {
        let Some(kind) = self.kind() else { return };
        match kind {
            /* If multiple children are invalid, they will push us as many times on the
            propagation stack, so we count them right. */
            Kind::Expert(expert) => expert.incr_invalid_children(),
            _kind => {
                #[cfg(debug_assertions)]
                match _kind {
                    Kind::BindMain { .. } => (), // and IfThenElse, JoinMain
                    _ => panic!("nodes with no children are never pushed on the stack"),
                }
            }
        }
    }
    fn has_invalid_child(&self) -> bool {
        let mut any = false;
        self.foreach_child(&mut |_ix, child| {
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
            ahh.ensure_height_requirement(original_child, original_parent, &self.packed(), &parent);
        }
    }
    fn set_height(&self, height: i32) {
        tracing::trace!("{:?} set height to {height}", self.id);
        self.height.set(height);
    }
    fn is_stale(&self) -> bool {
        let Some(kind) = self.kind() else {
            return false;
        };
        match kind {
            Kind::Var(var) => {
                let set_at = var.set_at.get();
                let recomputed_at = self.recomputed_at.get();
                set_at > recomputed_at
            }
            Kind::Constant(_) => self.recomputed_at.get().is_never(),

            Kind::MapRef(_)
            | Kind::ArrayFold(_)
            | Kind::Map(_)
            | Kind::MapWithOld(_)
            | Kind::Map2(_)
            | Kind::Map3(_)
            | Kind::Map4(_)
            | Kind::Map5(_)
            | Kind::Map6(_)
            | Kind::BindLhsChange { .. }
            | Kind::BindMain { .. } => {
                // i.e. never recomputed, or a child has changed more recently than we have been
                // recomputed
                self.recomputed_at.get().is_never() || self.is_stale_with_respect_to_a_child()
            }
            Kind::Expert(e) => {
                e.force_stale.get()
                    || self.recomputed_at.get().is_never()
                    || self.is_stale_with_respect_to_a_child()
            }
        }
    }
    fn is_stale_with_respect_to_a_child(&self) -> bool {
        let mut is_stale = false;
        // TODO: make a version of try_fold for this, to short-circuit it
        self.foreach_child(&mut |_ix, child| {
            tracing::trace!(
                "child.changed_at {:?} >? self.recomputed_at {:?}",
                child.changed_at().get(),
                self.recomputed_at.get()
            );
            is_stale = is_stale || child.changed_at().get() > self.recomputed_at.get()
        });
        is_stale
    }
    fn edge_is_stale(&self, parent: &dyn ErasedNode) -> bool {
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
    fn became_necessary_propagate(&self, state: &State) {
        self.became_necessary(state);
        state.propagate_invalidity();
    }

    // #[tracing::instrument]
    fn became_necessary(&self, state: &State) {
        if self.is_valid() && !self.created_in.is_necessary() {
            panic!("trying to make a node necessary whose defining bind is not necessary");
        }
        tracing::debug!("node {:?} became necessary", self.id);
        state.num_nodes_became_necessary.increment();
        self.maybe_handle_after_stabilisation(state);
        /* Since [node] became necessary, to restore the invariant, we need to:
        - add parent pointers to [node] from its children.
        - set [node]'s height.
        - add [node] to the recompute heap, if necessary. */
        state.set_height(self.packed(), self.created_in.height() + 1);
        let h = &Cell::new(self.height());
        let p1 = self.as_parent_ref();
        let p2 = self.as_parent2_ref();
        let pdyn = self.as_parent_dyn_ref();
        self.foreach_child_typed(ForeachChild {
            i1: &mut move |index, child| {
                child.add_parent_without_adjusting_heights(index, p1.clone(), state);
                if child.height() >= h.get() {
                    h.set(child.height() + 1);
                }
            },
            i2: &mut move |index, child| {
                child.add_parent_without_adjusting_heights(index, p2.clone(), state);
                if child.height() >= h.get() {
                    h.set(child.height() + 1);
                }
            },
            idyn: &mut move |index, child| {
                child.dyn_add_parent_without_adjusting_heights(index, pdyn, state);
                if child.height() >= h.get() {
                    h.set(child.height() + 1);
                }
            },
        });
        state.set_height(self.packed(), h.get());
        debug_assert!(!self.is_in_recompute_heap());
        debug_assert!(self.is_necessary());
        if self.is_stale() {
            state.recompute_heap.insert(self.packed());
        }
        if let Some(Kind::Expert(expert)) = self.kind() {
            expert.observability_change(true)
        }
    }

    fn check_if_unnecessary(&self, state: &State) {
        if !self.is_necessary() {
            self.became_unnecessary(state);
        }
    }

    fn became_unnecessary(&self, state: &State) {
        tracing::debug!("node {:?} became unnecessary", self.id);
        state.num_nodes_became_unnecessary.increment();
        self.maybe_handle_after_stabilisation(state);
        state.set_height(self.packed(), -1);
        self.remove_children(state);
        if let Some(Kind::Expert(expert)) = self.kind() {
            expert.observability_change(false)
        }
        debug_assert!(!self.needs_to_be_computed());
        if self.is_in_recompute_heap() {
            state.recompute_heap.remove(self.packed());
        }
    }

    fn foreach_child(&self, f: &mut dyn FnMut(i32, NodeRef)) {
        let Some(kind) = self.kind() else { return };
        match kind {
            Kind::Constant(_) => {}
            Kind::Map(kind::MapNode { input, .. })
            | Kind::MapRef(kind::MapRefNode { input, .. })
            | Kind::MapWithOld(kind::MapWithOld { input, .. }) => f(0, input.packed()),
            Kind::Map2(kind::Map2Node { one, two, .. }) => {
                f(0, one.clone().packed());
                f(1, two.clone().packed());
            }
            Kind::Map3(kind::Map3Node {
                one, two, three, ..
            }) => {
                f(0, one.clone().packed());
                f(1, two.clone().packed());
                f(2, three.clone().packed());
            }
            Kind::Map4(kind::Map4Node {
                one,
                two,
                three,
                four,
                ..
            }) => {
                f(0, one.clone().packed());
                f(1, two.clone().packed());
                f(2, three.clone().packed());
                f(3, four.clone().packed());
            }
            Kind::Map5(kind::Map5Node {
                one,
                two,
                three,
                four,
                five,
                ..
            }) => {
                f(0, one.clone().packed());
                f(1, two.clone().packed());
                f(2, three.clone().packed());
                f(3, four.clone().packed());
                f(4, five.clone().packed());
            }
            Kind::Map6(kind::Map6Node {
                one,
                two,
                three,
                four,
                five,
                six,
                ..
            }) => {
                f(0, one.clone().packed());
                f(1, two.clone().packed());
                f(2, three.clone().packed());
                f(3, four.clone().packed());
                f(4, five.clone().packed());
                f(5, six.clone().packed());
            }
            Kind::BindLhsChange { bind, .. } => f(0, bind.lhs.packed()),
            Kind::BindMain {
                bind, lhs_change, ..
            } => {
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
            Kind::Var(_var) => {}
            Kind::Expert(e) => {
                let borrow_span = tracing::debug_span!("expert.children.borrow() in foreach_child");
                borrow_span.in_scope(|| {
                    for (ix, child) in e.children.borrow().iter().enumerate() {
                        f(ix as i32, child.packed())
                    }
                });
            }
        }
    }
    fn is_in_recompute_heap(&self) -> bool {
        self.height_in_recompute_heap.get() >= 0
    }
    fn changed_at(&self) -> &Cell<StabilisationNum> {
        &self.changed_at
    }
    fn recomputed_at(&self) -> &Cell<StabilisationNum> {
        &self.recomputed_at
    }

    fn recompute(&self, state: &State) {
        // This is a flattened version of the original recursion, which OCaml could probably tail-call
        // optimise. First recompute self
        let Some(mut parent) = self.recompute_one(state) else {
            return;
        };

        // Then, as far as we can_recompute_now, recompute parent
        while let Some(next_parent) = parent.recompute_one(state) {
            parent = next_parent;
        }
    }

    #[tracing::instrument(skip(self, state), fields(height = %self.height(), id = ?self.id, node = %self.kind_debug_ty()))]
    fn recompute_one(&self, state: &State) -> Option<NodeRef> {
        #[cfg(debug_assertions)]
        {
            state
                .only_in_debug
                .currently_running_node
                .replace(Some(self.weak()));
            // t.only_in_debug.expert_nodes_created_by_current_node <- []);
        }
        state.num_nodes_recomputed.increment();
        self.recomputed_at.set(state.stabilisation_num.get());

        let Some(kind) = self.kind() else {
            // We should not be invalidating nodes that have already been queued for recompute.
            // invalidate_nodes_created_on_rhs should only invalidate a node higher than the
            // current one. So removing such nodes from the recompute heap should work.
            panic!("recomputing invalid node {:?}", self.id);
        };

        match kind {
            Kind::Map(map) => {
                let new_value = {
                    let mut f = map.mapper.borrow_mut();
                    let input = map.input.value_as_ref().unwrap();
                    f(&input)
                };
                self.maybe_change_value(new_value, state)
            }
            Kind::Var(var) => {
                let new_value = {
                    let value = var.value.borrow();
                    value.clone()
                };
                self.maybe_change_value(new_value, state)
            }
            Kind::Constant(v) => self.maybe_change_value(v.clone(), state),
            Kind::MapRef(mapref) => {
                // don't run child_changed on our parents, because we already did that in OUR child_changed.
                self.maybe_change_value_manual(None, None, mapref.did_change.get(), false, state)
            }
            Kind::MapWithOld(map) => {
                let input = map.input.value_as_ref().unwrap();
                let mut f = map.mapper.borrow_mut();
                let (new_value, did_change) = {
                    let mut current_value = self.value_opt.borrow_mut();
                    tracing::trace!("<- old: {current_value:?}");
                    f(current_value.take(), &input)
                };
                self.maybe_change_value_manual(None, Some(new_value), did_change, true, state)
            }
            Kind::Map2(map2) => {
                let i1 = map2.one.value_as_ref().unwrap();
                let i2 = map2.two.value_as_ref().unwrap();
                let mut f = map2.mapper.borrow_mut();
                let new_value = f(&i1, &i2);
                self.maybe_change_value(new_value, state)
            }
            Kind::Map3(map) => {
                let i1 = map.one.value_as_ref().unwrap();
                let i2 = map.two.value_as_ref().unwrap();
                let i3 = map.three.value_as_ref().unwrap();
                let mut f = map.mapper.borrow_mut();
                let new_value = f(&i1, &i2, &i3);
                self.maybe_change_value(new_value, state)
            }
            Kind::Map4(map) => {
                let i1 = map.one.value_as_ref().unwrap();
                let i2 = map.two.value_as_ref().unwrap();
                let i3 = map.three.value_as_ref().unwrap();
                let i4 = map.four.value_as_ref().unwrap();
                let mut f = map.mapper.borrow_mut();
                let new_value = f(&i1, &i2, &i3, &i4);
                self.maybe_change_value(new_value, state)
            }
            Kind::Map5(map) => {
                let i1 = map.one.value_as_ref().unwrap();
                let i2 = map.two.value_as_ref().unwrap();
                let i3 = map.three.value_as_ref().unwrap();
                let i4 = map.four.value_as_ref().unwrap();
                let i5 = map.five.value_as_ref().unwrap();
                let mut f = map.mapper.borrow_mut();
                let new_value = f(&i1, &i2, &i3, &i4, &i5);
                self.maybe_change_value(new_value, state)
            }
            Kind::Map6(map) => {
                let i1 = map.one.value_as_ref().unwrap();
                let i2 = map.two.value_as_ref().unwrap();
                let i3 = map.three.value_as_ref().unwrap();
                let i4 = map.four.value_as_ref().unwrap();
                let i5 = map.five.value_as_ref().unwrap();
                let i6 = map.six.value_as_ref().unwrap();
                let mut f = map.mapper.borrow_mut();
                let new_value = f(&i1, &i2, &i3, &i4, &i5, &i6);
                self.maybe_change_value(new_value, state)
            }
            Kind::BindLhsChange { casts, bind } => {
                // leaves an empty vec for next time
                // TODO: we could double-buffer this to save allocations.
                let mut old_all_nodes_created_on_rhs = bind.all_nodes_created_on_rhs.take();
                let lhs = bind.lhs.value_as_ref().unwrap();
                let rhs = {
                    let old_scope = state.current_scope();
                    *state.current_scope.borrow_mut() = bind.rhs_scope.borrow().clone();
                    let mut f = bind.mapper.borrow_mut();
                    let rhs = f(&lhs);
                    *state.current_scope.borrow_mut() = old_scope;
                    // Check that the returned RHS node is from the same world.
                    assert!(crate::weak_thin_ptr_eq(
                        rhs.node.weak_state(),
                        &state.weak_self
                    ));
                    rhs
                };

                // TODO: let mut old_rhs = bind.rhs.borrow_mut().replace(rhs.clone());
                let mut old_rhs = Some(rhs.clone());
                {
                    let mut bind_rhs = bind.rhs.borrow_mut();
                    core::mem::swap(&mut *bind_rhs, &mut old_rhs);
                }
                /* Anticipate what [maybe_change_value] will do, to make sure Bind_main is stale
                right away. This way, if the new child is invalid, we'll satisfy the invariant
                saying that [needs_to_be_computed bind_main] in [propagate_invalidity] */
                self.changed_at.set(state.stabilisation_num.get());
                {
                    let main_ = bind.main.borrow();
                    if let Some(main) = main_.upgrade() {
                        main.change_child_bind_rhs(
                            old_rhs.clone(),
                            rhs,
                            Kind::<G>::BIND_RHS_CHILD_INDEX,
                            state,
                        );
                    }
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
                        invalidate_nodes_created_on_rhs(&mut old_all_nodes_created_on_rhs, state)
                    } else {
                        panic!();
                    }
                    state.propagate_invalidity();
                }
                /* [node] was valid at the start of the [Bind_lhs_change] branch, and invalidation
                only visits higher nodes, so [node] is still valid. */
                debug_assert!(self.is_valid());
                self.maybe_change_value(casts.r_unit.cast(()), state)
            }
            Kind::BindMain { casts, bind, .. } => {
                let rhs = bind.rhs.borrow().as_ref().unwrap().clone();
                self.copy_child_bindrhs(&rhs.node, casts.rhs_r, state)
            }
            Kind::ArrayFold(af) => self.maybe_change_value(af.compute(), state),
            Kind::Expert(e) => match e.before_main_computation() {
                Err(Invalid) => {
                    self.invalidate_node(state);
                    state.propagate_invalidity();
                    None
                }
                Ok(()) => {
                    let value = {
                        if let Some(r) = e.recompute.borrow_mut().as_mut() {
                            r()
                        } else {
                            panic!()
                        }
                    };
                    self.maybe_change_value(value, state)
                }
            },
        }
    }

    /// Returns true if we can recompute the parent (self) immediately.
    /// If it returns false, it has already added parent to the RCH.
    fn parent_iter_can_recompute_now(&self, child: &dyn ErasedNode, state: &State) -> bool {
        let parent = self;

        let Some(parent_kind) = parent.kind() else {
            return false;
        };

        let can_recompute_now = match parent_kind {
            // these nodes aren't parents
            Kind::Constant(_) | Kind::Var(_) => panic!(),
            // These nodes have more than one child.
            Kind::ArrayFold(_)
            | Kind::Map2(..)
            | Kind::Map3(..)
            | Kind::Map4(..)
            | Kind::Map5(..)
            | Kind::Map6(..)
            | Kind::Expert(..) => false,
            /* We can immediately recompute [parent] if no other node needs to be stable
            before computing it.  If [parent] has a single child (i.e. [node]), then
            this amounts to checking that [parent] won't be invalidated, i.e. that
            [parent]'s scope has already stabilized. */
            Kind::BindLhsChange { .. } => child.height() > parent.created_in.height(),
            Kind::MapRef(_) | Kind::MapWithOld(_) | Kind::Map(_) => {
                child.height() > parent.created_in.height()
            }
            // | Freeze _ -> node.height > Scope.height parent.created_in
            // | If_test_change _ -> node.height > Scope.height parent.created_in
            // | Join_lhs_change _ -> node.height > Scope.height parent.created_in
            // | Step_function _ -> node.height > Scope.height parent.created_in
            //
            /* For these, we need to check that the "_change" child has already been
            evaluated (if needed).  If so, this also implies:

            {[
            node.height > Scope.height parent.created_in
            ]} */
            Kind::BindMain { lhs_change, .. } => child.height() > lhs_change.height(),
            // | Kind::If_then_else i -> node.height > i.test_change.height
            // | Join_main j -> node.height > j.lhs_change.height
        };
        if can_recompute_now || parent.height() <= state.recompute_heap.min_height() {
            /* If [parent.height] is [<=] the height of all nodes in the recompute heap
            (possibly because the recompute heap is empty), then we can recompute
            [parent] immediately and save adding it to and then removing it from the
            recompute heap. */
            // t.num_nodes_recomputed_directly_because_one_child += 1;
            tracing::info!(
                "can_recompute_now {:?}, proceeding to recompute",
                can_recompute_now
            );
            true
        } else {
            // we already know that !parent.is_in_recompute_heap()
            debug_assert!(parent.needs_to_be_computed());
            debug_assert!(!parent.is_in_recompute_heap());
            tracing::debug!(
                "inserting parent into recompute heap at height {:?}",
                parent.height()
            );
            state.recompute_heap.insert(parent.packed());
            false
        }
    }

    fn has_child(&self, child: &WeakNode) -> bool {
        let Some(upgraded) = child.upgrade() else {
            return false;
        };
        let mut any = false;
        self.foreach_child(&mut |_ix, child| {
            any = any || crate::rc_thin_ptr_eq(&child, &upgraded);
        });
        any
    }

    #[tracing::instrument(skip_all, fields(id = ?self.id))]
    fn invalidate_node(&self, state: &State) {
        if !self.is_valid() {
            return;
        }
        tracing::debug!("invalidating node");
        self.maybe_handle_after_stabilisation(state);
        self.value_opt.take();
        // this was for node-level subscriptions. we don't have those
        // debug_assert!(self.old_value_opt.borrow().is_none());
        self.changed_at.set(state.stabilisation_num.get());
        self.recomputed_at.set(state.stabilisation_num.get());
        state.num_nodes_invalidated.increment();
        if self.is_necessary() {
            self.remove_children(state);
            /* The self doesn't have children anymore, so we can lower its height as much as
            possible, to one greater than the scope it was created in.  Also, because we
            are lowering the height, we don't need to adjust any of its ancestors' heights.
            We could leave the height alone, but we may as well lower it as much as
            possible to avoid making the heights of any future ancestors unnecessarily
            large. */
            let h = self.created_in.height() + 1;
            state.set_height(self.packed(), h);
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
        if let Some(Kind::BindMain { bind, .. }) = self.kind() {
            let mut all = bind.all_nodes_created_on_rhs.borrow_mut();
            invalidate_nodes_created_on_rhs(&mut all, state);
        }
        self.is_valid.set(false);
        let mut prop_stack = state.propagate_invalidity.borrow_mut();
        for parent in self.parents.borrow().iter() {
            let Ok(parent) = parent.upgrade_erased() else {
                continue;
            };
            prop_stack.push(parent.weak());
        }
        drop(prop_stack);
        debug_assert!(!self.needs_to_be_computed());
        if self.is_in_recompute_heap() {
            state.recompute_heap.remove(self.packed());
        }
    }

    fn adjust_heights_bind_lhs_change(
        &self,
        ahh: &mut AdjustHeightsHeap,
        oc: &NodeRef,
        op: &NodeRef,
    ) {
        if let Some(Kind::BindLhsChange { bind, .. }) = self.kind() {
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
    }

    fn created_in(&self) -> Scope {
        self.created_in.clone()
    }
    fn run_on_update_handlers(&self, node_update: NodeUpdateDelayed, now: StabilisationNum) {
        let input = self.as_input();
        let mut ouh = self.on_update_handlers.borrow_mut();
        for handler in ouh.iter_mut() {
            handler.run(self, node_update, now)
        }
        drop(ouh);
        let observers = self.observers.borrow();
        for (_id, obs) in observers.iter() {
            let Some(obs) = obs.upgrade() else { continue };
            obs.run_all(&*input, node_update, now)
        }
        drop(observers);
    }

    #[inline]
    fn maybe_handle_after_stabilisation(&self, state: &State) {
        if self.num_on_update_handlers.get() > 0 {
            self.handle_after_stabilisation(state);
        }
    }

    fn handle_after_stabilisation(&self, state: &State) {
        let is_in_stack = &self.is_in_handle_after_stabilisation;
        if !is_in_stack.get() {
            is_in_stack.set(true);
            let mut stack = state.handle_after_stabilisation.borrow_mut();
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

    fn iter_descendants_internal_one(
        &self,
        seen: &mut HashMap<NodeId, i32>,
        f: &mut dyn FnMut(&NodeRef),
    ) {
        if let std::collections::hash_map::Entry::Vacant(e) = seen.entry(self.id) {
            e.insert(self.height.get());
            f(&self.packed());
            self.foreach_child(&mut |_ix, child| child.iter_descendants_internal_one(seen, f))
        }
    }

    fn dot_label(&self, f: &mut dyn Write) -> fmt::Result {
        let id = self.id;
        if let Some(user) = self.graphviz_user_data.borrow().as_ref() {
            writeln!(f, "{:?}", user)?;
        }
        let h = self.height.get();
        let Some(kind) = self.kind() else {
            return write!(f, "Invalid");
        };
        match kind {
            Kind::Constant(v) => return write!(f, "Constant({id:?}) @ {h} => {v:?}"),
            Kind::ArrayFold(_) => write!(f, "ArrayFold"),
            Kind::Var(_) => write!(f, "Var"),
            Kind::Map(_) => write!(f, "Map"),
            Kind::MapRef(_) => write!(f, "MapRef"),
            Kind::MapWithOld(_) => write!(f, "MapWithOld"),
            Kind::Map2(_) => write!(f, "Map2"),
            Kind::Map3(_) => write!(f, "Map3"),
            Kind::Map4(_) => write!(f, "Map4"),
            Kind::Map5(_) => write!(f, "Map5"),
            Kind::Map6(_) => write!(f, "Map6"),
            Kind::BindLhsChange { .. } => return write!(f, "BindLhsChange({id:?}) @ {h}"),
            Kind::BindMain { .. } => write!(f, "BindMain"),
            Kind::Expert(..) => write!(f, "Expert"),
        }?;
        write!(f, "({id:?})")?;
        write!(f, " @ {h}")?;
        if let Some(val) = self.value_as_ref() {
            write!(f, " => {:#?}", val)?;
        }
        Ok(())
    }

    fn dot_add_bind_edges(&self, bind_edges: &mut Vec<(NodeRef, NodeRef)>) {
        if let Some(Kind::BindLhsChange { bind, .. }) = self.kind() {
            let all = bind.all_nodes_created_on_rhs.borrow();
            for rhs in all.iter().filter_map(Weak::upgrade) {
                bind_edges.push((self.packed(), rhs.clone()));
            }
        }
    }

    fn dot_was_recomputed(&self, state: &State) -> bool {
        let r = state.stabilisation_num.get();
        match state.status.get() {
            IncrStatus::NotStabilising => self.recomputed_at.get().add1() == r,
            _ => self.recomputed_at.get() == r,
        }
    }

    fn dot_was_changed(&self) -> bool {
        let state = self.state();
        let r = state.stabilisation_num.get();
        match state.status.get() {
            IncrStatus::NotStabilising => self.changed_at.get().add1() == r,
            _ => self.changed_at.get() == r,
        }
    }

    fn dot_node(&self, f: &mut dyn Write, name: &str) -> fmt::Result {
        let node = self;
        write!(f, "  {} [", name)?;
        let t = node.state();

        struct EscapedWriter<'a> {
            s: &'a str,
        }
        impl fmt::Display for EscapedWriter<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let mut s = self.s;
                write!(f, "\"")?;
                while !s.is_empty() {
                    let Some(found_esc) = s.find(['"', '\n', '\\']) else {
                        f.write_str(s)?;
                        break;
                    };
                    let b = s.as_bytes();
                    f.write_str(&s[..found_esc])?;
                    match b[found_esc] {
                        // " => \"
                        b'"' => f.write_str("\\\"")?,
                        // newline => \l (left-justified)
                        b'\n' => f.write_str("\\l")?,
                        // \ => \\
                        b'\\' => f.write_str("\\\\")?,
                        _ => return Err(fmt::Error),
                    }
                    s = &s[found_esc + 1..];
                }
                write!(f, "\"")
            }
        }
        write!(f, "label=")?;
        let mut buf = String::new();
        node.dot_label(&mut buf)?;
        write!(f, "{}", EscapedWriter { s: &buf })?;
        if node.is_in_recompute_heap() {
            write!(f, ", fillcolor=3, style=filled")?;
        } else if node.dot_was_recomputed(&t) {
            write!(f, ", fillcolor=6, style=filled")?;
        } else {
            write!(f, ", fillcolor=5, style=filled")?;
        }
        match node.kind() {
            Some(Kind::Var(..)) => {
                write!(f, ", shape=note")?;
            }
            Some(Kind::BindLhsChange { .. }) => {
                write!(f, ", shape=box3d, bgcolor=grey")?;
            }
            _ => {}
        }
        writeln!(f, "]")?;
        Ok(())
    }

    // Expert

    /* Note that the two following functions are not symmetric of one another: in [let y =
    map x], [x] is always a child of [y] (assuming [x] doesn't become invalid) but [y] in
    only a parent of [x] if y is necessary. */
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
        let Some(current) = current.upgrade() else {
            return;
        };
        assert!(
            current.has_child(&self.weak()),
            "({name}) currently running node was not a parent"
        );
    }
    fn expert_make_stale(&self) {
        if !self.is_valid() {
            return;
        }
        let Some(Kind::Expert(expert)) = self.kind() else {
            return;
        };
        #[cfg(debug_assertions)]
        self.assert_currently_running_node_is_child("make_stale");
        match expert.make_stale() {
            MakeStale::AlreadyStale => {}
            MakeStale::Ok => {
                if self.is_necessary() && !self.is_in_recompute_heap() {
                    let t = self.state();
                    t.recompute_heap.insert(self.packed());
                }
            }
        }
    }

    fn expert_add_dependency(&self, packed_edge: PackedEdge) {
        let Some(Kind::Expert(expert)) = self.kind() else {
            return;
        };
        // if debug
        // then
        //   if am_stabilizing state
        //   && not
        //        (List.mem
        //           ~equal:phys_equal
        //           state.only_in_debug.expert_nodes_created_by_current_node
        //           (T node))
        let new_child_index = expert.add_child_edge(packed_edge.clone());
        /* [node] is not guaranteed to be necessary, even if we are running in a child of
        [node], because we could be running due to a parent other than [node] making us
        necessary. */
        if self.is_necessary() {
            let state = self.state();
            let dyn_child = packed_edge.erased_input();
            dyn_child.dyn_state_add_parent(new_child_index, self.as_parent_dyn_ref(), &state);
            debug_assert!(self.needs_to_be_computed());
            if !self.is_in_recompute_heap() {
                state.recompute_heap.insert(self.packed());
                tracing::debug!("dependency ix {new_child_index} inserted into RCH");
            }
        }
    }

    fn expert_remove_dependency(&self, dyn_edge: &dyn IsEdge) {
        let Some(Kind::Expert(expert)) = self.kind() else {
            return;
        };
        #[cfg(debug_assertions)]
        self.assert_currently_running_node_is_child("remove_dependency");
        /* [node] is not guaranteed to be necessary, for the reason stated in
        [add_dependency] */
        let edge_index = dyn_edge.index_cell().get().unwrap();
        let edge_child = dyn_edge.packed();
        let last_edge = expert.last_child_edge().unwrap();
        let last_edge_index = last_edge.index_cell().get().unwrap();
        if edge_index != last_edge_index {
            if self.is_necessary() {
                self.expert_swap_children_except_in_kind(
                    &edge_child,
                    edge_index,
                    &last_edge.packed(),
                    last_edge_index,
                );
            }
            expert.swap_children(edge_index as usize, last_edge_index as usize);
            // if debug then Node.invariant ignore node;
        }
        let popped_edge = expert.pop_child_edge().unwrap();
        debug_assert!(crate::dyn_thin_ptr_eq(&*popped_edge, dyn_edge));

        debug_assert!(self.is_stale());
        if self.is_necessary() {
            let state = self.state();
            self.expert_remove_child(dyn_edge, last_edge_index, &state);
            if !self.is_in_recompute_heap() {
                state.recompute_heap.insert(self.packed());
            }
            if !edge_child.is_valid() {
                expert.decr_invalid_children();
            }
        }
    }

    #[rustfmt::skip]
    fn expert_swap_children_except_in_kind(
        &self,
        child1: &NodeRef,
        child_index1: i32,
        child2: &NodeRef,
        child_index2: i32,
    ) {
        let parent = self;
        debug_assert!(child1.ptr_eq(&*parent.slow_get_child(child_index1)));
        debug_assert!(child2.ptr_eq(&*parent.slow_get_child(child_index2)));

        let parent_pci_ = parent.parent_child_indices();
        let child1_pci_ = child1.parent_child_indices();
        let child2_pci_ = child2.parent_child_indices();
        let mut parent_pci = parent_pci_.borrow_mut();
        let mut child1_pci = child1_pci_.borrow_mut();
        let mut child2_pci = child2_pci_.borrow_mut();

        let index_of_parent_in_child1 = parent_pci.my_parent_index_in_child_at_index[child_index1 as usize];
        let index_of_parent_in_child2 = parent_pci.my_parent_index_in_child_at_index[child_index2 as usize];
        debug_assert_eq!(child1_pci.my_child_index_in_parent_at_index[index_of_parent_in_child1 as usize], child_index1);
        debug_assert_eq!(child2_pci.my_child_index_in_parent_at_index[index_of_parent_in_child2 as usize], child_index2);
        /* now start swapping */
        child1_pci.my_child_index_in_parent_at_index[index_of_parent_in_child1 as usize] = child_index2;
        child2_pci.my_child_index_in_parent_at_index[index_of_parent_in_child2 as usize] = child_index1;
        parent_pci.my_parent_index_in_child_at_index[child_index1 as usize] = index_of_parent_in_child2;
        parent_pci.my_parent_index_in_child_at_index[child_index2 as usize] = index_of_parent_in_child1;
    }

    fn expert_remove_child(&self, dyn_edge: &dyn IsEdge, child_index: i32, state: &State) {
        let child = dyn_edge.erased_input();
        child.dyn_remove_parent(child_index, self);
        child.check_if_unnecessary(state);
    }
}

fn invalidate_nodes_created_on_rhs(all_nodes_created_on_rhs: &mut Vec<WeakNode>, state: &State) {
    tracing::info!("draining all_nodes_created_on_rhs for invalidation");
    for node in all_nodes_created_on_rhs.drain(..) {
        if let Some(node) = node.upgrade() {
            node.invalidate_node(state);
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

    pub fn create(state: Weak<State>, created_in: Scope, kind: Kind<G>) -> Self {
        let t = state.upgrade().unwrap();
        t.num_nodes_created.increment();
        Node {
            id: NodeId::next(),
            weak_self: Weak::<Self>::new(),
            weak_state: state,
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
            // old_value_opt: RefCell::new(None),
            _kind: kind,
            parents: smallvec::smallvec![].into(),
            observers: HashMap::new().into(),
            on_update_handlers: Default::default(),
            graphviz_user_data: None.into(),
            cutoff: Cutoff::PartialEq.into(),
            is_valid: true.into(),
        }
    }

    pub fn create_rc(state: Weak<State>, created_in: Scope, kind: Kind<G>) -> Rc<Self> {
        Node::create(state, created_in, kind).into_rc()
    }

    fn kind(&self) -> Option<&Kind<G>> {
        if !self.is_valid() {
            return None;
        }
        Some(&self._kind)
    }

    fn maybe_change_value(&self, value: G::R, state: &State) -> Option<NodeRef> {
        let old_value_opt = self.value_opt.take();
        let mut cutoff = self.cutoff.borrow_mut();
        let should_change = old_value_opt
            .as_ref()
            .map_or(true, |old| !cutoff.should_cutoff(old, &value));
        return self.maybe_change_value_manual(
            old_value_opt.as_ref(),
            Some(value),
            should_change,
            true,
            state,
        );
    }

    fn maybe_change_value_manual(
        &self,
        old_value_opt: Option<&G::R>,
        new_value_opt: Option<G::R>,
        did_change: bool,
        run_child_changed: bool,
        state: &State,
    ) -> Option<NodeRef> {
        if did_change {
            self.value_opt.replace(new_value_opt);
            self.changed_at.set(state.stabilisation_num.get());
            state.num_nodes_changed.increment();
            self.maybe_handle_after_stabilisation(state);
            let parents = self.parents.borrow();
            let mut parents_iter = parents.iter().enumerate();
            let pci = self.parent_child_indices.borrow();
            // steal the first parent
            let first_parent = parents_iter.next();
            for (parent_index, parent) in parents_iter {
                let child_index = pci.my_child_index_in_parent_at_index[parent_index];
                let result = if run_child_changed {
                    parent.child_changed(self, child_index, old_value_opt)
                } else {
                    parent.upgrade_erased()
                };
                let p = match result {
                    Ok(p) => p,
                    // FIXME: refactor kept previous behaviour, but this should probably
                    // be an error
                    Err(ParentError::ParentDeallocated) => continue,
                    // TODO: handle this somehow
                    _ => result.unwrap(),
                };
                debug_assert!(
                    p.needs_to_be_computed(),
                    "p.needs_to_be_computed(): {:?}",
                    p
                );
                /* We don't do the [can_recompute_now] optimization.  Since most nodes only have
                one parent, it is not probably not a big loss.  If we did it anyway, we'd
                have to be careful, because while we iterate over the list of parents, we
                would execute them, and in particular we can execute lhs-change nodes who can
                change the structure of the list of parents we iterate on.  Think about:

                {[
                lhs >>= fun b -> if b then lhs >>| Fn.id else const b
                aka
                lhs.binds(|incr, b| if b { lhs.map(id) } else { incr.constant(b) })
                ]}

                If the optimization kicks in when we propagate change to the parents of [lhs]
                (which changes from [true] to [false]), we could execute the [lhs-change]
                first, which would make disconnect the [map] node from [lhs].  And then we
                would execute the second child of the [lhs], which doesn't exist anymore and
                incremental would segfault (there may be a less naive way of making this work
                though). */
                if !p.is_in_recompute_heap() {
                    tracing::debug!(
                        "inserting parent into recompute heap at height {:?}",
                        p.height()
                    );
                    state.recompute_heap.insert(p.packed());
                }
            }
            if let Some((parent_index, parent)) = first_parent {
                let child_index = pci.my_child_index_in_parent_at_index[parent_index];
                // a bit of a dance to avoid upgrading the Weak pointer twice.
                let result = if run_child_changed {
                    parent.child_changed(self, child_index, old_value_opt)
                } else {
                    parent.upgrade_erased()
                };
                let p = match result {
                    Ok(p) => p,
                    // FIXME: refactor kept previous behaviour, but this should probably
                    // be an error
                    Err(ParentError::ParentDeallocated) => return None,
                    // TODO: handle this somehow
                    _ => result.unwrap(),
                };
                debug_assert!(
                    p.needs_to_be_computed(),
                    "p.needs_to_be_computed(): {:?}",
                    p
                );
                if !p.is_in_recompute_heap() && p.parent_iter_can_recompute_now(self, state) {
                    return Some(p);
                }
            }
        } else {
            tracing::info!("cutoff applied to value change");
            self.value_opt.replace(new_value_opt);
        }
        None
    }

    #[tracing::instrument(skip_all, fields(bind_main = ?self.id))]
    fn change_child_bind_rhs(
        &self,
        old_child: Option<Incr<G::BindRhs>>,
        new_child: Incr<G::BindRhs>,
        child_index: i32,
        state: &State,
    ) {
        let bind_main = self;
        let Some(Kind::BindMain { casts: id, .. }) = bind_main.kind() else {
            return;
        };
        let new_child_node = id.input_rhs_i1.cast_ref(&new_child.node);
        match old_child {
            None => {
                tracing::debug!(
                    "change_child simply adding parent to {:?} at child_index {child_index}",
                    new_child_node
                );
                new_child_node.state_add_parent(child_index, bind_main.as_parent_ref(), state);
            }
            Some(old_child) => {
                // ptr_eq is better than ID checking -- no vtable call,
                if old_child.ptr_eq(&new_child) {
                    // nothing to do! nothing changed!
                    return;
                }
                let old_child_node = id.input_rhs_i1.cast_ref(&old_child.node);
                /* We remove [old_child] before adding [new_child], because they share the same
                child index. */
                old_child_node.remove_parent(child_index, bind_main.as_parent_ref());
                /* We force [old_child] to temporarily be necessary so that [add_parent] can't
                mistakenly think it is unnecessary and transition it to necessary (which would
                add duplicate edges and break things horribly). */
                old_child_node.force_necessary().set(true);
                new_child_node.state_add_parent(child_index, bind_main.as_parent_ref(), state);
                old_child_node.force_necessary().set(false);
                /* We [check_if_unnecessary] after [add_parent], so that we don't unnecessarily
                transition nodes from necessary to unnecessary and then back again. */
                old_child_node.check_if_unnecessary(state);
            }
        }
    }

    fn copy_child_bindrhs(
        &self,
        child: &Input<G::BindRhs>,
        token: Id<G::BindRhs, G::R>,
        state: &State,
    ) -> Option<NodeRef> {
        if child.is_valid() {
            let latest = child.latest();
            self.maybe_change_value(token.cast(latest), state)
        } else {
            self.invalidate_node(state);
            state.propagate_invalidity();
            None
        }
    }

    fn add_parent(&self, child_index: i32, parent_ref: ParentRef<G::R>) {
        let child = self;
        let mut child_indices = child.parent_child_indices.borrow_mut();
        let mut child_parents = child.parents.borrow_mut();

        // we're appending here
        let parent = parent_ref.erased();
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

        child_parents.push(parent_ref.weak());
    }

    fn as_parent_weak(&self) -> ParentWeak<G::I1> {
        ParentWeak::Input1(self.weak_self.clone())
    }
    fn as_parent_ref(&self) -> ParentRef<G::I1> {
        ParentRef::I1(self)
    }
    fn as_parent2_weak(&self) -> ParentWeak<G::I2> {
        ParentWeak::Input2(self.weak_self.clone())
    }
    fn as_parent2_ref(&self) -> ParentRef<G::I2> {
        ParentRef::I2(self)
    }
    fn as_parent_dyn_ref(&self) -> &dyn ParentNodeDyn {
        self
    }
    fn foreach_child_typed<'b>(&'b self, f: ForeachChild<'b, G>) {
        let Some(kind) = self.kind() else { return };
        match kind {
            Kind::Constant(_) => {}
            Kind::Map(kind::MapNode { input, .. })
            | Kind::MapRef(kind::MapRefNode { input, .. })
            | Kind::MapWithOld(kind::MapWithOld { input, .. }) => {
                (f.i1)(0, input.clone().as_input())
            }
            Kind::Map2(kind::Map2Node { one, two, .. }) => {
                (f.i1)(0, one.clone().as_input());
                (f.i2)(1, two.clone().as_input());
            }
            Kind::Map3(kind::Map3Node {
                one, two, three, ..
            }) => {
                (f.idyn)(0, one.clone().erased_input());
                (f.idyn)(1, two.clone().erased_input());
                (f.idyn)(2, three.clone().erased_input());
            }
            Kind::Map4(kind::Map4Node {
                one,
                two,
                three,
                four,
                ..
            }) => {
                (f.idyn)(0, one.clone().erased_input());
                (f.idyn)(1, two.clone().erased_input());
                (f.idyn)(2, three.clone().erased_input());
                (f.idyn)(3, four.clone().erased_input());
            }
            Kind::Map5(kind::Map5Node {
                one,
                two,
                three,
                four,
                five,
                ..
            }) => {
                (f.idyn)(0, one.clone().erased_input());
                (f.idyn)(1, two.clone().erased_input());
                (f.idyn)(2, three.clone().erased_input());
                (f.idyn)(3, four.clone().erased_input());
                (f.idyn)(4, five.clone().erased_input());
            }
            Kind::Map6(kind::Map6Node {
                one,
                two,
                three,
                four,
                five,
                six,
                ..
            }) => {
                (f.idyn)(0, one.clone().erased_input());
                (f.idyn)(1, two.clone().erased_input());
                (f.idyn)(2, three.clone().erased_input());
                (f.idyn)(3, four.clone().erased_input());
                (f.idyn)(4, five.clone().erased_input());
                (f.idyn)(5, six.clone().erased_input());
            }
            Kind::BindLhsChange { casts, bind } => {
                let input = casts.input_lhs_i2.cast(bind.lhs.as_input());
                (f.i2)(0, input)
            }
            Kind::BindMain {
                casts,
                bind,
                lhs_change,
            } => {
                let input = casts.input_lhs_i2.cast(lhs_change.as_input());
                (f.i2)(0, input);
                if let Some(rhs) = bind.rhs.borrow().as_ref() {
                    let input = casts.input_rhs_i1.cast(rhs.node.as_input());
                    (f.i1)(1, input)
                }
            }
            Kind::ArrayFold(af) => {
                for (ix, child) in af.children.iter().enumerate() {
                    (f.i1)(ix as i32, child.node.as_input())
                }
            }
            Kind::Var(_var) => {}
            Kind::Expert(e) => {
                let borrow_span = tracing::debug_span!("expert.children.borrow() in foreach_child");
                borrow_span.in_scope(|| {
                    for (ix, child) in e.children.borrow().iter().enumerate() {
                        (f.idyn)(ix as i32, child.erased_input());
                    }
                });
            }
        }
    }

    fn remove_children(&self, state: &State) {
        self.foreach_child_typed(ForeachChild {
            i1: &mut |index, child| {
                child.remove_parent(index, self.as_parent_ref());
                child.check_if_unnecessary(state);
            },
            i2: &mut |index, child| {
                child.remove_parent(index, self.as_parent2_ref());
                child.check_if_unnecessary(state);
            },
            idyn: &mut |index, child| {
                child.dyn_remove_parent(index, self.as_parent_dyn_ref());
                child.check_if_unnecessary(state);
            },
        })
    }

    /// Only used for debug assertions. It's okay to be slow.
    fn slow_get_child(&self, child_index: i32) -> NodeRef {
        let Some(kind) = self.kind() else { panic!() };
        match kind {
            Kind::Expert(e) => e.children.borrow()[child_index as usize].packed(),
            Kind::ArrayFold(af) => af.children[child_index as usize].node.packed(),
            _ => {
                let mut found = None;
                self.foreach_child(&mut |ix, child| {
                    if ix == child_index {
                        found.replace(child);
                    }
                });
                found.unwrap()
            }
        }
    }
}

struct ForeachChild<'a, G: NodeGenerics> {
    // TODO: make these &dyn Incremental<G::I1> instead of a cloned Rc
    i1: &'a mut dyn FnMut(i32, Input<G::I1>),
    i2: &'a mut dyn FnMut(i32, Input<G::I2>),
    idyn: &'a mut dyn FnMut(i32, &dyn ErasedIncremental),
}

#[test]
#[ignore = "changes a lot these days"]
fn test_node_size() {
    use super::kind::Constant;
    let state = State::new();
    let node =
        Node::<Constant<i32>>::create_rc(state.weak(), state.current_scope(), Kind::Constant(5i32));
    assert_eq!(core::mem::size_of_val(&*node), 408);
}

fn iter_descendants_internal(
    i: &mut dyn Iterator<Item = &dyn ErasedNode>,
    f: &mut dyn FnMut(&NodeRef),
) -> HashMap<NodeId, i32> {
    let mut seen = HashMap::new();
    for node in i {
        node.iter_descendants_internal_one(&mut seen, f);
    }
    seen
}

pub(crate) fn save_dot_to_file(
    nodes: &mut dyn Iterator<Item = &dyn ErasedNode>,
    named: &str,
) -> std::io::Result<()> {
    let buf = &mut String::new();
    save_dot(buf, nodes).unwrap();

    use std::fs::File;
    use std::io::Write;

    let mut file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(named)
        .unwrap();
    file.write_all(buf.as_bytes())
}

pub(crate) fn save_dot(
    f: &mut dyn Write,
    nodes: &mut dyn Iterator<Item = &dyn ErasedNode>,
) -> fmt::Result {
    fn node_name(node: &NodeRef) -> String {
        node.id().0.to_string()
    }
    writeln!(f, "digraph G {{")?;
    writeln!(
        f,
        r#"rankdir = BT
        graph [fontname = "Courier"];
        node [fontname = "Courier", shape=box, colorscheme=rdylbu7];
        edge [fontname = "Courier", colorscheme=rdylbu7];"#
    )?;
    let mut bind_edges = vec![];
    let seen = iter_descendants_internal(nodes, &mut |node| {
        let name = node_name(node);
        node.dot_node(f, &name).unwrap();
        node.foreach_child(&mut |_, child| {
            writeln!(f, "  {} -> {}", node_name(&child.packed()), name).unwrap();
            if child.dot_was_changed() {
                write!(f, " [color=1]").unwrap();
            }
            writeln!(f).unwrap();
        });
        node.dot_add_bind_edges(&mut bind_edges);
    });
    for (bind, rhs) in bind_edges {
        if seen.contains_key(&rhs.id()) {
            writeln!(
                f,
                "  {} -> {} [style=dashed{}]",
                node_name(&bind),
                node_name(&rhs),
                if bind.dot_was_changed() {
                    ", color=2"
                } else {
                    ""
                }
            )?;
        }
    }
    let mut by_height: HashMap<i32, Vec<NodeId>> = HashMap::new();
    let mut min_height = i32::MAX;
    for (node_id, height) in seen {
        by_height.entry(height).or_default().push(node_id);
        min_height = min_height.min(height);
    }
    for (height, nodes) in by_height {
        let rank = if height == min_height { "min" } else { "same" };
        writeln!(f, "{{ rank={:?}; ", rank)?;
        for id in nodes {
            writeln!(f, "{};", id.0)?;
        }
        writeln!(f, "}}")?;
    }
    writeln!(f, "}}")?;
    Ok(())
}

#[cfg(debug_assertions)]
impl<G: NodeGenerics> Drop for Node<G> {
    fn drop(&mut self) {
        tracing::trace!("dropping Node: {:?}", self);
    }
}

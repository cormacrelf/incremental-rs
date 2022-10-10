// use enum_dispatch::enum_dispatch;

use super::adjust_heights_heap::{AdjustHeightsHeap, NodeRef};
use super::internal_observer::ErasedObserver;
use super::scope::Scope;
use super::{state::State, var::Var};
use super::{BindNode, CutoffNode, Incr, Map2Node, MapNode};
use core::fmt::Debug;

use super::stabilisation_num::StabilisationNum;
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

#[derive(Debug, Copy, Clone)]
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
pub(crate) struct NodeInner {
    pub state: Rc<State>,
    // cutoff
    // num_on_update_handlers
    // TODO: optimise the one parent case
    // pub(crate) num_parents: i32,
    // parent0
    // parent1_and_beyond
    pub(crate) my_parent_index_in_child_at_index: Vec<i32>,
    pub(crate) my_child_index_in_parent_at_index: Vec<i32>,
    // next_node_in_same_scope
    pub(crate) prev_in_recompute_heap: Option<PackedNode>,
    pub(crate) next_in_recompute_heap: Option<PackedNode>,
    pub(crate) force_necessary: bool,
    pub(crate) parents: Vec<Option<WeakNode>>,
    pub(crate) observers: Vec<Weak<dyn ErasedObserver>>,
}

pub(crate) struct Node<G: NodeGenerics> {
    pub(crate) id: NodeId,
    pub(crate) inner: RefCell<NodeInner>,
    pub(crate) kind: RefCell<Kind<G>>,
    pub(crate) value_opt: RefCell<Option<G::R>>,
    pub(crate) old_value_opt: RefCell<Option<G::R>>,
    pub height: Cell<i32>,
    pub old_height: Cell<i32>,
    pub height_in_recompute_heap: Cell<i32>,
    pub height_in_adjust_heights_heap: Cell<i32>,
    pub(crate) created_in: Scope,
    pub recomputed_at: Cell<StabilisationNum>,
    pub changed_at: Cell<StabilisationNum>,
    pub(crate) weak_self: Weak<dyn ErasedNode>,
}

pub(crate) type Input<R> = Rc<dyn Incremental<R>>;

pub(crate) trait Incremental<R>: ErasedNode + Debug {
    fn as_input(self: Rc<Self>) -> Input<R>;
    fn latest(&self) -> R;
    fn value_opt(&self) -> Option<R>;
}

impl<G: NodeGenerics + 'static> Incremental<G::R> for Node<G> {
    fn as_input(self: Rc<Self>) -> Input<G::R> {
        self as Input<G::R>
    }
    fn latest(&self) -> G::R {
        self.value_opt.borrow().clone().unwrap()
    }
    fn value_opt(&self) -> Option<G::R> {
        self.value_opt.borrow().clone()
    }
}

pub(crate) type PackedNode = Rc<dyn ErasedNode>;
pub(crate) type WeakNode = Weak<dyn ErasedNode>;

// pub type ParentNode<I> = Weak<dyn ParentNode<I>>;
// pub trait ParentNode<I>: ErasedNode {
//     fn child_changed(&self, child: Weak<dyn ErasedNode>, child_index: i32);
// }

pub(crate) trait ErasedNode: Debug {
    fn is_valid(&self) -> bool;
    fn height(&self) -> i32;
    fn height_in_recompute_heap(&self) -> &Cell<i32>;
    fn height_in_adjust_heights_heap(&self) -> &Cell<i32>;
    fn set_height(&self, height: i32);
    fn old_height(&self) -> i32;
    fn set_old_height(&self, height: i32);
    fn add_parent_without_adjusting_heights(&self, child_index: i32, parent_weak: WeakNode);
    fn state_add_parent(&self, child_index: i32, parent_weak: WeakNode);
    fn is_stale(&self) -> bool;
    fn is_stale_with_respect_to_a_child(&self) -> bool;
    fn edge_is_stale(&self, parent: WeakNode) -> bool;
    fn is_necessary(&self) -> bool;
    fn needs_to_be_recomputed(&self) -> bool;
    fn became_necessary(&self);
    fn became_unnecessary(&self);
    fn remove_children(&self);
    fn remove_child(&self, child: PackedNode, child_index: i32);
    fn check_if_unnecessary(&self);
    fn is_in_recompute_heap(&self) -> bool;
    fn recompute(&self);
    fn inner(&self) -> &RefCell<NodeInner>;
    fn state(&self) -> Rc<State>;
    fn weak(&self) -> Weak<dyn ErasedNode>;
    fn packed(&self) -> Rc<dyn ErasedNode>;
    fn foreach_child(&self, f: &mut dyn FnMut(i32, PackedNode) -> ());
    fn recomputed_at(&self) -> &Cell<StabilisationNum>;
    fn invalidate(&self);
    fn adjust_heights_bind_lhs_change(
        &self,
        ahh: &mut AdjustHeightsHeap,
        oc: &NodeRef,
        op: &NodeRef,
    );
    fn created_in(&self) -> Scope;
}

impl NodeInner {
    fn is_necessary(&self) -> bool {
        self.num_parents() > 0
            || !self.observers.is_empty()
            // || kind is freeze
            || self.force_necessary
    }
    pub(crate) fn parents(&self) -> &[Option<WeakNode>] {
        &self.parents[..]
    }
    pub(crate) fn num_parents(&self) -> usize {
        self.parents.len()
    }
    fn remove_parent(&mut self, child_index: i32, parent_weak: WeakNode) {
        // println!("remove_parent {self:?}, ix {child_index}");
        let parent = parent_weak.upgrade().unwrap();
        let parent_inner = parent.inner();
        let mut parent_i = parent_inner.borrow_mut();

        let child = self;
        debug_assert!(child.num_parents() >= 1);
        let parent_index = parent_i.my_parent_index_in_child_at_index[child_index as usize];
        debug_assert!(parent_weak.ptr_eq(&child.parents[parent_index as usize].clone().unwrap()));
        let last_parent_index = child.num_parents() - 1;
        if (parent_index as usize) < last_parent_index {
            // we swap the parent the end of the array into this one's position. This keeps the array
            // small.
            let end_p_weak = child.parents[last_parent_index].clone().unwrap();
            if let Some(end_p) = end_p_weak.upgrade() {
                let end_p_inner = end_p.inner();
                let mut end_p_i = end_p_inner.borrow_mut();
                let end_child_index = child.my_child_index_in_parent_at_index[last_parent_index];
                // link parent_index & end_child_index
                end_p_i.my_parent_index_in_child_at_index[end_child_index as usize] = parent_index;
                child.my_child_index_in_parent_at_index[parent_index as usize] = end_child_index;
            }
            // unlink last_parent_index & child_index
            parent_i.my_parent_index_in_child_at_index[child_index as usize] = -1;
            child.my_child_index_in_parent_at_index[last_parent_index] = -1;
            // now do what we just did but super easily in the actual Vec
            child.parents.swap_remove(parent_index as usize);
        }
    }
    fn add_parent(&mut self, child_index: i32, parent_weak: WeakNode) {
        let child = self;
        // we're appending here
        let parent_index = child.parents.len() as i32;
        let parent = parent_weak.upgrade().unwrap();
        let parent_inner = parent.inner();
        let mut parent_i = parent_inner.borrow_mut();

        while child.my_child_index_in_parent_at_index.len() <= parent_index as usize {
            child.my_child_index_in_parent_at_index.push(-1);
        }
        println!("ci_in_pia[{parent_index}] = {child_index}");
        child.my_child_index_in_parent_at_index[parent_index as usize] = child_index;

        while parent_i.my_parent_index_in_child_at_index.len() <= child_index as usize {
            parent_i.my_parent_index_in_child_at_index.push(-1);
        }
        println!("pi_in_cia[{child_index}] = {parent_index}");
        parent_i.my_parent_index_in_child_at_index[child_index as usize] = parent_index;

        child.parents.push(Some(parent_weak));
    }
}

impl<G: NodeGenerics + 'static> Debug for Node<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("id", &self.id)
            .field("value_opt", &self.value_opt.borrow())
            .field("height", &self.height.get())
            .field("kind", &self.kind.borrow())
            // .field("inner", &self.inner)
            .finish()
    }
}
impl<G: NodeGenerics + 'static> ErasedNode for Node<G> {
    fn weak(&self) -> Weak<dyn ErasedNode> {
        self.weak_self.clone() as Weak<dyn ErasedNode>
    }
    fn packed(&self) -> PackedNode {
        let Some(strong) = self.weak_self.upgrade() else {
            panic!("Node not initialised properly (did not call into_rc())");
        };
        strong as Rc<dyn ErasedNode>
    }
    fn inner(&self) -> &RefCell<NodeInner> {
        &self.inner
    }
    fn state(&self) -> Rc<State> {
        self.inner().borrow().state.clone()
    }
    fn is_valid(&self) -> bool {
        let k = self.kind.borrow();
        match &*k {
            Kind::Invalid => false,
            _ => true,
        }
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
    fn set_height(&self, height: i32) {
        println!("HHHHHHHHH node id={:?}, set height to {height}", self.id);
        // TODO: checks
        self.height.set(height);
    }
    fn old_height(&self) -> i32 {
        self.old_height.get()
    }
    fn set_old_height(&self, height: i32) {
        self.old_height.set(height);
    }
    fn add_parent_without_adjusting_heights(&self, child_index: i32, parent_weak: WeakNode) {
        println!("add_parent_without_adjusting_heights");
        debug_assert!({
            let Some(p) = parent_weak.upgrade() else { panic!() };
            p.is_necessary()
        });
        let was_necessary = self.is_necessary();
        let mut child_i = self.inner.borrow_mut();
        child_i.add_parent(child_index, parent_weak);
        drop(child_i);
        if !self.is_valid() {
            println!("TODO: propagate_invalidity");
            // child_i.state.propagate_invalidity.push(parent);
        }
        if !was_necessary {
            self.became_necessary();
        }
    }
    fn state_add_parent(&self, child_index: i32, parent_weak: WeakNode) {
        let parent = parent_weak.upgrade().unwrap();
        debug_assert!(parent.is_necessary());
        self.add_parent_without_adjusting_heights(child_index, parent_weak.clone());
        if self.height() >= parent.height() {
            // This happens when change_child whacks a neew child in
            // What's happening here is that
            println!(
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
            && (parent.recomputed_at().get().is_none() || self.edge_is_stale(parent_weak.clone()))
        {
            let t = self.state();
            let mut rch = t.recompute_heap.borrow_mut();
            rch.insert(parent);
        }
    }
    fn is_stale(&self) -> bool {
        let k = self.kind.borrow();
        match &*k {
            Kind::Invalid => panic!(),
            Kind::Var(var) => {
                let set_at = var.set_at.get();
                let recomputed_at = self.recomputed_at.get();
                set_at > recomputed_at
            }
            Kind::Map(_) | Kind::Map2(_) | Kind::BindLhsChange(_) | Kind::BindMain(_) => {
                self.recomputed_at.get() == StabilisationNum(-1)
                    || self.is_stale_with_respect_to_a_child()
            }
            // wrong
            _ => false,
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
        let i = self.inner().borrow();
        i.is_necessary() // || kind is freeze
    }
    fn needs_to_be_recomputed(&self) -> bool {
        self.is_necessary() && self.is_stale()
    }
    fn became_necessary(&self) {
        println!("became_necessary: {:?}", self);
        if self.is_valid() && !self.created_in.is_necessary() {
            panic!("trying to make a node necessary whose defining bind is not necessary");
        }
        let t = self.state();
        t.num_nodes_became_necessary
            .set(t.num_nodes_became_necessary.get() + 1);
        // if node.num_on_update_handlers > 0 then handle_after_stabilization node;
        /* Since [node] became necessary, to restore the invariant, we need to:
        - add parent pointers to [node] from its children.
        - set [node]'s height.
        - add [node] to the recompute heap, if necessary. */
        let weak = self.clone().weak();
        let mut h = {
            self.set_height(self.created_in.height() + 1);
            self.height()
        };
        self.foreach_child(&mut |index, child| {
            child.add_parent_without_adjusting_heights(index, weak.clone());
            {
                if child.height() >= h {
                    h = child.height() + 1;
                }
            }
        });
        self.set_height(h);
        debug_assert!(!self.is_in_recompute_heap());
        debug_assert!(self.is_necessary());
        if self.is_stale() {
            let mut rch = t.recompute_heap.borrow_mut();
            rch.insert(self.packed());
            println!("===> added self(id={:?}) to rch", self.id);
        }
        println!("became_necessary(post): {:?}", self);
    }
    fn check_if_unnecessary(&self) {
        if !self.is_necessary() {
            self.became_unnecessary();
        }
    }
    fn became_unnecessary(&self) {
        println!("became_unnecessary: {:?}", self);
        let t = self.state();
        t.num_nodes_became_unnecessary
            .set(t.num_nodes_became_unnecessary.get() + 1);
        // if node.num_on_update_handlers > 0 then handle_after_stabilization node;
        self.set_height(-1);
        self.remove_children();
        /* (match node.kind with
        | Unordered_array_fold u -> Unordered_array_fold.force_full_compute u
        | Expert p -> Expert.observability_change p ~is_now_observable:false
        | _ -> ()); */
        debug_assert!(!self.needs_to_be_recomputed());
        if self.is_in_recompute_heap() {
            let t = self.state();
            let mut rch = t.recompute_heap.borrow_mut();
            rch.remove(self.packed());
        }
    }
    fn remove_child(&self, child: PackedNode, child_index: i32) {
        {
            let child_inner = child.inner();
            let mut ci = child_inner.borrow_mut();
            ci.remove_parent(child_index, self.weak());
        }
        child.check_if_unnecessary();
    }
    fn remove_children(&self) {
        self.foreach_child(&mut |ix, child| self.remove_child(child, ix));
    }
    fn foreach_child(&self, f: &mut dyn FnMut(i32, PackedNode) -> ()) {
        let k = self.kind.borrow();
        match &*k {
            Kind::Invalid => {}
            Kind::Uninitialised => {}
            Kind::Map(MapNode { input, .. }) => f(0, input.packed()),
            Kind::Map2(Map2Node { one, two, .. }) => {
                f(0, one.clone().packed());
                f(1, two.clone().packed());
            }
            Kind::BindLhsChange(bind) => f(0, bind.lhs.packed()),
            Kind::BindMain(bind) => {
                f(0, bind.lhs_change.packed());
                if let Some(rhs) = bind.rhs.borrow().as_ref() {
                    f(1, rhs.node.packed())
                }
            }
            Kind::Var(var) => {}
            Kind::Cutoff(CutoffNode { input, .. }) => f(0, input.clone().packed()),
        }
    }
    fn is_in_recompute_heap(&self) -> bool {
        self.height_in_recompute_heap.get() >= 0
    }
    fn recomputed_at(&self) -> &Cell<StabilisationNum> {
        &self.recomputed_at
    }
    fn recompute(&self) {
        let t = self.inner.borrow().state.clone();
        t.num_nodes_recomputed.set(t.num_nodes_recomputed.get() + 1);
        self.recomputed_at.set(t.stabilisation_num.get());
        let mut k = self.kind.borrow_mut();
        let id = self.id;
        let height = self.height();
        match &mut *k {
            Kind::Var(var) => {
                let value = var.value.borrow();
                let v = value.clone();
                println!("-- recomputing Var(id={id:?}) <- {v:?}");
                drop(value);
                self.maybe_change_value(v);
            }
            Kind::Map(map) => {
                let map: &mut MapNode<G::F1, G::I1, G::R> = map;
                let input = map.input.latest();
                let new_value = (map.mapper)(input);
                println!("-- recomputing Map(id={id:?}) <- {new_value:?}");
                self.maybe_change_value(new_value);
            }
            Kind::Map2(map2) => {
                let map2: &Map2Node<G::F2, G::I1, G::I2, G::R> = map2;
                let i1 = map2.one.latest();
                let i2 = map2.two.latest();
                let new_value = (map2.mapper)(i1, i2);
                println!("-- recomputing Map2(id={id:?}) <- {new_value:?}");
                self.maybe_change_value(new_value);
            }
            Kind::BindLhsChange(bind) => {
                let bind: &mut Rc<BindNode<G::B1, G::I1, G::R>> = bind;
                // leaves an empty vec for next time
                let mut old_all_nodes_created_on_rhs = bind.all_nodes_created_on_rhs.take();
                let lhs = bind.lhs.latest();
                let rhs = {
                    let old_scope = self.state().current_scope();
                    *self.state().current_scope.borrow_mut() = bind.rhs_scope.borrow().clone();
                    println!("-- recomputing BindLhsChange(id={id:?}, {lhs:?})");
                    let rhs = (bind.mapper)(lhs);
                    *self.state().current_scope.borrow_mut() = old_scope;
                    println!("-- recomputing BindLhsChange(id={id:?}) <- {rhs:?}");
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
                    main.change_child(old_rhs.clone(), rhs, Kind::<G>::BIND_RHS_CHILD_INDEX);
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
                }
                /* [node] was valid at the start of the [Bind_lhs_change] branch, and invalidation
                only visits higher nodes, so [node] is still valid. */
                // debug_assert!(self.is_valid());
                self.maybe_change_value_opt(None);
            }
            Kind::BindMain(bind) => {
                let rhs = bind.rhs.borrow().as_ref().unwrap().clone();
                println!("-- recomputing BindMain(id={id:?}, h={height:?}) <- {rhs:?}");
                self.copy_child(rhs.node);
            }
            Kind::Invalid => panic!("should not have Kind::Invalid nodes in the recompute heap"),
            _ => {}
        }
    }
    fn invalidate(&self) {
        if self.is_valid() {
            let t = self.state();
            // if node.num_on_update_handlers > 0 then handle_after_stabilization node;
            *self.value_opt.borrow_mut() = None;
            debug_assert!(self.old_value_opt.borrow().is_none());
            self.changed_at.set(t.stabilisation_num.get());
            self.recomputed_at.set(t.stabilisation_num.get());
            t.num_nodes_invalidated
                .set(t.num_nodes_invalidated.get() + 1);
            if self.is_necessary() {
                self.remove_children();
                /* The node doesn't have children anymore, so we can lower its height as much as
                possible, to one greater than the scope it was created in.  Also, because we
                are lowering the height, we don't need to adjust any of its ancestors' heights.
                We could leave the height alone, but we may as well lower it as much as
                possible to avoid making the heights of any future ancestors unnecessarily
                large. */
                let h = self.created_in.height() + 1;
                self.set_height(h);
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
                Kind::BindMain(bind) => {
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
        ahh: &mut AdjustHeightsHeap,
        oc: &NodeRef,
        op: &NodeRef,
    ) {
        let kind = self.kind.borrow();
        match &*kind {
            Kind::BindLhsChange(bind) => {
                println!("adjust_heights_bind_lhs_change {:?}", bind);
                let all = bind.all_nodes_created_on_rhs.borrow();
                for rnode_weak in all.iter() {
                    let rnode = rnode_weak.upgrade().unwrap();
                    println!("---- all_nodes_created_on_rhs: {:?}", rnode);
                    if rnode.is_necessary() {
                        ahh.ensure_height_requirement(oc, op, &self.packed(), &rnode)
                    }
                }
            }
            _ => {}
        }
    }

    fn created_in(&self) -> Scope {
        self.created_in.clone()
    }
}

fn invalidate_nodes_created_on_rhs(all_nodes_created_on_rhs: &mut Vec<WeakNode>) {
    for node in all_nodes_created_on_rhs.drain(..) {
        invalidate_node(node);
    }
}
fn invalidate_node(node: WeakNode) {
    let Some(node) = node.upgrade() else { return };
    node.invalidate();
}

impl<G: NodeGenerics + 'static> Node<G> {
    pub fn into_rc(mut self) -> Rc<Self> {
        let rc = Rc::<Self>::new_cyclic(|weak| {
            self.weak_self = weak.clone();
            self
        });
        rc.created_in.add_node(rc.clone());
        rc
    }
    pub fn create(state: Rc<State>, created_in: Scope, kind: Kind<G>) -> Self {
        Node {
            id: NodeId::next(),
            weak_self: Weak::<Self>::new(),
            inner: RefCell::new(NodeInner {
                state,
                prev_in_recompute_heap: None,
                next_in_recompute_heap: None,
                observers: Default::default(),
                parents: vec![],
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
            recomputed_at: Cell::new(StabilisationNum::init()),
            value_opt: RefCell::new(None),
            old_value_opt: RefCell::new(None),
            kind: RefCell::new(kind),
        }
    }
    fn value(&self) -> G::R {
        self.value_opt.borrow().clone().unwrap()
    }
    fn maybe_change_value(&self, value: G::R) {
        self.maybe_change_value_opt(Some(value));
    }
    /// BindLhsChange doesn't want a Default bound. So we let ourselves set value_opt to None.
    /// Don't abuse this privilege! We make sure we don't read the bind lhs node's value.
    fn maybe_change_value_opt(&self, value: Option<G::R>) {
        self.old_value_opt.replace(None);
        self.old_value_opt.swap(&self.value_opt);
        self.value_opt.replace(value);
        let inner = self.inner.borrow();
        self.changed_at.set(inner.state.stabilisation_num.get());
        inner
            .state
            .num_nodes_changed
            .set(inner.state.num_nodes_changed.get() + 1);
        /* if node.num_on_update_handlers > 0
        then (
        node.old_value_opt <- old_value_opt;
        handle_after_stabilization node); */
        let i = self.inner.borrow();
        if i.num_parents() >= 1 {
            for (parent_index, parent) in i
                .parents()
                .iter()
                .enumerate()
                .filter_map(|(i, x)| x.as_ref().and_then(|a| a.upgrade()).map(|a| (i, a)))
            {
                if !parent.is_in_recompute_heap() {
                    println!(
                        "inserting parent into recompute heap at height {:?}",
                        parent.height()
                    );
                    let t = self.state();
                    let mut rch = t.recompute_heap.borrow_mut();
                    rch.insert(parent.packed());
                }
            }
        }
    }
    fn change_child<T: Clone + Debug + 'static>(
        &self,
        old_child: Option<Incr<T>>,
        new_child: Incr<T>,
        child_index: i32,
    ) {
        match old_child {
            None => {
                println!(
                    "change_child simply adding parent to {:?} at child_index {child_index}",
                    new_child.node
                );
                new_child.node.state_add_parent(child_index, self.weak());
            }
            Some(old_child) => {
                if old_child.ptr_eq(&new_child) {
                    // nothing to do! nothing changed!
                    return;
                }
                {
                    let oc_inner = old_child.node.inner();
                    let mut oci = oc_inner.borrow_mut();
                    oci.remove_parent(child_index, self.weak());
                    /* We force [old_child] to temporarily be necessary so that [add_parent] can't
                    mistakenly think it is unnecessary and transition it to necessary (which would
                    add duplicate edges and break things horribly). */
                    oci.force_necessary = true;
                    {
                        new_child.node.state_add_parent(child_index, self.weak());
                    }
                    oci.force_necessary = false;
                    drop(oci);
                    old_child.node.check_if_unnecessary();
                }
            }
        }
    }
}
impl<G: NodeGenerics + 'static> Node<G> {
    fn copy_child(&self, child: Input<G::R>) {
        if child.is_valid() {
            self.maybe_change_value(child.latest());
        } else {
            self.invalidate();
            self.state().propagate_invalidity();
        }
    }
}

pub trait NodeGenerics {
    type Output: Debug + Clone + 'static;
    type R: Debug + Clone + 'static;
    type I1: Debug + Clone + 'static;
    type I2: Debug + Clone + 'static;
    type F1: Fn(Self::I1) -> Self::R + 'static;
    type F2: Fn(Self::I1, Self::I2) -> Self::R + 'static;
    type B1: Fn(Self::I1) -> Incr<Self::R> + 'static;
}

pub(crate) enum Kind<G: NodeGenerics> {
    Invalid,
    Uninitialised,
    Var(Rc<Var<G::R>>),
    Map(MapNode<G::F1, G::I1, G::R>),
    Map2(super::Map2Node<G::F2, G::I1, G::I2, G::R>),
    BindLhsChange(Rc<super::BindNode<G::B1, G::I1, G::R>>),
    BindMain(Rc<super::BindNode<G::B1, G::I1, G::R>>),
    Cutoff(super::CutoffNode<G::R>),
}

impl<G: NodeGenerics> Debug for Kind<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Kind::Invalid => write!(f, "Invalid"),
            Kind::Uninitialised => write!(f, "Uninitialised"),
            Kind::Var(var) => write!(f, "Var({:?})", var),
            Kind::Map(map) => write!(f, "Map({:?})", map),
            Kind::Map2(map2) => write!(f, "Map2({:?})", map2),
            Kind::BindLhsChange(bind) => write!(f, "BindLhsChange({:?})", bind),
            Kind::BindMain(bind) => write!(f, "BindMain({:?})", bind),
            Kind::Cutoff(cutoff) => write!(f, "Cutoff({:?})", cutoff),
        }
    }
}

impl<G: NodeGenerics + 'static> Kind<G> {
    pub const BIND_RHS_CHILD_INDEX: i32 = 1;
    fn initial_num_children(&self) -> usize {
        match self {
            Self::Invalid => 0,
            Self::Uninitialised => 0,
            Self::Var(_) => 0,
            Self::Map(_) | Self::Cutoff(_) => 1,
            Self::Map2(_) => 2,
            Self::BindLhsChange(_) => 1,
            Self::BindMain(_) => 2,
        }
    }
}

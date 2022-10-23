// use enum_dispatch::enum_dispatch;

use super::adjust_heights_heap::AdjustHeightsHeap;
use super::internal_observer::ErasedObserver;
use super::kind::{Kind, NodeGenerics};
use super::scope::Scope;
use super::state::State;
use super::{CutoffNode, Incr, Map2Node, MapNode, NodeRef, WeakNode};
use core::fmt::Debug;
use std::rc::Weak;

use super::stabilisation_num::StabilisationNum;
use refl::Id;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
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
pub(crate) struct NodeInner<'a> {
    pub state: Rc<State<'a>>,
    // cutoff
    // num_on_update_handlers
    // TODO: optimise the one parent case
    // pub(crate) num_parents: i32,
    // parent0
    // parent1_and_beyond
    pub(crate) my_parent_index_in_child_at_index: Vec<i32>,
    pub(crate) my_child_index_in_parent_at_index: Vec<i32>,
    pub(crate) force_necessary: bool,
    pub(crate) observers: Vec<Weak<dyn ErasedObserver<'a>>>,
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
    pub(crate) created_in: Scope<'a>,
    pub recomputed_at: Cell<StabilisationNum>,
    pub changed_at: Cell<StabilisationNum>,
    weak_self: Weak<Self>,
    pub(crate) parents: Vec<ParentRef<'a, G::R>>,
}

pub(crate) type Input<'a, R> = Rc<dyn Incremental<'a, R> + 'a>;

pub(crate) trait Incremental<'a, R>: ErasedNode<'a> + Debug {
    fn as_input(self: Rc<Self>) -> Input<'a, R>;
    fn latest(&self) -> R;
    fn value_opt(&self) -> Option<R>;
    fn add_parent_without_adjusting_heights(&self, child_index: i32, parent_weak: ParentRef<'a, R>);
    fn state_add_parent(&self, child_index: i32, parent_weak: ParentRef<'a, R>);
    fn remove_parent(&mut self, child_index: i32, parent_weak: ParentRef<'a, R>);
}

impl<'a, G: NodeGenerics<'a> + 'a> Incremental<'a, G::R> for Node<'a, G> {
    fn as_input(self: Rc<Self>) -> Input<'a, G::R> {
        self as Input<G::R>
    }
    fn latest(&self) -> G::R {
        self.value_opt.borrow().clone().unwrap()
    }
    fn value_opt(&self) -> Option<G::R> {
        self.value_opt.borrow().clone()
    }
    fn add_parent_without_adjusting_heights(&self, child_index: i32, parent_weak: ParentRef<'a, G::R>) {
        println!("add_parent_without_adjusting_heights");
        debug_assert!({
            let Some(p) = parent_weak.upgrade() else { panic!() };
            p.is_necessary()
        });
        let was_necessary = self.is_necessary();
        let mut child_i = self.inner.borrow_mut();
        self.add_parent(child_index, parent_weak);
        drop(child_i);
        if !self.is_valid() {
            println!("TODO: propagate_invalidity");
            // child_i.state.propagate_invalidity.push(parent);
        }
        if !was_necessary {
            self.became_necessary();
        }
    }
    fn state_add_parent(&self, child_index: i32, parent_weak: ParentRef<'a, G::R>) {
        let parent = parent_weak.upgrade().unwrap();
        debug_assert!(parent.is_necessary());
        self.add_parent_without_adjusting_heights(child_index, parent_weak);
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
            && (parent.recomputed_at().get().is_none() || self.edge_is_stale(parent.weak()))
            {
                let t = self.state();
                let mut rch = t.recompute_heap.borrow_mut();
                rch.insert(parent.packed());
            }
    }
    fn remove_parent(&mut self, child_index: i32, parent_weak: ParentRef<'a, G::R>) {
        let child = self;
        let child_inner = child.inner();
        let mut ci = child_inner.borrow_mut();
        // println!("remove_parent {self:?}, ix {child_index}");
        let parent = parent_weak.upgrade().unwrap();
        let parent_inner = parent.inner();
        let mut parent_i = parent_inner.borrow_mut();

        debug_assert!(child.num_parents() >= 1);
        let parent_index = parent_i.my_parent_index_in_child_at_index[child_index as usize];
        debug_assert!(parent_weak.ptr_eq(
            &child.parents[parent_index as usize]
                .clone()
        ));
        let last_parent_index = child.num_parents() - 1;
        if (parent_index as usize) < last_parent_index {
            // we swap the parent the end of the array into this one's position. This keeps the array
            // small.
            let end_p_weak = child.parents[last_parent_index].clone();
            if let Some(end_p) = end_p_weak.upgrade() {
                let end_p_inner = end_p.inner();
                let mut end_p_i = end_p_inner.borrow_mut();
                let end_child_index = ci.my_child_index_in_parent_at_index[last_parent_index];
                // link parent_index & end_child_index
                end_p_i.my_parent_index_in_child_at_index[end_child_index as usize] = parent_index;
                ci.my_child_index_in_parent_at_index[parent_index as usize] = end_child_index;
            }
            // unlink last_parent_index & child_index
            parent_i.my_parent_index_in_child_at_index[child_index as usize] = -1;
            ci.my_child_index_in_parent_at_index[last_parent_index] = -1;
            // now do what we just did but super easily in the actual Vec
            child.parents.swap_remove(parent_index as usize);
        }
    }

}

pub(crate) trait ParentNode<'a, I>: Debug + ErasedNode<'a> {
    // fn as_parent(&self) -> ParentRef<'a, I>;
    fn child_changed(&mut self, child: &Input<'a, I>, child_index: i32, old_value_opt: Option<I>);
}


type ParentRef<'a, I> = Weak<dyn ParentNode<'a, I> + 'a>;

impl<'a, G: NodeGenerics<'a>> ParentNode<'a, G::I1> for Node<'a, G> {
    fn child_changed(
        &mut self,
        child: &Input<'a, G::I1>,
        child_index: i32,
        old_value_opt: Option<G::I1>,
    ) {
        let k = self.kind.borrow();
        match &*k {
            // Kind::Expert => ...,
            Kind::UnorderedArrayFold(uaf) => {
                let new_value = child.latest();
                uaf.child_changed(child, child_index, old_value_opt, new_value)
            }
            _ => {}
        }
    }
}

pub(crate) trait ErasedNode<'a>: Debug {
    fn num_parents(&self) -> usize;
    fn id(&self) -> NodeId;
    fn is_valid(&self) -> bool;
    fn height(&self) -> i32;
    fn height_in_recompute_heap(&self) -> &Cell<i32>;
    fn height_in_adjust_heights_heap(&self) -> &Cell<i32>;
    fn ensure_parent_height_requirements(
        &self,
        ahh: &mut AdjustHeightsHeap<'a>,
        original_child: &NodeRef,
        original_parent: &NodeRef,
    );
    fn set_height(&self, height: i32);
    fn old_height(&self) -> i32;
    fn set_old_height(&self, height: i32);
    fn is_stale(&self) -> bool;
    fn is_stale_with_respect_to_a_child(&self) -> bool;
    fn edge_is_stale(&self, parent: WeakNode<'a>) -> bool;
    fn is_necessary(&self) -> bool;
    fn needs_to_be_recomputed(&self) -> bool;
    fn became_necessary(&self);
    fn became_unnecessary(&self);
    fn check_if_unnecessary(&self);
    fn is_in_recompute_heap(&self) -> bool;
    fn recompute(&self);
    fn inner(&self) -> &RefCell<NodeInner<'a>>;
    fn state(&self) -> Rc<State<'a>>;
    fn weak(&self) -> WeakNode<'a>;
    fn packed(&self) -> NodeRef<'a>;
    fn foreach_child(&self, f: &mut dyn FnMut(i32, NodeRef<'a>) -> ());
    fn recomputed_at(&self) -> &Cell<StabilisationNum>;
    fn invalidate(&self);
    fn adjust_heights_bind_lhs_change(
        &self,
        ahh: &mut AdjustHeightsHeap<'a>,
        oc: &NodeRef<'a>,
        op: &NodeRef<'a>,
    );
    fn created_in(&self) -> Scope<'a>;
}

    

impl<'a, G: NodeGenerics<'a>> Debug for Node<'a, G> {
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
    fn state(&self) -> Rc<State<'a>> {
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
    fn ensure_parent_height_requirements(
        &self,
        ahh: &mut AdjustHeightsHeap<'a>,
        original_child: &NodeRef,
        original_parent: &NodeRef,
    ) {
        for parent in self.parents() {
            let parent = parent.upgrade().unwrap().packed();
            ahh.ensure_height_requirement(
                &original_child,
                &original_parent,
                &self.packed(),
                &parent,
            );
        }
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
    fn is_stale(&self) -> bool {
        let k = self.kind.borrow();
        match &*k {
            Kind::Invalid => panic!(),
            Kind::Var(var) => {
                let set_at = var.set_at.get();
                let recomputed_at = self.recomputed_at.get();
                set_at > recomputed_at
            }
            Kind::ArrayFold(_)
            | Kind::Map(_)
            | Kind::Map2(_)
            | Kind::BindLhsChange(..)
            | Kind::BindMain(..) => {
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
    fn edge_is_stale(&self, parent: WeakNode<'a>) -> bool {
        let Some(parent) = parent.upgrade() else { return false };
        self.changed_at.get() > parent.recomputed_at().get()
    }
    fn is_necessary(&self) -> bool {
        let i = self.inner().borrow();
        self.num_parents() > 0
            || !i.observers.is_empty()
            // || kind is freeze
            || i.force_necessary
    }
    fn needs_to_be_recomputed(&self) -> bool {
        self.is_necessary() && self.is_stale()
    }
    fn num_parents(&self) -> usize {
        self.parents.len()
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
        let as_parent = self.as_parent();
        let h = Cell::new({
            self.set_height(self.created_in.height() + 1);
            self.height()
        });
        self.foreach_child_typed(ForeachChild {
            i1: &|index, child| {
                child.add_parent_without_adjusting_heights(index, self.as_parent());
                if child.height() >= h.get() {
                    h.set(child.height() + 1);
                }
            },
            i2: &|index, child| {
                child.add_parent_without_adjusting_heights(index, self.as_parent2());
                if child.height() >= h.get() {
                    h.set(child.height() + 1);
                }
            },
        });
        self.set_height(h.get());
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
    fn foreach_child(&self, f: &mut dyn FnMut(i32, NodeRef<'a>) -> ()) {
        let k = self.kind.borrow();
        match &*k {
            Kind::Invalid => {}
            Kind::Uninitialised => {}
            Kind::Map(MapNode { input, .. }) => f(0, input.packed()),
            Kind::Map2(Map2Node { one, two, .. }) => {
                f(0, one.clone().packed());
                f(1, two.clone().packed());
            }
            Kind::BindLhsChange(_, bind) => f(0, bind.lhs.packed()),
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
        let k = self.kind.borrow();
        let id = self.id;
        let height = self.height();
        match &*k {
            Kind::Var(var) => {
                let value = var.value.borrow();
                let v = value.clone();
                println!("-- recomputing Var(id={id:?}) <- {v:?}");
                drop(value);
                self.maybe_change_value(v);
            }
            Kind::Map(map) => {
                let map: &MapNode<G::F1, G::I1, G::R> = map;
                let input = map.input.latest();
                let mut f = map.mapper.borrow_mut();
                let new_value = f(input);
                println!("-- recomputing Map(id={id:?}) <- {new_value:?}");
                self.maybe_change_value(new_value);
            }
            Kind::Map2(map2) => {
                let map2: &Map2Node<'a, G::F2, G::I1, G::I2, G::R> = map2;
                let i1 = map2.one.latest();
                let i2 = map2.two.latest();
                let mut f = map2.mapper.borrow_mut();
                let new_value = f(i1, i2);
                println!("-- recomputing Map2(id={id:?}) <- {new_value:?}");
                self.maybe_change_value(new_value);
            }
            Kind::BindLhsChange(id, bind) => {
                // leaves an empty vec for next time
                let mut old_all_nodes_created_on_rhs = bind.all_nodes_created_on_rhs.take();
                let lhs = bind.lhs.latest();
                let rhs = {
                    let old_scope = self.state().current_scope();
                    *self.state().current_scope.borrow_mut() = bind.rhs_scope.borrow().clone();
                    println!("-- recomputing BindLhsChange(id={id:?}, {lhs:?})");
                    let mut f = bind.mapper.borrow_mut();
                    let rhs = f(lhs);
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
                    main.change_child_bind_rhs(old_rhs.clone(), rhs, Kind::<G>::BIND_RHS_CHILD_INDEX);
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
                debug_assert!(self.is_valid());
                self.maybe_change_value(id.r_unit.cast(()));
            }
            Kind::BindMain(id, bind) => {
                let rhs = bind.rhs.borrow().as_ref().unwrap().clone();
                println!("-- recomputing BindMain(id={id:?}, h={height:?}) <- {rhs:?}");
                self.copy_child_d(&rhs.node, id.rhs_r);
            }
            Kind::ArrayFold(af) => self.maybe_change_value(af.compute()),
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

    fn created_in(&self) -> Scope<'a> {
        self.created_in.clone()
    }
}

fn invalidate_nodes_created_on_rhs(all_nodes_created_on_rhs: &mut Vec<WeakNode<'_>>) {
    for node in all_nodes_created_on_rhs.drain(..) {
        invalidate_node(node);
    }
}
fn invalidate_node(node: WeakNode<'_>) {
    let Some(node) = node.upgrade() else { return };
    node.invalidate();
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

    pub fn create(state: Rc<State<'a>>, created_in: Scope<'a>, kind: Kind<'a, G>) -> Rc<Self> {
        Node {
            id: NodeId::next(),
            weak_self: Weak::<Self>::new(),
            inner: RefCell::new(NodeInner {
                state,
                observers: Default::default(),
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
            parents: vec![],
        }
        .into_rc()
    }

    fn maybe_change_value(&self, value: G::R) {
        let old = self.value_opt.replace(Some(value));
        let inner = self.inner.borrow();
        // TODO: cutoffs!
        let should_cutoff = || false;
        if old.is_none() || !should_cutoff() {
            self.changed_at.set(inner.state.stabilisation_num.get());
            inner
                .state
                .num_nodes_changed
                .set(inner.state.num_nodes_changed.get() + 1);
            /* if node.num_on_update_handlers > 0
            then (
            node.old_value_opt <- old_value_opt;
            handle_after_stabilization node); */
            if self.num_parents() >= 1 {
                for (parent_index, parent) in self
                    .parents
                    .iter()
                    .enumerate()
                    .filter_map(|(i, x)| x.upgrade().map(|a| (i, a)))
                {
                    let i = self.inner().borrow();
                    let child_index = i.my_child_index_in_parent_at_index[parent_index];
                    parent.child_changed(&self.as_child(), child_index, old);
                    debug_assert!(parent.needs_to_be_recomputed());
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
    }

    fn change_child_bind_rhs(
        &self,
        old_child: Option<Incr<'a, G::BindRhs>>,
        new_child: Incr<'a, G::BindRhs>,
        child_index: i32,
    ) {
        let k = self.kind.borrow();
        let id = match &*k {
            Kind::BindMain(id, _) => id,
            _ => return
        };
        let new_child_node = id.input_rhs_i1.cast(new_child.node);
        match old_child {
            None => {
                println!(
                    "change_child simply adding parent to {:?} at child_index {child_index}",
                    new_child_node
                );
                new_child_node.state_add_parent(child_index, self.as_parent());
            }
            Some(old_child) => {
                if old_child.ptr_eq(&new_child) {
                    // nothing to do! nothing changed!
                    return;
                }
                {
                    let old_child_node = id.input_rhs_i1.cast(old_child.node);
                    old_child_node.remove_parent(child_index, self.as_parent());
                    /* We force [old_child] to temporarily be necessary so that [add_parent] can't
                    mistakenly think it is unnecessary and transition it to necessary (which would
                    add duplicate edges and break things horribly). */
                    let oc_inner = old_child.node.inner();
                    let mut oci = oc_inner.borrow_mut();
                    oci.force_necessary = true;
                    {
                        new_child_node.state_add_parent(child_index, self.as_parent());
                    }
                    oci.force_necessary = false;
                    drop(oci);
                    old_child.node.check_if_unnecessary();
                }
            }
        }
    }

    fn copy_child(&self, child: &Input<G::R>) {
        if child.is_valid() {
            self.maybe_change_value(child.latest());
        } else {
            self.invalidate();
            self.state().propagate_invalidity();
        }
    }

    fn copy_child_d(&self, child: &Input<G::BindRhs>, token: Id<G::BindRhs, G::R>) {
        if child.is_valid() {
            let latest = child.latest();
            self.maybe_change_value(token.cast(latest));
        } else {
            self.invalidate();
            self.state().propagate_invalidity();
        }
    }

    fn parents(&self) -> &[ParentRef<'a, G::R>] {
        &self.parents[..]
    }
    fn num_parents(&self) -> usize {
        self.parents.len()
    }
    fn add_parent(&mut self, child_index: i32, parent_weak: ParentRef<'a, G::R>) {
        let child = self;
        let child_inner = self.inner();
        let ci = child_inner.borrow_mut();

        // we're appending here
        let parent = parent_weak.upgrade().unwrap();
        let parent_index = child.parents.len() as i32;
        let parent_inner = parent.inner();
        let mut parent_i = parent_inner.borrow_mut();

        while ci.my_child_index_in_parent_at_index.len() <= parent_index as usize {
            ci.my_child_index_in_parent_at_index.push(-1);
        }
        println!("ci_in_pia[{parent_index}] = {child_index}");
        ci.my_child_index_in_parent_at_index[parent_index as usize] = child_index;

        while parent_i.my_parent_index_in_child_at_index.len() <= child_index as usize {
            parent_i.my_parent_index_in_child_at_index.push(-1);
        }
        println!("pi_in_cia[{child_index}] = {parent_index}");
        parent_i.my_parent_index_in_child_at_index[child_index as usize] = parent_index;

        child.parents.push(parent_weak);
    }
    
    fn as_parent(&self) -> ParentRef<'a, G::I1> {
        self.weak_self.clone()
    }
    fn as_parent2(&self) -> ParentRef<'a, G::I2> {
        self.weak_self.clone()
    }
    fn as_child(&self) -> Input<'a, G::R> {
        self.weak_self.clone().upgrade().unwrap()
    }
    fn foreach_child_typed(&self, mut f: ForeachChild<'a, G>) {
        let k = self.kind.borrow();
        match &*k {
            Kind::Invalid => {}
            Kind::Uninitialised => {}
            Kind::Map(MapNode { input, .. }) => (f.i1)(0, input.clone().as_input()),
            Kind::Map2(Map2Node { one, two, .. }) => {
                (f.i1)(0, one.clone().as_input());
                (f.i2)(1, two.clone().as_input());
            }
            Kind::BindLhsChange(id, bind) => {
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
            Kind::Cutoff(_) => unimplemented!(),
            // Kind::Cutoff(CutoffNode { input, .. }) => (f.i1)(0, input.as_input()),
        }
    }

    fn remove_children(&self) {
        self.foreach_child_typed( ForeachChild {
            i1: &|index, child| {
                child.remove_parent(index, self.as_parent());
                child.check_if_unnecessary();
            },
            i2: &|index, child| {
                child.remove_parent(index, self.as_parent2());
                child.check_if_unnecessary();
            },
        })
    }

}

struct ForeachChild<'a, G: NodeGenerics<'a>> {
    i1: &'a dyn FnMut(i32, Input<'a, G::I1>),
    i2: &'a dyn FnMut(i32, Input<'a, G::I2>),
}

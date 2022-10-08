// use enum_dispatch::enum_dispatch;

use super::internal_observer::Observer;
use super::{state::State, var::Var};
use super::{BindNode, BindScope, CutoffNode, Incr, Map2Node, MapNode};
use core::fmt::Debug;

use super::stabilisation_num::StabilisationNum;
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

#[derive(Debug)]
pub struct NodeId;
impl NodeId {
    fn next() -> Self {
        NodeId
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Scope {
    Top,
    Bind(Weak<dyn BindScope>),
}

impl Scope {
    pub(crate) fn height(&self) -> i32 {
        match self {
            Self::Top => 0,
            Self::Bind(weak) => {
                let Some(strong) = weak.upgrade() else { panic!() };
                strong.height()
            }
        }
    }
}

/// Needs a better name but ok
#[derive(Debug)]
pub struct NodeInner {
    pub id: NodeId,
    pub state: Rc<State>,
    // cutoff
    // num_on_update_handlers
    // TODO: optimise the one parent case
    // pub(crate) num_parents: i32,
    // parent0
    // parent1_and_beyond
    pub(crate) parents: Vec<Option<WeakNode>>,
    pub(crate) my_parent_index_in_child_at_index: Vec<i32>,
    pub(crate) my_child_index_in_parent_at_index: Vec<i32>,
    // next_node_in_same_scope
    pub prev_in_recompute_heap: Option<PackedNode>,
    pub next_in_recompute_heap: Option<PackedNode>,
    pub(crate) observers: Vec<Weak<dyn Observer>>,
    pub(crate) force_necessary: bool,
}

pub(crate) struct Node<G: NodeGenerics> {
    pub(crate) inner: RefCell<NodeInner>,
    pub(crate) kind: RefCell<Kind<G>>,
    pub(crate) value_opt: RefCell<Option<G::R>>,
    pub(crate) old_value_opt: RefCell<Option<G::R>>,
    pub height: Cell<i32>,
    pub height_in_recompute_heap: Cell<i32>,
    pub(crate) created_in: Scope,
    pub recomputed_at: Cell<StabilisationNum>,
    pub changed_at: Cell<StabilisationNum>,
}

pub type Input<R> = Rc<dyn Incremental<R>>;

pub trait Incremental<R>: ErasedNode {
    fn as_input(self: Rc<Self>) -> Input<R>;
    fn latest(&self) -> R;
}

impl<G: NodeGenerics + 'static> Incremental<G::R> for Node<G> {
    fn as_input(self: Rc<Self>) -> Input<G::R> {
        let rc = self.clone();
        rc as Input<G::R>
    }
    fn latest(&self) -> G::R {
        self.value()
    }
}

pub type PackedNode = Rc<dyn ErasedNode>;
pub type WeakNode = Weak<dyn ErasedNode>;

pub trait ErasedNode: Debug {
    fn is_valid(&self) -> bool;
    fn height(&self) -> i32;
    fn height_in_recompute_heap(&self) -> &Cell<i32>;
    fn set_height(&mut self, height: i32);
    fn add_parent_without_adjusting_heights(self: Rc<Self>, child_index: i32, parent: WeakNode);
    fn is_stale(&self) -> bool;
    fn is_necessary(&self) -> bool;
    fn became_necessary(self: Rc<Self>);
    fn is_in_recompute_heap(&self) -> bool;
    fn recompute(&self);
    fn inner(&self) -> &RefCell<NodeInner>;
    fn state(&self) -> Rc<State>;
    fn weak(self: Rc<Self>) -> Weak<dyn ErasedNode>;
    fn packed(self: Rc<Self>) -> Rc<dyn ErasedNode>;
    fn foreach_child(&self, f: &mut dyn FnMut(i32, PackedNode) -> ());
}

impl NodeInner {
    fn is_necessary(&self) -> bool {
        println!(
            "is_necessary: num_parents {:?}, observers len {:?} force_nec {:?}",
            self.num_parents(),
            self.observers.len(),
            self.force_necessary
        );
        self.num_parents() > 0
            || !self.observers.is_empty()
            // || kind is freeze
            || self.force_necessary
    }
    fn num_parents(&self) -> usize {
        self.parents.len()
    }
    fn add_parent(&mut self, child_index: i32, parent: WeakNode) {
        let child_i = self;
        let parent_index = child_i.parents.len();
        while child_i.my_child_index_in_parent_at_index.len() < parent_index as usize {
            child_i.my_child_index_in_parent_at_index.push(-1);
        }
        child_i.my_child_index_in_parent_at_index[parent_index] = child_index;

        while child_i.my_parent_index_in_child_at_index.len() < child_index as usize {
            child_i.my_parent_index_in_child_at_index.push(-1);
        }
        child_i.parents.push(Some(parent));
    }
}

impl<G: NodeGenerics + 'static> Debug for Node<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node").field("inner", &self.inner).finish()
    }
}
impl<G: NodeGenerics + 'static> ErasedNode for Node<G> {
    fn weak(self: Rc<Self>) -> Weak<dyn ErasedNode> {
        let weak = Rc::downgrade(&self);
        weak as Weak<dyn ErasedNode>
    }
    fn packed(self: Rc<Self>) -> PackedNode {
        self as Rc<dyn ErasedNode>
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
    fn set_height(&mut self, height: i32) {
        // TODO: checks
        self.height.set(height);
    }
    fn add_parent_without_adjusting_heights(self: Rc<Self>, child_index: i32, parent: WeakNode) {
        println!("add_parent_without_adjusting_heights");
        debug_assert!({
            let Some(p) = parent.upgrade() else { panic!() };
            p.is_necessary()
        });
        let mut child_i = self.inner.borrow_mut();
        let was_necessary = child_i.is_necessary();
        child_i.add_parent(child_index, parent);
        drop(child_i);
        if !self.is_valid() {
            println!("TODO: propagate_invalidity");
            // child_i.state.propagate_invalidity.push(parent);
        }
        if !was_necessary {
            self.became_necessary();
        }
    }
    fn is_stale(&self) -> bool {
        let k = self.kind.borrow();
        match &*k {
            Kind::Invalid => panic!(),
            Kind::Var(var) => {
                let Some(var) = var.upgrade() else { return false };
                let set_at = var.set_at.get();
                let recomputed_at = self.recomputed_at.get();
                set_at > recomputed_at
            }
            // wrong
            _ => false,
        }
    }
    fn is_necessary(&self) -> bool {
        let i = self.inner().borrow();
        i.is_necessary() // || kind is freeze
    }
    fn became_necessary(self: Rc<Self>) {
        println!("became_necessary");
        // if Node.is_valid node && not (Scope.is_necessary node.created_in)
        //   then
        //     failwiths
        //       ~here:[%here]
        //       "Trying to make a node necessary whose defining bind is not necessary"
        //       node
        //       [%sexp_of: _ Node.t];
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
            self.height.set(self.created_in.height() + 1);
            self.height.get()
        };
        self.foreach_child(&mut |index, child| {
            child
                .clone()
                .add_parent_without_adjusting_heights(index, weak.clone());
            {
                if child.height() >= h {
                    h = child.height() + 1;
                }
            }
        });
        self.height.set(h);
        debug_assert!(!self.is_in_recompute_heap());
        debug_assert!(self.is_necessary());
        if self.is_stale() {
            let mut rch = t.recompute_heap.borrow_mut();
            rch.insert(self.clone().weak());
        }
    }
    fn foreach_child(&self, f: &mut dyn FnMut(i32, PackedNode) -> ()) {
        let k = self.kind.borrow();
        match &*k {
            Kind::Invalid => {}
            Kind::Uninitialised => {}
            Kind::Map(MapNode { input, .. }) => f(0, input.clone().packed()),
            Kind::Map2(Map2Node { one, two, .. }) => {
                f(0, one.clone().packed());
                f(1, two.clone().packed());
            }
            Kind::BindLhsChange(bind) => f(0, bind.lhs.clone().packed()),
            Kind::BindMain(bind) => f(0, bind.lhs_change.clone().packed()),
            Kind::Var(var) => {}
            Kind::Cutoff(CutoffNode { input, .. }) => f(0, input.clone().packed()),
        }
    }
    fn is_in_recompute_heap(&self) -> bool {
        self.height_in_recompute_heap.get() >= 0
    }
    fn recompute(&self) {
        let t = self.inner.borrow().state.clone();
        t.num_nodes_recomputed.set(t.num_nodes_recomputed.get() + 1);
        self.recomputed_at.set(t.stabilisation_num.get());
        let mut k = self.kind.borrow_mut();
        match &mut *k {
            Kind::Var(var) => {
                let Some(var): Option<Rc<Var<G::R>>> = var.upgrade() else { return };
                let value = var.value.borrow();
                let v = value.clone();
                drop(value);
                self.maybe_change_value(v);
            }
            Kind::Map(map) => {
                let map: &mut MapNode<G::F1, G::I1, G::R> = map;
                let input = map.input.latest();
                let new_value = (map.mapper)(input);
                self.maybe_change_value(new_value);
            }
            Kind::Map2(map2) => {
                let map2: &Map2Node<G::F2, G::I1, G::I2, G::R> = map2;
                let i1 = map2.one.latest();
                let i2 = map2.two.latest();
                let new_value = (map2.mapper)(i1, i2);
                self.maybe_change_value(new_value);
            }
            Kind::BindLhsChange(bind) => {
                let bind: &mut Rc<BindNode<G::B1, G::I1, G::R>> = bind;
                let mut all = bind.all_nodes_created_on_rhs.borrow_mut();
                all.clear();
                let lhs = bind.lhs.latest();
                let rhs = {
                    let old_scope = self.state().current_scope();
                    *self.state().current_scope.borrow_mut() = bind.rhs_scope.borrow().clone();
                    let rhs = (bind.mapper)(lhs);
                    *self.state().current_scope.borrow_mut() = old_scope;
                    rhs
                };
                *bind.rhs.borrow_mut() = Some(rhs);
            }
            _ => {}
        }
    }
}

impl<G: NodeGenerics + 'static> Node<G> {
    pub fn create(state: Rc<State>, created_in: Scope, kind: Kind<G>) -> Self {
        Node {
            inner: RefCell::new(NodeInner {
                id: NodeId::next(),
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
            height_in_recompute_heap: Cell::new(-1),
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
        self.old_value_opt.replace(None);
        self.old_value_opt.swap(&self.value_opt);
        self.value_opt.replace(Some(value));
        let inner = self.inner.borrow();
        self.changed_at.set(inner.state.stabilisation_num.get());
        inner
            .state
            .num_nodes_changed
            .set(inner.state.num_nodes_changed.get() + 1);
        // self.changed_at = self.state.stabilization_num;
        // self.state.num_nodes_changed = self.state.num_nodes_changed + 1;
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
    Var(Weak<Var<G::R>>),
    Map(MapNode<G::F1, G::I1, G::R>),
    Map2(super::Map2Node<G::F2, G::I1, G::I2, G::R>),
    BindLhsChange(Rc<super::BindNode<G::B1, G::I1, G::R>>),
    BindMain(Rc<super::BindNode<G::B1, G::I1, G::R>>),
    Cutoff(super::CutoffNode<G::R>),
}

impl<G: NodeGenerics + 'static> Kind<G> {
    fn initial_num_children(&self) -> usize {
        match self {
            Self::Invalid => 0,
            Self::Uninitialised => 0,
            Self::Var(_) => 0,
            Self::Map(_) | Self::BindLhsChange(_) | Self::BindMain(_) | Self::Cutoff(_) => 1,
            Self::Map2(_) => 2,
        }
    }
}

// macro_rules! impl_nodekind {
//     (type I1)
// }

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn testit() {
        let incr = State::new();
        let var = incr.var(5);
        var.set(10);
        let watch = var.watch();
        let observer = watch.observe();
        incr.stabilise();
        assert_eq!(watch.value(), 10);
        assert_eq!(observer.value(), 10);
    }
}
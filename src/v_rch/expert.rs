use std::{
    cell::{Cell, RefCell},
    marker::PhantomData,
    rc::Rc, any::Any,
};

use crate::{Value, Incr};

use super::{node::Input, kind::NodeGenerics, NodeRef};

pub(crate) trait ExpertEdge: Any {
    fn on_change(&self);
    fn as_any(&self) -> &dyn Any;
    fn packed(&self) -> NodeRef;
    fn index_cell(&self) -> &Cell<Option<i32>>;
}

pub(crate) trait IsEdge: ExpertEdge + Any {}
impl<T> IsEdge for T where T: ExpertEdge + Any {}

pub(crate) type PackedEdge = Rc<dyn IsEdge>;

pub(crate) struct Edge<T> {
    pub child: Input<T>,
    pub on_change: RefCell<Option<Box<dyn FnMut(&T)>>>,
    /* [index] is defined whenever the [edge] is in the [children] of some [t]. Then it is
    the index of this [edge] in that [children] array. It might seem redundant with all
    the other indexes we have, but it is necessary to remove children.  The index may
    change as sibling children are removed. */
    pub index: Cell<Option<i32>>,
}

pub(crate) fn expect_edge_downcast<'a, T: 'static>(packed_edge: &'a PackedEdge, name: &'static str) -> &'a Edge<T> {
    match packed_edge.as_any().downcast_ref() {
        None => panic!(
            "add_dependency called with wrong child type: should be {}",
            std::any::type_name::<Edge<T>>()
        ),
        Some(edge) => edge,
    }
}

impl<T> Edge<T> {
    fn new(child: Input<T>, on_change: Option<Box<dyn FnMut(&T)>>) -> Self {
        Self {
            child,
            on_change: on_change.into(),
            index: None.into(),
        }
    }
    pub(crate) fn as_input(&self) -> Input<T> {
        self.child.clone()
    }
}

impl<T: Value> ExpertEdge for Edge<T> {
    fn on_change(&self) {
        let mut handler = self.on_change.borrow_mut();
        if let Some(h) = &mut *handler {
            let v = self.child.value_as_ref();
            h(v.as_ref().unwrap());
        }
    }
    fn packed(&self) -> NodeRef {
        self.child.packed()
    }
    fn as_any(&self) -> &dyn Any {
        self as &dyn Any
    }

    fn index_cell(&self) -> &Cell<Option<i32>> {
        &self.index
    }
}

pub(crate) struct ExpertNode<T, C, F, ObsChange> {
    pub node: PhantomData<(T, C)>,
    pub recompute: RefCell<F>,
    pub on_observability_change: RefCell<ObsChange>,
    pub children: RefCell<Vec<PackedEdge>>,
    pub force_stale: Cell<bool>,
    pub num_invalid_children: Cell<i32>,
    pub will_fire_all_callbacks: Cell<bool>,
}

pub enum MakeStale {
    AlreadyStale,
    Ok,
}

impl<T, C: 'static, F> ExpertNode<T, C, F, fn(bool)>
where
    F: FnMut() -> T,
{
    pub(crate) fn new(recompute: F) -> Self {
        fn ignore(_: bool) {}
        ExpertNode::new_obs(recompute, ignore)
    }
}

impl<T, C: 'static, F, ObsChange> ExpertNode<T, C, F, ObsChange>
where
    F: FnMut() -> T,
    ObsChange: FnMut(bool),
{
    pub(crate) fn new_obs(recompute: F, on_observability_change: ObsChange) -> Self {
        Self {
            node: PhantomData,
            recompute: recompute.into(),
            on_observability_change: on_observability_change.into(),
            children: vec![].into(),
            force_stale: false.into(),
            num_invalid_children: 0.into(),
            will_fire_all_callbacks: true.into(),
        }
    }
    pub(crate) fn incr_invalid_children(&self) {
        self.num_invalid_children.set(self.num_invalid_children.get() + 1);
    }
    pub(crate) fn decr_invalid_children(&self) {
        self.num_invalid_children.set(self.num_invalid_children.get() - 1);
    }

    pub(crate) fn make_stale(&self) -> MakeStale {
        if self.force_stale.get() {
            MakeStale::AlreadyStale
        } else {
            self.force_stale.set(true);
            MakeStale::Ok
        }
    }
    pub(crate) fn add_child_edge(&self, edge: PackedEdge) -> i32 {
        assert!(edge.index_cell().get().is_none());
        let mut children = self.children.borrow_mut();
        let new_child_index = children.len() as i32;
        edge.index_cell().set(Some(new_child_index));
        children.push(edge);
        self.force_stale.set(true);
        new_child_index
    }
    pub(crate) fn swap_children(&self, one: usize, two: usize) {
        let mut children = self.children.borrow_mut();
        let c1 = children[one].index_cell();
        let c2 = children[two].index_cell();
        c1.swap(c2);
        children.swap(one, two);
    }
    pub(crate) fn last_child_edge_exn(&self) -> PackedEdge {
        let children = self.children.borrow();
        children.last().unwrap().clone()
    }
    pub(crate) fn remove_last_child_edge_exn(&self) {
        let mut children = self.children.borrow_mut();
        let packed_edge = children.pop().unwrap();
        self.force_stale.set(true);
        packed_edge.index_cell().set(None);
    }
    pub(crate) fn before_main_computation(&self) -> Result<(), Invalid> {
        if self.num_invalid_children.get() > 0 {
            Err(Invalid)
        } else {
            self.force_stale.set(false);
            if self.will_fire_all_callbacks.replace(false) {
                for child in self.children.borrow().iter() {
                    child.on_change()
                }
            }
            Ok(())
        }
    }
    pub(crate) fn observability_change(&self, is_now_observable: bool) {
        let mut handler = self.on_observability_change.borrow_mut();
        handler(is_now_observable);
        if !is_now_observable {
            // for next time. this is a reset.
            self.will_fire_all_callbacks.set(true);
            /* If we don't reset num_invalid_children, we would double count them: just imagine
               what happens we if reconnect/disconnect/reconnect/disconnect with an invalid
               child. */
            self.num_invalid_children.set(0);
        }
    }
    pub(crate) fn run_edge_callback(&self, child_index: i32) {
        if !self.will_fire_all_callbacks.get() {
            let children = self.children.borrow();
            let Some(child) = children.get(child_index as usize) else {return};
            child.on_change()
        }
    }
}

pub(crate) struct Invalid;

use core::fmt::Debug;
impl<T, C, R, O> Debug for ExpertNode<T, C, R, O>
where T: Debug, C: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExpertNode").finish()
    }
}

impl<T, C, FRecompute, FObsChange> NodeGenerics for ExpertNode<T, C, FRecompute, FObsChange>
where
    T: Value,
    C: Value,
    FRecompute: FnMut() -> T + 'static,
    FObsChange: FnMut(bool) + 'static,
{
    type R = T;
    type BindLhs = ();
    type BindRhs = ();
    type I1 = C;
    type I2 = ();
    type F1 = fn(&Self::I1) -> Self::R;
    type F2 = fn(&Self::I1, &Self::I2) -> Self::R;
    type B1 = fn(&Self::BindLhs) -> Incr<Self::BindRhs>;
    type Fold = fn(Self::R, &Self::I1) -> Self::R;
    type Update = fn(Self::R, &Self::I1, &Self::I1) -> Self::R;
    type WithOld = fn(Option<Self::R>, &Self::I1) -> (Self::R, bool);
    type FRef = fn(&Self::I1) -> &Self::R;
    type Recompute = FRecompute;
    type ObsChange = FObsChange;
}

pub mod public {
    use std::{rc::Rc, marker::PhantomData};

    use crate::{Incr, IncrState, Value};

    use super::Edge;

    pub struct Dependency<T> {
        edge: Rc<Edge<T>>,
    }
    impl<T> core::fmt::Debug for Dependency<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Dependency")
        }
    }
    impl<T> PartialEq for Dependency<T> {
        fn eq(&self, other: &Self) -> bool {
            Rc::ptr_eq(&self.edge, &other.edge)
        }
    }
    impl<T> Clone for Dependency<T> {
        fn clone(&self) -> Self {
            Dependency {
                edge: self.edge.clone(),
            }
        }
    }

    impl<T> Dependency<T> {
        #[inline]
        pub fn new(on: &Incr<T>) -> Self {
            let edge = Edge::new(on.node.clone(), None).into();
            Dependency { edge }
        }
        pub fn new_on_change(on: Incr<T>, on_change: impl FnMut(&T) + 'static) -> Self {
            let edge = Edge::new(on.node.clone(), Some(Box::new(on_change))).into();
            Dependency { edge }
        }
    }

    impl<T: Clone> Dependency<T> {
        pub fn value_cloned(&self) -> T {
            self.edge.child.latest()
        }
    }

    pub struct Node<T, C> {
        incr: Incr<T>,
        _p: PhantomData<C>,
    }

    impl<T, C> Clone for Node<T, C> {
        fn clone(&self) -> Self {
            Node { incr: self.incr.clone() , _p: self._p }
        }
    }

    use crate::v_rch::state::expert;
    impl<T: Value, C: Value> Node<T, C> {
        pub fn new(state: &IncrState, f: impl FnMut() -> T + 'static) -> Node<T, C> {
            fn ignore(_: bool) {}
            Node::new_(state, f, ignore)
        }
        pub fn new_(state: &IncrState, f: impl FnMut() -> T + 'static, obs_change: impl FnMut(bool) + 'static) -> Self {
            let incr = expert::create::<T, C, _, _>(&state.0, f, obs_change);
            Self { incr, _p: PhantomData }
        }
        pub fn watch(&self) -> Incr<T> {
            self.incr.clone()
        }
        pub fn make_stale(&self) {
            expert::make_stale(&self.incr.node.packed())
        }
        pub fn invalidate(&self) {
            expert::invalidate(&self.incr.node.packed())
        }
        pub fn add_dependency<D: Value>(&self, dep: Dependency<D>) {
            let edge = dep.edge;
            expert::add_dependency(&self.incr.node.packed(), edge)
        }
        pub fn remove_dependency<D: Value>(&self, dep: Dependency<D>) {
            let edge = dep.edge;
            expert::remove_dependency(&self.incr.node.packed(), edge)
        }
    }

    #[test]
    fn test() {
        let incr = crate::IncrState::new();
        let node = incr.var(10).watch();
        let dep = Dependency::new(&node);
    }
}

use std::{
    any::{Any, TypeId},
    cell::{Cell, RefCell},
    marker::PhantomData,
    rc::Rc,
};

use super::NodeGenerics;
use crate::node::ErasedIncremental;
use crate::{CellIncrement, Incr, NodeRef, Value};

pub(crate) trait ExpertEdge: Any {
    /// Called from run_edge_callback
    fn on_change(&self);
    fn packed(&self) -> NodeRef;
    fn index_cell(&self) -> &Cell<Option<i32>>;
    fn erased_input(&self) -> &dyn ErasedIncremental;
    fn edge_input_type_id(&self) -> TypeId;
}

pub(crate) trait IsEdge: ExpertEdge + Any {}
impl<T> IsEdge for T where T: ExpertEdge + Any {}

pub(crate) type PackedEdge = Rc<dyn IsEdge>;

pub(crate) struct Edge<T> {
    pub child: Incr<T>,
    pub on_change: RefCell<Option<Box<dyn FnMut(&T)>>>,
    /* [index] is defined whenever the [edge] is in the [children] of some [t]. Then it is
    the index of this [edge] in that [children] array. It might seem redundant with all
    the other indexes we have, but it is necessary to remove children.  The index may
    change as sibling children are removed. */
    pub index: Cell<Option<i32>>,
}

impl<T> Edge<T> {
    fn new(child: Incr<T>, on_change: Option<Box<dyn FnMut(&T)>>) -> Self {
        Self {
            child,
            on_change: on_change.into(),
            index: None.into(),
        }
    }
}

impl<T: Value> ExpertEdge for Edge<T> {
    fn on_change(&self) {
        let mut handler = self.on_change.borrow_mut();
        if let Some(h) = &mut *handler {
            let v = self.child.node.value_as_ref();
            h(v.as_ref().unwrap());
        }
    }
    fn packed(&self) -> NodeRef {
        self.child.node.packed()
    }

    fn index_cell(&self) -> &Cell<Option<i32>> {
        &self.index
    }

    fn erased_input(&self) -> &dyn ErasedIncremental {
        self.child.node.erased_input()
    }

    fn edge_input_type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }
}

pub(crate) struct ExpertNode<T, F, ObsChange> {
    pub _f: PhantomData<T>,
    pub recompute: RefCell<Option<F>>,
    pub on_observability_change: RefCell<Option<ObsChange>>,
    pub children: RefCell<Vec<PackedEdge>>,
    pub force_stale: Cell<bool>,
    pub num_invalid_children: Cell<i32>,
    pub will_fire_all_callbacks: Cell<bool>,
}

impl<T, F, O> Drop for ExpertNode<T, F, O> {
    fn drop(&mut self) {
        self.children.take();
        self.recompute.take();
        self.on_observability_change.take();
    }
}

pub enum MakeStale {
    AlreadyStale,
    Ok,
}

impl<T, F, ObsChange> ExpertNode<T, F, ObsChange>
where
    F: FnMut() -> T,
    ObsChange: FnMut(bool),
{
    pub(crate) fn new_obs(recompute: F, on_observability_change: ObsChange) -> Self {
        Self {
            _f: PhantomData,
            recompute: Some(recompute).into(),
            on_observability_change: Some(on_observability_change).into(),
            children: vec![].into(),
            force_stale: false.into(),
            num_invalid_children: 0.into(),
            will_fire_all_callbacks: true.into(),
        }
    }
    pub(crate) fn incr_invalid_children(&self) {
        self.num_invalid_children.increment();
    }
    pub(crate) fn decr_invalid_children(&self) {
        self.num_invalid_children.increment();
    }

    pub(crate) fn expert_input_type_id(&self, index_of_child_in_parent: i32) -> TypeId {
        let borrow_span = tracing::debug_span!("expert.children.borrow() in expert_input_type_id");
        borrow_span.in_scope(|| {
            let children = self.children.borrow();
            assert!(index_of_child_in_parent >= 0);
            let edge = &children[index_of_child_in_parent as usize];
            edge.edge_input_type_id()
        })
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
        let borrow_span =
            tracing::debug_span!("expert.children.borrow_mut() in ExpertNode::add_child_edge");
        borrow_span.in_scope(|| {
            let mut children = self.children.borrow_mut();
            let new_child_index = children.len() as i32;
            edge.index_cell().set(Some(new_child_index));
            children.push(edge);
            self.force_stale.set(true);
            tracing::debug!("expert added child, ix {new_child_index}");
            new_child_index
        })
    }
    pub(crate) fn swap_children(&self, one: usize, two: usize) {
        let borrow_span =
            tracing::debug_span!("expert.children.borrow_mut() in ExpertNode::swap_children");
        borrow_span.in_scope(|| {
            let mut children = self.children.borrow_mut();
            let c1 = children[one].index_cell();
            let c2 = children[two].index_cell();
            c1.swap(c2);
            children.swap(one, two);
        });
    }
    pub(crate) fn last_child_edge(&self) -> Option<PackedEdge> {
        let children = self.children.borrow();
        children.last().cloned()
    }
    pub(crate) fn pop_child_edge(&self) -> Option<PackedEdge> {
        let mut children = self.children.borrow_mut();
        let packed_edge = children.pop()?;
        self.force_stale.set(true);
        packed_edge.index_cell().set(None);
        Some(packed_edge)
    }
    pub(crate) fn before_main_computation(&self) -> Result<(), Invalid> {
        if self.num_invalid_children.get() > 0 {
            Err(Invalid)
        } else {
            self.force_stale.set(false);
            if self.will_fire_all_callbacks.replace(false) {
                let borrow_span = tracing::debug_span!(
                    "expert.children.borrow_mut() in ExpertNode::before_main_computation"
                );

                let cloned = borrow_span.in_scope(|| self.children.borrow().clone());
                tracing::debug!("running on_change for {} children", cloned.len());
                for child in cloned {
                    child.on_change()
                }
            }
            Ok(())
        }
    }
    pub(crate) fn observability_change(&self, is_now_observable: bool) {
        if let Some(handler) = self.on_observability_change.borrow_mut().as_mut() {
            handler(is_now_observable);
        }
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
            let child = {
                let borrow_span = tracing::debug_span!(
                    "expert.children.borrow_mut() in ExpertNode::run_edge_callback"
                );
                borrow_span.in_scope(|| {
                    let children = self.children.borrow();
                    let Some(child) = children.get(child_index as usize) else {
                        return None;
                    };
                    // clone the child, so we can drop the borrow of the children vector.
                    // the child on_change callback may add or remove children. It needs borrow_mut access!
                    Some(child.clone())
                })
            };
            let Some(child) = child else {
                return;
            };
            child.on_change()
        }
    }
}

pub(crate) struct Invalid;

use core::fmt::Debug;
impl<T, R, O> Debug for ExpertNode<T, R, O>
where
    T: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExpertNode").finish()
    }
}

impl<T, FRecompute, FObsChange> NodeGenerics for ExpertNode<T, FRecompute, FObsChange>
where
    T: Value,
    FRecompute: FnMut() -> T + 'static,
    FObsChange: FnMut(bool) + 'static,
{
    type R = T;
    type Recompute = FRecompute;
    type ObsChange = FObsChange;
    node_generics_default! { I1, I2, I3, I4, I5, I6 }
    node_generics_default! { F1, F2, F3, F4, F5, F6 }
    node_generics_default! { B1, BindLhs, BindRhs }
    node_generics_default! { Fold, Update, WithOld, FRef }
}

pub mod public {
    use std::rc::{Rc, Weak};

    use crate::{Incr, Value, WeakIncr, WeakState};

    use super::Edge;

    #[derive(Clone)]
    pub struct Dependency<T> {
        edge: Weak<Edge<T>>,
    }

    impl<T> core::fmt::Debug for Dependency<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Dependency")
        }
    }
    impl<T> PartialEq for Dependency<T> {
        fn eq(&self, other: &Self) -> bool {
            crate::weak_thin_ptr_eq(&self.edge, &other.edge)
        }
    }

    impl<T> Dependency<T> {
        pub fn node(&self) -> Incr<T> {
            self.edge.upgrade().unwrap().child.clone()
        }
    }

    impl<T: Clone> Dependency<T> {
        pub fn value_cloned(&self) -> T {
            self.edge.upgrade().unwrap().child.node.latest()
        }
    }

    pub struct Node<T> {
        incr: Incr<T>,
    }

    // impl<T> Clone for Node<T> {
    //     fn clone(&self) -> Self {
    //     }
    // }

    use crate::state::expert;
    impl<T: Value> Node<T> {
        pub fn weak(&self) -> WeakNode<T> {
            WeakNode {
                incr: self.incr.weak(),
            }
        }

        pub fn new(state: &WeakState, f: impl FnMut() -> T + 'static) -> Node<T> {
            fn ignore(_: bool) {}
            Node::new_(state, f, ignore)
        }
        pub fn new_(
            state: &WeakState,
            f: impl FnMut() -> T + 'static,
            obs_change: impl FnMut(bool) + 'static,
        ) -> Self {
            let incr = expert::create::<T, _, _>(&state.upgrade_inner().unwrap(), f, obs_change);
            Self { incr }
        }
        pub fn new_cyclic<F>(state: &WeakState, f: impl FnOnce(WeakIncr<T>) -> F) -> Node<T>
        where
            F: FnMut() -> T + 'static,
        {
            fn ignore(_: bool) {}
            Node::new_cyclic_(state, f, ignore)
        }
        pub fn new_cyclic_<F>(
            state: &WeakState,
            f: impl FnOnce(WeakIncr<T>) -> F,
            obs_change: impl FnMut(bool) + 'static,
        ) -> Node<T>
        where
            F: FnMut() -> T + 'static,
        {
            let incr =
                expert::create_cyclic::<T, _, _, _>(&state.upgrade_inner().unwrap(), f, obs_change);
            Self { incr }
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
        pub fn add_dependency<D: Value>(&self, on: &Incr<D>) -> Dependency<D> {
            let edge = Rc::new(Edge::new(on.clone(), None));
            let dep = Dependency {
                edge: Rc::downgrade(&edge),
            };
            expert::add_dependency(&self.incr.node.packed(), edge);
            dep
        }
        /// Add dependency with a change callback.
        ///
        /// Note that you should not use the change callback to
        /// add or remove dependencies. The scheduler isn't smart enough to initialize such a
        /// system and cut off any recursion that results, so it doesn't try. You can implement
        /// Bind-like behaviour by introducing a Map node, adding/removing dynamic dependencies in
        /// the Map function, and then adding a dependency on the Map node. The static dependency
        /// on the Map node ensures all the dynamic dependnencies are resolved before the expert
        /// Node runs. This way you also get cycle detection done by the system.
        pub fn add_dependency_with<D: Value>(
            &self,
            on: &Incr<D>,
            on_change: impl FnMut(&D) + 'static,
        ) -> Dependency<D> {
            let edge = Rc::new(Edge::new(on.clone(), Some(Box::new(on_change))));
            let dep = Dependency {
                edge: Rc::downgrade(&edge),
            };
            expert::add_dependency(&self.incr.node.packed(), edge);
            dep
        }
        /// Caution: if the Dependency is on an expert::Node, then running this may cause
        /// a related WeakNode to be deallocated. If you wish to use the related node after
        /// (i.e. to invalidate it) then upgrade the WeakNode first.
        pub fn remove_dependency<D: Value>(&self, dep: Dependency<D>) {
            let edge = dep.edge.upgrade().unwrap();
            expert::remove_dependency(&*self.incr.node, &*edge);
        }
    }

    #[derive(Clone)]
    pub struct WeakNode<T> {
        incr: WeakIncr<T>,
    }

    impl<T: Value> WeakNode<T> {
        #[inline]
        pub fn watch(&self) -> WeakIncr<T> {
            self.incr.clone()
        }
        #[inline]
        pub fn upgrade(&self) -> Option<Node<T>> {
            self.incr.upgrade().map(|incr| Node { incr })
        }
        #[inline]
        pub fn make_stale(&self) {
            self.upgrade().unwrap().make_stale();
        }
        #[inline]
        pub fn invalidate(&self) {
            self.upgrade().unwrap().invalidate();
        }
        #[inline]
        pub fn add_dependency<D: Value>(&self, on: &Incr<D>) -> Dependency<D> {
            self.upgrade().unwrap().add_dependency(on)
        }
        /// See [Node::add_dependency_with], noting especially that you should not use the on_change
        /// callback to add dynamic dependencies to this expert node.
        pub fn add_dependency_with<D: Value>(
            &self,
            on: &Incr<D>,
            on_change: impl FnMut(&D) + 'static,
        ) -> Dependency<D> {
            self.upgrade().unwrap().add_dependency_with(on, on_change)
        }
        /// Caution: if the Dependency is on an expert::Node, then running this may cause
        /// a related WeakNode to be deallocated. If you wish to use the related node after
        /// (i.e. to invalidate it) then upgrade the WeakNode first.
        pub fn remove_dependency<D: Value>(&self, dep: Dependency<D>) {
            self.upgrade().unwrap().remove_dependency(dep)
        }
    }

    impl<T: Value> AsRef<Incr<T>> for Node<T> {
        #[inline]
        fn as_ref(&self) -> &Incr<T> {
            &self.incr
        }
    }
}

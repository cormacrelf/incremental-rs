use std::{
    cell::{Cell, RefCell},
    marker::PhantomData,
    rc::Rc,
};

use crate::{Value, Incr};

use super::{node::Input, kind::NodeGenerics};

trait ExpertEdge {
    fn on_change(&self);
}

type PackedEdge = Rc<dyn ExpertEdge>;

pub(crate) struct Edge<T> {
    pub node: Input<T>,
    pub on_change: RefCell<Option<Box<dyn FnMut(&T)>>>,
    /* [index] is defined whenever the [edge] is in the [children] of some [t]. Then it is
    the index of this [edge] in that [children] array. It might seem redundant with all
    the other indexes we have, but it is necessary to remove children.  The index may
    change as sibling children are removed. */
    pub index: Cell<i32>,
}

impl<T> Edge<T> {
    fn new(child: Input<T>, on_change: Option<Box<dyn FnMut(&T)>>) -> Self {
        Self {
            node: child,
            on_change: on_change.into(),
            index: 0.into(),
        }
    }
}

impl<T: Value> ExpertEdge for Edge<T> {
    fn on_change(&self) {
        let mut handler = self.on_change.borrow_mut();
        if let Some(h) = &mut *handler {
            let v = self.node.value_as_ref();
            h(v.as_ref().unwrap());
        }
    }
}

pub(crate) struct ExpertNode<T, C, F, ObsChange> {
    pub node: PhantomData<T>,
    pub recompute: RefCell<F>,
    pub on_observability_change: ObsChange,
    pub children: RefCell<Vec<Input<C>>>,
    pub force_stale: Cell<bool>,
}

pub enum MakeStale {
    AlreadyStale,
    Ok,
}

impl<T, C, F> ExpertNode<T, C, F, fn(&T)>
where
    F: FnMut() -> T,
{
    pub(crate) fn new(recompute: F) -> Self {
        fn ignore<T>(t: &T) {}
        ExpertNode::new_obs(recompute, ignore)
    }
}

impl<T, C, F, ObsChange> ExpertNode<T, C, F, ObsChange>
where
    F: FnMut() -> T,
    ObsChange: FnMut(&T),
{
    pub(crate) fn new_obs(recompute: F, on_observability_change: ObsChange) -> Self {
        Self {
            node: PhantomData,
            recompute: recompute.into(), 
            on_observability_change,
            children: vec![].into(),
            force_stale: false.into()
        }
    }
    pub(crate) fn make_stale(&self) -> MakeStale {
        if self.force_stale.get() {
            MakeStale::AlreadyStale
        } else {
            self.force_stale.set(true);
            MakeStale::Ok
        }
    }
}

pub(crate) struct Invalid;

impl<T, C, F, ObsChange> ExpertNode<T, C, F, ObsChange> {
    pub(crate) fn before_main_computation(&self) -> Result<(), Invalid> {
        Ok(())
    }
}

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
    FObsChange: FnMut(&T) + 'static,
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
    use std::rc::Rc;

    use crate::{Incr, IncrState};

    use super::{Edge, ExpertNode};

    pub struct Dependency<T> {
        edge: Rc<Edge<T>>,
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
            self.edge.node.latest()
        }
    }

    pub struct Node<T, C> {
        node: Rc<ExpertNode<T, C, fn() -> T, fn()>>,
    }

    impl<T, C> Node<T, C> {
        pub fn new(state: &IncrState, f: impl FnMut() -> T) -> Self {
            todo!()
        }
        pub fn new_(state: &IncrState, f: fn() -> T, obs_change: fn()) -> Self {
            todo!()
        }
        pub fn watch(&self) -> Incr<T> {
            todo!()
        }
        pub fn make_stale(&self) {
            todo!()
        }
        pub fn invalidate(&self) {
            todo!()
        }
        pub fn add_dependency<D>(&self, dep: Dependency<D>) {
            todo!()
        }
        pub fn remove_dependency<D>(&self, dep: Dependency<D>) {
            todo!()
        }
    }

    #[test]
    fn test() {
        let incr = crate::IncrState::new();
        let node = incr.var(10).watch();
        let dep = Dependency::new(&node);
    }
}

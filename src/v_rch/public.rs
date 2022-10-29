use core::fmt::Debug;
use std::rc::Rc;

pub use super::internal_observer::ObserverError;
use super::internal_observer::{ErasedObserver, InternalObserver};
use super::node::NodeId;
pub use super::state::State;
use super::var::Var as InternalVar;
pub use super::Incr;
pub use super::Value;

#[derive(Clone)]
pub struct Observer<'a, T: Value<'a>> {
    internal: Rc<InternalObserver<'a, T>>,
    sentinel: Rc<()>,
}

impl<'a, T: Value<'a>> Observer<'a, T> {
    pub(crate) fn new(internal: Rc<InternalObserver<'a, T>>) -> Self {
        Self { internal, sentinel: Rc::new(()) }
    }
    #[inline]
    pub fn value(&self) -> Result<T, ObserverError> {
        self.internal.value()
    }
    #[inline]
    pub fn expect_value(&self) -> T {
        self.internal.value().unwrap()
    }
}

impl<'a, T: Value<'a>> Drop for Observer<'a, T> {
    fn drop(&mut self) {
        // all_observers holds another strong reference to internal.
        // but we can be _sure_ we're the last public::Observer by using a sentinel Rc.
        if Rc::strong_count(&self.sentinel) <= 1 {
            // causes it to eventually be dropped
            self.internal.disallow_future_use();
        }
    }
}

// Just to hide the Rc in the interface
#[derive(Debug, Clone)]
pub struct Var<'a, T: Value<'a>> {
    internal: Rc<InternalVar<'a, T>>,
    sentinel: Rc<()>,
}

impl<'a, T: Value<'a>> Var<'a, T> {
    pub(crate) fn new(internal: Rc<InternalVar<'a, T>>) -> Self {
        Self { internal, sentinel: Rc::new(()) }
    }
    #[inline]
    pub fn set(&self, value: T) {
        self.internal.set(value)
    }
    #[inline]
    pub fn get(&self) -> T {
        self.internal.get()
    }
    #[inline]
    pub fn watch(&self) -> Incr<'a, T> {
        self.internal.watch()
    }
    #[inline]
    pub fn id(&self) -> NodeId {
        self.internal.node_id
    }
}

impl<'a, T: Value<'a>> Drop for Var<'a, T> {
    fn drop(&mut self) {
        println!("dropping public::Var with id {:?}", self.id());
        // one is for us; one is for the watch node.
        // if it's down to 2 (i.e. all public::Vars have been dropped),
        // then we need to break the Rc cycle between Var & Node (via Kind::Var(Rc<...>)).
        //
        // we can be _sure_ we're the last public::Var by using a sentinel Rc.
        // i.e. this Var will never get set again.
        if Rc::strong_count(&self.sentinel) <= 1 {
            let state = self.internal.state.upgrade().unwrap();
            let mut dead_vars = state.dead_vars.borrow_mut();
            dead_vars.push(self.internal.erased());
        }
    }
}

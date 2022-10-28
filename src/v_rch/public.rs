use core::fmt::Debug;
use std::rc::Rc;

use super::internal_observer::InternalObserver;
pub use super::internal_observer::ObserverError;
use super::node::NodeId;
pub use super::state::State;
use super::var::Var as InternalVar;
pub use super::Incr;
pub use super::Value;

#[derive(Clone)]
pub struct Observer<'a, T> {
    internal: Rc<InternalObserver<'a, T>>,
}

impl<'a, T: Value<'a>> Observer<'a, T> {
    pub(crate) fn new(internal: Rc<InternalObserver<'a, T>>) -> Self {
        Self { internal }
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

// Just to hide the Rc in the interface
#[derive(Debug, Clone)]
pub struct Var<'a, T: Value<'a>> {
    internal: Rc<InternalVar<'a, T>>,
}

impl<'a, T: Value<'a>> Var<'a, T> {
    pub(crate) fn new(internal: Rc<InternalVar<'a, T>>) -> Self {
        Self { internal }
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
        // drop the reference to the node. This is necessary because Var and Node are in an Rc cycle,
        // so one of them needs to manually drop its Rc handle lest they each never get dropped.
        //
        // What happens when we do this from here?
        //
        // The internal.node itself will live on, as long as it's a part of some computation graph.
        // We only needed a ref to it here in order to create new .watch()s, and we have been
        // dropped. so we're done.
        //
        // Does Node itself still need var's reference to the containing Node? No. So it's fine to
        // kill the ref.
        *self.internal.node.borrow_mut() = None;
    }
}

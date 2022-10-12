use core::fmt::Debug;

use super::internal_observer::InternalObserver;
pub use super::internal_observer::ObserverError;
use super::node::NodeId;
pub use super::state::State;
use super::var::Var as InternalVar;
pub use super::Incr;

#[derive(Clone)]
pub struct Observer<'a, T> {
    internal: &'a InternalObserver<'a, T>,
}

impl<'a, T: Debug + Clone + 'a> Observer<'a, T> {
    pub(crate) fn new(internal: &'a InternalObserver<'a, T>) -> Self {
        Self { internal }
    }
    #[inline]
    pub fn value(&self) -> Result<T, ObserverError> {
        self.internal.value()
    }
    #[inline]
    pub fn value_unwrap(&self) -> T {
        self.internal.value().unwrap()
    }
}

// Just to hide the Rc in the interface
#[derive(Clone)]
pub struct Var<'a, T: Debug + Clone + 'a> {
    internal: &'a InternalVar<'a, T>,
}

impl<'a, T: Debug + Clone + 'a> Var<'a, T> {
    pub(crate) fn new(internal: &'a InternalVar<'a, T>) -> Self {
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
        self.internal.node.borrow().as_ref().unwrap().id
    }
}

impl<'a, T: Debug + Clone + 'a> Drop for Var<'a, T> {
    fn drop(&mut self) {
        // drop the reference to the node.
        // a reference may still be held by the .watch() Incr. but we only needed it in
        // order to create new .watch()s, and we have been dropped. so we're done.
        //
        // Does Node itself still need var's reference to itself? no. So it's fine for
        // InternalVar to no longer store &'a Node.
        *self.internal.node.borrow_mut() = None;
    }
}

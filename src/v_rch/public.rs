use core::fmt::Debug;
use std::rc::Rc;

use super::internal_observer::InternalObserver;
pub use super::internal_observer::ObserverError;
pub use super::state::State;
use super::var::Var as InternalVar;
pub use super::Incr;

#[derive(Clone)]
pub struct Observer<T> {
    internal: Rc<InternalObserver<T>>,
}

impl<T: Debug + Clone + 'static> Observer<T> {
    pub(crate) fn new(internal: Rc<InternalObserver<T>>) -> Self {
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
pub struct Var<T: Debug + Clone + 'static> {
    internal: Rc<InternalVar<T>>,
}

impl<T: Debug + Clone + 'static> Var<T> {
    pub(crate) fn new(internal: Rc<InternalVar<T>>) -> Self {
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
    pub fn watch(&self) -> Incr<T> {
        self.internal.watch()
    }
}

impl<T: Debug + Clone + 'static> Drop for Var<T> {
    fn drop(&mut self) {
        // drop the reference to the node.
        // a reference may still be held by the .watch() Incr. but we only needed it in
        // order to create new .watch()s, and we have been dropped. so we're done.
        //
        // Does Node itself still need var's reference to itself? no. So it's fine for
        // InternalVar to no longer store Rc<Node>.
        *self.internal.node.borrow_mut() = None;
    }
}

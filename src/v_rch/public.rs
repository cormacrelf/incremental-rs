use core::fmt::Debug;
use std::rc::Rc;

use super::internal_observer::InternalObserver;
pub use super::state::State;
pub use super::Incr;
use super::var::Var as InternalVar;

#[derive(Clone)]
pub struct Observer<T> {
    internal: Rc<InternalObserver<T>>,
}

impl<T: Debug + Clone + 'static> Observer<T> {
    pub(crate) fn new(internal: Rc<InternalObserver<T>>) -> Self {
        Self { internal }
    }
    #[inline]
    pub fn value(&self) -> T {
        self.internal.value()
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

use std::fmt::Debug;
use std::{cell::Cell, rc::Weak};

use super::node::PackedNode;
use super::Incr;

pub(crate) type WeakObserver = Weak<dyn ErasedObserver>;

pub(crate) trait ErasedObserver: Debug + 'static {
    fn use_is_allowed(&self) -> bool;
    fn state(&self) -> &Cell<State>;
    fn observing(&self) -> PackedNode;
}

pub struct InternalObserver<T> {
    state: Cell<State>,
    observing: Incr<T>,
    on_update_handlers: (),
}

impl<T> Debug for InternalObserver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InternalObserver")
            .field("state", &self.state.get())
            .finish()
    }
}

impl<T: Debug + Clone + 'static> InternalObserver<T> {
    pub(crate) fn new(observing: Incr<T>) -> Self {
        Self {
            state: Cell::new(State::Created),
            observing,
            on_update_handlers: (),
        }
    }
    pub(crate) fn value(&self) -> T {
        self.observing.value()
    }
}
impl<T: 'static> ErasedObserver for InternalObserver<T> {
    fn use_is_allowed(&self) -> bool {
        match self.state.get() {
            State::Created | State::InUse => true,
            State::Disallowed | State::Unlinked => false,
        }
    }
    fn state(&self) -> &Cell<State> {
        &self.state
    }
    fn observing(&self) -> PackedNode {
        self.observing.node.clone().packed()
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum State {
    Created,
    InUse,
    Disallowed,
    Unlinked,
}

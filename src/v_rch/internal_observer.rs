use std::fmt::{Debug, Display};
use std::{cell::Cell, rc::Weak};

use super::{NodeRef, Value};
use super::Incr;

pub(crate) type WeakObserver<'a> = Weak<dyn ErasedObserver<'a>>;

pub(crate) trait ErasedObserver<'a>: Debug + 'a {
    fn use_is_allowed(&self) -> bool;
    fn state(&self) -> &Cell<State>;
    fn observing(&self) -> NodeRef<'a>;
}

pub struct InternalObserver<'a, T> {
    state: Cell<State>,
    observing: Incr<'a, T>,
    on_update_handlers: (),
}

#[derive(Debug, PartialEq, Eq, Clone)]
#[non_exhaustive]
pub enum ObserverError {
    CurrentlyStabilising,
    NeverStabilised,
    Disallowed,
    ObservingInvalid,
}

impl Display for ObserverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CurrentlyStabilising => write!(f, "Incremental is currently stabilising. You cannot call Observer::value inside e.g. a map or bind function."),
            Self::NeverStabilised => write!(f, "Incremental has never stabilised. Observer does not yet have a value."),
            Self::Disallowed => write!(f, "Observer has been disallowed"),
            Self::ObservingInvalid => write!(f, "observing an invalid Incr"),
        }
    }
}
impl std::error::Error for ObserverError {}

impl<'a, T: Value<'a>> Debug for InternalObserver<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InternalObserver")
            .field("state", &self.state.get())
            .field("value", &self.value())
            .finish()
    }
}

impl<'a, T: Value<'a>> InternalObserver<'a, T> {
    pub(crate) fn new(observing: Incr<'a, T>) -> Self {
        Self {
            state: Cell::new(State::Created),
            observing,
            on_update_handlers: (),
        }
    }
    pub(crate) fn value(&self) -> Result<T, ObserverError> {
        match self.state.get() {
            State::Created => Err(ObserverError::NeverStabilised),
            State::InUse => self
                .observing
                .node
                .value_opt()
                .ok_or(ObserverError::ObservingInvalid),
            State::Disallowed | State::Unlinked => Err(ObserverError::Disallowed),
        }
    }
}
impl<'a, T: Value<'a>> ErasedObserver<'a> for InternalObserver<'a, T> {
    fn use_is_allowed(&self) -> bool {
        match self.state.get() {
            State::Created | State::InUse => true,
            State::Disallowed | State::Unlinked => false,
        }
    }
    fn state(&self) -> &Cell<State> {
        &self.state
    }
    fn observing(&self) -> NodeRef<'a> {
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

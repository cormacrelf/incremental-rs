use std::fmt::{Debug, Display};
use std::rc::Rc;
use std::{cell::Cell, rc::Weak};
use super::state::State;
use std::hash::Hash;

use super::Incr;
use super::{NodeRef, Value};

use self::ObserverState::*;

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct ObserverId(usize);
impl ObserverId {
    fn next() -> Self {
        thread_local! {
            static OBSERVER_ID: Cell<usize> = Cell::new(0);
        }

        OBSERVER_ID.with(|x| {
            let next = x.get() + 1;
            x.set(next);
            ObserverId(next)
        })
    }
}

pub(crate) struct InternalObserver<'a, T> {
    id: ObserverId,
    pub(crate) state: Cell<ObserverState>,
    observing: Incr<'a, T>,
    weak_self: WeakObserver<'a>,
    on_update_handlers: (),
}

pub(crate) type WeakObserver<'a> = Weak<dyn ErasedObserver<'a> + 'a>;
pub(crate) type StrongObserver<'a> = Rc<dyn ErasedObserver<'a> + 'a>;

pub(crate) trait ErasedObserver<'a>: Debug + 'a {
    fn id(&self) -> ObserverId;
    fn use_is_allowed(&self) -> bool;
    fn state(&self) -> &Cell<ObserverState>;
    fn observing(&self) -> NodeRef<'a>;
    fn disallow_future_use(&self, state: &State<'a>);
}

impl<'a, T: Value<'a>> ErasedObserver<'a> for InternalObserver<'a, T> {
    fn id(&self) -> ObserverId {
        self.id
    }
    fn use_is_allowed(&self) -> bool {
        match self.state.get() {
            Created | InUse => true,
            Disallowed | Unlinked => false,
        }
    }
    fn state(&self) -> &Cell<ObserverState> {
        &self.state
    }
    fn observing(&self) -> NodeRef<'a> {
        self.observing.node.clone().packed()
    }
    fn disallow_future_use(&self, state: &State<'a>) {
        match self.state.get() {
            Disallowed | Unlinked => {}
            Created => {
                state.num_active_observers.set(state.num_active_observers.get() - 1);
                self.state.set(Unlinked);
                // self.on_update_handlers = ();
            }
            InUse => {
                state.num_active_observers.set(state.num_active_observers.get() - 1);
                self.state.set(Disallowed);
                let mut dobs = state.disallowed_observers.borrow_mut();
                dobs.push(self.weak_self.clone());
            }
        }
    }
}

impl<'a, T: Value<'a>> Debug for InternalObserver<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InternalObserver")
            .field("state", &self.state.get())
            .field("value", &self.value())
            .finish()
    }
}

impl<'a, T: Value<'a>> InternalObserver<'a, T> {
    pub(crate) fn incr_state(&self) -> Option<Rc<State<'a>>> {
        self.observing.node.state_opt()
    }
    pub(crate) fn new(observing: Incr<'a, T>) -> Rc<Self> {
        Rc::new_cyclic(|weak_self| { Self {
            id: ObserverId::next(),
            state: Cell::new(Created),
            observing,
            on_update_handlers: (),
            weak_self: weak_self.clone() as WeakObserver<'a>,
        } })
    }
    pub(crate) fn value(&self) -> Result<T, ObserverError> {
        match self.state.get() {
            Created => Err(ObserverError::NeverStabilised),
            InUse => self
                .observing
                .node
                .value_opt()
                .ok_or(ObserverError::ObservingInvalid),
            Disallowed | Unlinked => Err(ObserverError::Disallowed),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum ObserverState {
    Created,
    InUse,
    Disallowed,
    Unlinked,
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


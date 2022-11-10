use super::node::Input;
use super::node_update::{NodeUpdateDelayed, OnUpdateHandler};
use super::stabilisation_num::StabilisationNum;
use super::state::{IncrStatus, State};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::rc::Rc;
use std::{cell::Cell, rc::Weak};

use super::{CellIncrement, Incr};
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

pub(crate) struct InternalObserver<T> {
    id: ObserverId,
    pub(crate) state: Cell<ObserverState>,
    observing: Incr<T>,
    weak_self: Weak<Self>,
    on_update_handlers: RefCell<HashMap<SubscriptionToken, OnUpdateHandler<T>>>,
    next_subscriber: Cell<SubscriptionToken>,
}

pub(crate) type WeakObserver = Weak<dyn ErasedObserver>;
pub(crate) type StrongObserver = Rc<dyn ErasedObserver>;

pub(crate) trait ErasedObserver: Debug {
    fn id(&self) -> ObserverId;
    fn use_is_allowed(&self) -> bool;
    fn state(&self) -> &Cell<ObserverState>;
    fn observing(&self) -> NodeRef;
    fn disallow_future_use(&self, state: &State);
    fn num_handlers(&self) -> i32;
    fn add_to_observed_node(&self);
    fn remove_from_observed_node(&self);
    fn unsubscribe(&self, token: SubscriptionToken) -> Result<(), ObserverError>;
}

impl<T: Value> ErasedObserver for InternalObserver<T> {
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
    fn observing(&self) -> NodeRef {
        self.observing.node.clone().packed()
    }
    fn disallow_future_use(&self, state: &State) {
        match self.state.get() {
            Disallowed | Unlinked => {}
            Created => {
                state
                    .num_active_observers
                    .set(state.num_active_observers.get() - 1);
                self.state.set(Unlinked);
                let mut ouh = self.on_update_handlers.borrow_mut();
                ouh.clear();
            }
            InUse => {
                state
                    .num_active_observers
                    .set(state.num_active_observers.get() - 1);
                self.state.set(Disallowed);
                let mut dobs = state.disallowed_observers.borrow_mut();
                dobs.push(self.weak_self.clone());
            }
        }
    }
    fn num_handlers(&self) -> i32 {
        self.on_update_handlers.borrow().len() as i32
    }
    fn add_to_observed_node(&self) {
        let node = &self.observing.node;
        let was_necessary = node.is_necessary();
        node.add_observer(self.id(), self.weak_self.clone());
        let num = node.num_on_update_handlers();
        num.set(num.get() + self.num_handlers());
    }
    fn remove_from_observed_node(&self) {
        let node = &self.observing.node;
        node.remove_observer(self.id());
        let num = node.num_on_update_handlers();
        num.set(num.get() - self.num_handlers());
    }

    // This is not available in OCaml Incremental, it seems!
    fn unsubscribe(&self, token: SubscriptionToken) -> Result<(), ObserverError> {
        if token.0 != self.id {
            return Err(ObserverError::Mismatch);
        }
        match self.state.get() {
            // In these cases, on_update_handlers is already cleared.
            // it's fine to try to unsubscribe from a dead/dying subscriber.
            // That will generally happen through State::unsubscribe
            // (which routes it to here through all_observers.get(...)).
            Disallowed | Unlinked => Ok(()),
            Created | InUse => {
                // delete from the list in either case
                self.on_update_handlers.borrow_mut().remove(&token);

                match self.state.get() {
                    Created => {
                        // No need to do a big cleanup. We haven't done the batch add yet in state.rs.
                        Ok(())
                    }
                    InUse => {
                        let observing = self.observing();
                        let num = observing.num_on_update_handlers();
                        num.increment();
                        Ok(())
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
}

impl<T: Value> Debug for InternalObserver<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InternalObserver")
            .field("state", &self.state.get())
            .field("value", &self.value())
            .finish()
    }
}

impl<T: Value> InternalObserver<T> {
    pub(crate) fn incr_state(&self) -> Option<Rc<State>> {
        self.observing.node.state_opt()
    }
    pub(crate) fn new(observing: Incr<T>) -> Rc<Self> {
        let id = ObserverId::next();
        Rc::new_cyclic(|weak_self| Self {
            id,
            state: Cell::new(Created),
            observing,
            on_update_handlers: Default::default(),
            weak_self: weak_self.clone(),
            next_subscriber: SubscriptionToken(id, 1).into(),
        })
    }
    pub(crate) fn value(&self) -> Result<T, ObserverError> {
        let t = self.incr_state();
        match t {
            Some(t) => match t.status.get() {
                IncrStatus::NotStabilising | IncrStatus::RunningOnUpdateHandlers => {
                    self.value_inner()
                }
                IncrStatus::Stabilising => Err(ObserverError::CurrentlyStabilising),
            },
            // the whole state is dead... so is the node, methinks.
            None => Err(ObserverError::ObservingInvalid),
        }
    }
    pub(crate) fn value_inner(&self) -> Result<T, ObserverError> {
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
    pub(crate) fn subscribe(
        &self,
        handler: OnUpdateHandler<T>,
    ) -> Result<SubscriptionToken, ObserverError> {
        match self.state.get() {
            Disallowed | Unlinked => Err(ObserverError::Disallowed),
            Created | InUse => {
                let token = self.next_subscriber.get();
                self.next_subscriber.set(token.succ());
                self.on_update_handlers.borrow_mut().insert(token, handler);
                match self.state.get() {
                    Created => {
                        /* We'll bump [observing.num_on_update_handlers] when [t] is actually added to
                        [observing.observers] at the start of the next stabilization. */
                    }
                    InUse => {
                        let observing = self.observing();
                        let num = observing.num_on_update_handlers();
                        num.set(num.get() + 1);
                    }
                    _ => unreachable!(),
                }
                Ok(token)
            }
        }
    }
    pub(crate) fn run_all(
        &self,
        input: &Input<T>,
        node_update: NodeUpdateDelayed,
        now: StabilisationNum,
    ) {
        let mut handlers = self.on_update_handlers.borrow_mut();
        for (id, handler) in handlers.iter_mut() {
            tracing::trace!("running update handler with id {id:?}");
            handler.run(input, node_update, now);
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct SubscriptionToken(ObserverId, i32);

impl SubscriptionToken {
    fn succ(&self) -> Self {
        Self(self.0, self.1 + 1)
    }
    pub(crate) fn observer_id(&self) -> ObserverId {
        self.0
    }
}

/// State transitions:
///
/// ```ignore
/// Created --> In_use --> Disallowed --> Unlinked
///    |                                     ^
///    \-------------------------------------/
/// ```
///
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
    Mismatch,
}

impl Display for ObserverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CurrentlyStabilising => write!(f, "Incremental is currently stabilising. You cannot call Observer::value inside e.g. a map or bind function."),
            Self::NeverStabilised => write!(f, "Incremental has never stabilised. Observer does not yet have a value."),
            Self::Disallowed => write!(f, "Observer has been disallowed"),
            Self::ObservingInvalid => write!(f, "observing an invalid Incr"),
            Self::Mismatch => write!(f, "called unsubscribe with the wrong observer"),
        }
    }
}
impl std::error::Error for ObserverError {}

#[cfg(debug_assertions)]
impl<T> Drop for InternalObserver<T> {
    fn drop(&mut self) {
        let count = Rc::strong_count(&self.observing.node);
        tracing::warn!(
            "dropping InternalObserver with id {:?}, observing node with strong_count {count}",
            self.id
        );
        debug_assert!(matches!(self.state.get(), Disallowed | Unlinked));
    }
}

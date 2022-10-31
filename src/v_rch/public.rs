use core::fmt::Debug;
use std::ops::Deref;
use std::rc::Rc;

pub use super::cutoff::Cutoff;
pub use super::internal_observer::{ObserverError, SubscriptionToken};
pub use super::node_update::NodeUpdate;
pub use super::state::{State, Stats, StatsDiff};
pub use super::Incr;
pub use super::Value;

use super::internal_observer::{ErasedObserver, InternalObserver};
use super::node::NodeId;
use super::node_update::OnUpdateHandler;
use super::var::{ErasedVariable, Var as InternalVar};

#[derive(Clone)]
pub struct Observer<'a, T: Value<'a>> {
    internal: Rc<InternalObserver<'a, T>>,
    sentinel: Rc<()>,
}

impl<'a, T: Value<'a>> Observer<'a, T> {
    pub(crate) fn new(internal: Rc<InternalObserver<'a, T>>) -> Self {
        Self {
            internal,
            sentinel: Rc::new(()),
        }
    }
    #[inline]
    pub fn value(&self) -> Result<T, ObserverError> {
        self.internal.value()
    }
    #[inline]
    pub fn expect_value(&self) -> T {
        self.internal.value().unwrap()
    }

    /// Equivalent of `observer_on_update_exn`
    pub fn subscribe(
        &self,
        on_update: impl FnMut(NodeUpdate<&T>) + 'a,
    ) -> Result<SubscriptionToken, ObserverError> {
        let handler_fn = Box::new(on_update);
        let state = self
            .internal
            .incr_state()
            .ok_or(ObserverError::ObservingInvalid)?;
        let now = state.stabilisation_num.get();
        let handler = OnUpdateHandler::new(now, handler_fn);
        let token = self.internal.subscribe(handler)?;
        let node = self.internal.observing();
        node.handle_after_stabilisation();
        Ok(token)
    }

    #[inline]
    pub fn unsubscribe(&self, token: SubscriptionToken) -> Result<(), ObserverError> {
        self.internal.unsubscribe(token)
    }
}

impl<'a, T: Value<'a>> Drop for Observer<'a, T> {
    fn drop(&mut self) {
        // all_observers holds another strong reference to internal. but we can be _sure_ we're the last public::Observer by using a sentinel Rc.
        if Rc::strong_count(&self.sentinel) <= 1 {
            if let Some(state) = self.internal.incr_state() {
                // causes it to eventually be dropped
                self.internal.disallow_future_use(&*state);
            } else {
                // if state is already dead, or is currently in the process of being dropped and
                // has triggered Observer::drop because an Observer was owned by some other node
                // by being used in its map() function etc (ugly, I know) then we don't need to
                // do disallow_future_use.
                // We'll just write this to be sure?
                self.internal
                    .state
                    .set(super::internal_observer::ObserverState::Disallowed);
            }
        }
    }
}

// Just to hide the Rc in the interface
#[derive(Debug, Clone)]
pub struct Var<'a, T: Value<'a>> {
    internal: Rc<InternalVar<'a, T>>,
    sentinel: Rc<()>,
    // for the Deref impl
    watch: Incr<'a, T>,
}

impl<'a, T: Value<'a>> PartialEq for Var<'a, T> {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl<'a, T: Value<'a>> Deref for crate::Var<'a, T> {
    type Target = Incr<'a, T>;
    fn deref(&self) -> &Self::Target {
        &self.watch
    }
}

impl<'a, T: Value<'a>> Var<'a, T> {
    pub(crate) fn new(internal: Rc<InternalVar<'a, T>>) -> Self {
        Self {
            watch: internal.watch(),
            internal,
            sentinel: Rc::new(()),
        }
    }
    #[inline]
    pub fn set(&self, value: T) {
        self.internal.set(value)
    }
    #[inline]
    pub fn update(&self, f: impl Fn(&mut T)) {
        self.internal.update(f)
    }
    #[inline]
    pub fn get(&self) -> T {
        self.internal.get()
    }
    #[inline]
    pub fn watch(&self) -> Incr<'a, T> {
        self.watch.clone()
    }
    #[inline]
    pub fn id(&self) -> NodeId {
        self.internal.node_id
    }
    // A duplicate to "override" the cutoff function
    // that our Deref target Incr has, and get the same
    // kind of function call to return Self.
    #[inline]
    pub fn cutoff(self, cutoff: Cutoff<T>) -> Self {
        self.watch.set_cutoff(cutoff);
        self
    }
}

impl<'a, T: Value<'a>> Drop for Var<'a, T> {
    fn drop(&mut self) {
        tracing::trace!("dropping public::Var with id {:?}", self.id());
        // one is for us; one is for the watch node.
        // if it's down to 2 (i.e. all public::Vars have been dropped),
        // then we need to break the Rc cycle between Var & Node (via Kind::Var(Rc<...>)).
        //
        // we can be _sure_ we're the last public::Var by using a sentinel Rc.
        // i.e. this Var will never get set again.
        if Rc::strong_count(&self.sentinel) <= 1 {
            if let Some(state) = self.internal.state.upgrade() {
                // we add the var to a delay queue, in order to ensure that
                // `self.internal.break_rc_cyle()` happens after any potential use
                // via set_during_stabilisation.
                // See `impl Drop for State` for more.
                let mut dead_vars = state.dead_vars.borrow_mut();
                dead_vars.push(self.internal.erased());
            } else {
                // no stabilise will ever run, so we don't need to add this to a delay queue
                self.internal.break_rc_cycle();
            }
        }
    }
}

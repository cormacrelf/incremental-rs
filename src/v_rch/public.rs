use core::fmt::Debug;
use std::fmt;
use std::ops::{Deref, Sub};
use std::rc::Rc;

pub use super::cutoff::Cutoff;
pub use super::internal_observer::{ObserverError, SubscriptionToken};
pub use super::node_update::NodeUpdate;
pub use super::Incr;
pub use super::Value;
pub use super::node::GraphvizDot;

use super::internal_observer::{ErasedObserver, InternalObserver};
use super::node::NodeId;
use super::node_update::OnUpdateHandler;
use super::scope::Scope;
use super::state::State;
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

    pub fn save_dot_to_file(&self, named: &str) {
        GraphvizDot::new_erased(self.internal.observing())
            .save_to_file(named)
            .unwrap();
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
#[derive(Clone)]
pub struct Var<'a, T: Value<'a>> {
    internal: Rc<InternalVar<'a, T>>,
    sentinel: Rc<()>,
    // for the Deref impl
    watch: Incr<'a, T>,
}

impl<'a, T: Value<'a>> fmt::Debug for Var<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut tuple = f.debug_tuple("Var");
        let internal = self.internal.value.borrow();
        tuple
            .field(&self.id())
            .field(&*internal)
            .finish()
    }
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

#[derive(Clone, Debug)]
pub struct IncrState<'a>(pub(crate) Rc<State<'a>>);

impl<'a> IncrState<'a> {
    pub fn new() -> Self {
        Self(State::new())
    }

    pub fn stabilise(&self) {
        self.0.stabilise();
    }

    #[inline]
    pub fn constant<T: Value<'a>>(&self, value: T) -> Incr<'a, T> {
        self.0.constant(value)
    }

    pub fn fold<F, T: Value<'a>, R: Value<'a>>(
        &self,
        vec: Vec<Incr<'a, T>>,
        init: R,
        f: F,
    ) -> Incr<'a, R>
    where
        F: FnMut(R, &T) -> R + 'a,
    {
        self.0.fold(vec, init, f)
    }

    pub fn unordered_fold_inverse<F, FInv, T: Value<'a>, R: Value<'a>>(
        &self,
        vec: Vec<Incr<'a, T>>,
        init: R,
        f: F,
        f_inverse: FInv,
        full_compute_every_n_changes: Option<u32>,
    ) -> Incr<'a, R>
    where
        F: FnMut(R, &T) -> R + Clone + 'a,
        FInv: FnMut(R, &T) -> R + 'a,
    {
        self.0
            .unordered_fold_inverse(vec, init, f, f_inverse, full_compute_every_n_changes)
    }

    pub fn unordered_fold<F, U, T: Value<'a>, R: Value<'a>>(
        &self,
        vec: Vec<Incr<'a, T>>,
        init: R,
        f: F,
        update: U,
        full_compute_every_n_changes: Option<u32>,
    ) -> Incr<'a, R>
    where
        F: FnMut(R, &T) -> R + 'a,
        U: FnMut(R, &T, &T) -> R + 'a,
    {
        self.0
            .unordered_fold(vec, init, f, update, full_compute_every_n_changes)
    }

    pub fn var<T: Value<'a>>(&self, value: T) -> Var<'a, T> {
        self.0.var_in_scope(value, Scope::Top)
    }

    pub fn var_current_scope<T: Value<'a>>(&self, value: T) -> Var<'a, T> {
        self.0.var_in_scope(value, self.0.current_scope())
    }

    pub fn unsubscribe(&self, token: SubscriptionToken) {
        self.0.unsubscribe(token)
    }

    pub fn set_max_height_allowed(&self, new_max_height: usize) {
        self.0.set_max_height_allowed(new_max_height)
    }

    pub fn stats(&self) -> Stats {
        Stats {
            created: self.0.num_nodes_created.get(),
            changed: self.0.num_nodes_changed.get(),
            recomputed: self.0.num_nodes_recomputed.get(),
            invalidated: self.0.num_nodes_invalidated.get(),
            became_necessary: self.0.num_nodes_became_necessary.get(),
            became_unnecessary: self.0.num_nodes_became_unnecessary.get(),
            necessary: self.0.num_nodes_became_necessary.get()
                - self.0.num_nodes_became_unnecessary.get(),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Stats {
    pub created: usize,
    pub changed: usize,
    pub recomputed: usize,
    pub invalidated: usize,
    pub became_necessary: usize,
    pub became_unnecessary: usize,
    pub necessary: usize,
}

#[derive(Copy, Clone, PartialEq, Eq, Default)]
pub struct StatsDiff {
    pub created: isize,
    pub changed: isize,
    pub recomputed: isize,
    pub invalidated: isize,
    pub became_necessary: isize,
    pub became_unnecessary: isize,
    pub necessary: isize,
}

impl Debug for StatsDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut f = f.debug_struct("StatsDiff");
        let mut field = |name: &str, x: isize| {
            if x != 0 {
                f.field(name, &x);
            }
        };
        field("created", self.created);
        field("changed", self.changed);
        field("recomputed", self.recomputed);
        field("invalidated", self.invalidated);
        field("became_necessary", self.became_necessary);
        field("became_unnecessary", self.became_unnecessary);
        field("necessary", self.necessary);
        drop(field);
        f.finish()
    }
}

impl Stats {
    pub fn diff(&self, other: Self) -> StatsDiff {
        StatsDiff {
            created: self.created as isize - other.created as isize,
            changed: self.changed as isize - other.changed as isize,
            recomputed: self.recomputed as isize - other.recomputed as isize,
            invalidated: self.invalidated as isize - other.invalidated as isize,
            became_necessary: self.became_necessary as isize - other.became_necessary as isize,
            became_unnecessary: self.became_unnecessary as isize
                - other.became_unnecessary as isize,
            necessary: self.necessary as isize - other.necessary as isize,
        }
    }
}

impl Sub for Stats {
    type Output = StatsDiff;
    fn sub(self, rhs: Self) -> Self::Output {
        self.diff(rhs)
    }
}

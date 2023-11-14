use core::fmt::Debug;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::ops::{Deref, Sub};
use std::rc::{Rc, Weak};

pub use super::cutoff::Cutoff;
pub use super::incr::{Incr, WeakIncr};
pub use super::internal_observer::{ObserverError, SubscriptionToken};
pub use super::kind::expert::public as expert;
pub use super::node_update::NodeUpdate;
#[doc(inline)]
pub use super::Value;

use super::internal_observer::{ErasedObserver, InternalObserver};
use super::node::NodeId;
use super::node_update::OnUpdateHandler;
use super::scope;
use super::state::State;
use super::var::{ErasedVariable, Var as InternalVar};

#[derive(Clone)]
pub struct Observer<T: Value> {
    internal: Rc<InternalObserver<T>>,
    sentinel: Rc<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Update<T> {
    Initialised(T),
    Changed(T),
    Invalidated,
}
impl<T> Update<T> {
    pub fn value(self) -> Option<T> {
        match self {
            Self::Initialised(t) => Some(t),
            Self::Changed(t) => Some(t),
            _ => None,
        }
    }
}
impl<T> Update<&T> {
    pub fn cloned(&self) -> Update<T>
    where
        T: Clone,
    {
        match *self {
            Self::Initialised(t) => Update::Initialised(t.clone()),
            Self::Changed(t) => Update::Changed(t.clone()),
            Self::Invalidated => Update::Invalidated,
        }
    }
}

impl<T: Value> Observer<T> {
    pub(crate) fn new(internal: Rc<InternalObserver<T>>) -> Self {
        Self {
            internal,
            sentinel: Rc::new(()),
        }
    }
    #[inline]
    pub fn try_get_value(&self) -> Result<T, ObserverError> {
        self.internal.try_get_value()
    }
    #[inline]
    pub fn value(&self) -> T {
        self.internal.try_get_value().unwrap()
    }

    pub fn subscribe(&self, on_update: impl FnMut(Update<&T>) + 'static) -> SubscriptionToken {
        self.try_subscribe(on_update).unwrap()
    }

    pub fn try_subscribe(
        &self,
        mut on_update: impl FnMut(Update<&T>) + 'static,
    ) -> Result<SubscriptionToken, ObserverError> {
        let handler_fn = Box::new(move |node_update: NodeUpdate<&T>| {
            let update = match node_update {
                NodeUpdate::Necessary(t) => Update::Initialised(t),
                NodeUpdate::Changed(t) => Update::Changed(t),
                NodeUpdate::Invalidated => Update::Invalidated,
                NodeUpdate::Unnecessary => {
                    panic!("Incremental bug -- Observer subscription got NodeUpdate::Unnecessary")
                }
            };
            on_update(update)
        });
        let state = self
            .internal
            .incr_state()
            .ok_or(ObserverError::ObservingInvalid)?;
        let now = state.stabilisation_num.get();
        let handler = OnUpdateHandler::new(now, handler_fn);
        let token = self.internal.subscribe(handler)?;
        let node = self.internal.observing_erased();
        let state = node.state();
        node.handle_after_stabilisation(&state);
        Ok(token)
    }

    #[inline]
    pub fn state(&self) -> WeakState {
        self.internal
            .incr_state()
            .map_or_else(|| WeakState { inner: Weak::new() }, |s| s.public_weak())
    }

    #[inline]
    pub fn unsubscribe(&self, token: SubscriptionToken) -> Result<(), ObserverError> {
        self.internal.unsubscribe(token)
    }

    pub fn save_dot_to_file(&self, named: &str) {
        let node = self.internal.observing_erased();
        super::node::save_dot_to_file(&mut core::iter::once(node), named).unwrap();
    }

    pub fn save_dot_to_string(&self) -> String {
        let node = self.internal.observing_erased();
        let mut buf = String::new();
        super::node::save_dot(&mut buf, &mut core::iter::once(node)).unwrap();
        buf
    }

    pub fn disallow_future_use(&self) {
        let Some(state) = self.internal.incr_state() else {
            return;
        };
        self.internal.disallow_future_use(&state);
    }
}

impl<T: Value> Drop for Observer<T> {
    fn drop(&mut self) {
        // all_observers holds another strong reference to internal. but we can be _sure_ we're the last public::Observer by using a sentinel Rc.
        if Rc::strong_count(&self.sentinel) <= 1 {
            if let Some(state) = self.internal.incr_state() {
                // causes it to eventually be dropped
                self.internal.disallow_future_use(&state);
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
pub struct Var<T: Value> {
    internal: Rc<InternalVar<T>>,
    sentinel: Rc<()>,
    // for the Deref impl
    watch: Incr<T>,
}

impl<T: Value> fmt::Debug for Var<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut tuple = f.debug_tuple("Var");
        let internal = self.internal.value.borrow();
        tuple.field(&self.id()).field(&*internal).finish()
    }
}

impl<T: Value> PartialEq for Var<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl<T: Value> Deref for Var<T> {
    type Target = Incr<T>;
    fn deref(&self) -> &Self::Target {
        &self.watch
    }
}

impl<T: Value> Var<T> {
    pub(crate) fn new(internal: Rc<InternalVar<T>>) -> Self {
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
    pub fn was_changed_during_stabilisation(&self) -> bool {
        self.internal.was_changed_during_stabilisation()
    }

    /// Takes the current value, replaces it using the function provided,
    /// and queues a recompute.
    ///
    /// Must be `T: Default` as we need to move out of the stored value, and
    /// `std::mem::take` lets us avoid cloning.
    #[inline]
    pub fn update(&self, f: impl FnOnce(T) -> T)
    where
        T: Default,
    {
        self.internal.update(f)
    }

    /// Like `RefCell::replace_with`. You get a mutable reference to the old
    /// value, your closure returns a new one, and you get back the old value
    /// (with any modifications you made to it during the closure).
    #[inline]
    pub fn replace_with(&self, f: impl FnOnce(&mut T) -> T) -> T {
        self.internal.replace_with(|mutable| f(mutable))
    }

    /// Like `RefCell::replace`.
    #[inline]
    pub fn replace(&self, value: T) -> T {
        self.internal.replace_with(|_| value)
    }

    /// Gives you a mutable reference to the variable, you do what you wish,
    /// and afterwards queues a recompute.
    #[inline]
    pub fn modify(&self, f: impl FnOnce(&mut T)) {
        self.internal.modify(f);
    }
    #[inline]
    pub fn get(&self) -> T {
        self.internal.get()
    }
    #[inline]
    pub fn watch(&self) -> Incr<T> {
        self.watch.clone()
    }
    #[inline]
    pub fn id(&self) -> NodeId {
        self.internal.node_id.get()
    }
}

impl<T: Value> Drop for Var<T> {
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

#[derive(Debug, Clone)]
pub struct IncrState {
    pub(crate) inner: Rc<State>,
}

impl Default for IncrState {
    fn default() -> Self {
        Self::new()
    }
}

impl IncrState {
    pub fn new() -> Self {
        let inner = State::new();
        Self { inner }
    }
    pub fn new_with_height(max_height: usize) -> Self {
        let inner = State::new_with_height(max_height);
        Self { inner }
    }

    pub fn weak(&self) -> WeakState {
        WeakState {
            inner: Rc::downgrade(&self.inner),
        }
    }

    pub fn add_weak_map<S: WeakMap + 'static>(&self, weak_map: Rc<RefCell<S>>) {
        self.inner.add_weak_map(weak_map)
    }

    pub fn weak_memoize_fn<I: Hash + Eq + Clone + 'static, T: Value>(
        &self,
        mut f: impl FnMut(I) -> Incr<T> + Clone,
    ) -> impl FnMut(I) -> Incr<T> + Clone {
        // // we define this inside the generic function.
        // // so it's a new type for every T!
        // slotmap::new_key_type! { struct TKey; }

        let storage: WeakHashMap<I, T> = WeakHashMap::default();
        let storage: Rc<RefCell<WeakHashMap<I, T>>> = Rc::new(RefCell::new(storage));
        self.add_weak_map(storage.clone());

        let weak_state = self.weak();

        // Function f is run in the scope you called weak_memoize_fn in.
        let creation_scope = self.inner.current_scope();

        move |i| {
            let storage_ = storage.borrow();
            if storage_.contains_key(&i) {
                let occ = storage_.get(&i).unwrap();
                let incr = occ.upgrade();
                if let Some(found_strong) = incr {
                    return found_strong;
                }
            }
            // don't want f to get a BorrowMutError if it's recursive
            drop(storage_);
            let val = weak_state
                .upgrade()
                .unwrap()
                .within_scope(creation_scope.clone(), || f(i.clone()));
            let mut storage_ = storage.borrow_mut();
            storage_.insert(i, val.weak());
            val
        }
    }

    /// Returns true if there is nothing to do. In particular, this allows you to
    /// find a fixed point in a computation that sets variables during stabilisation.
    pub fn is_stable(&self) -> bool {
        self.inner.is_stable()
    }

    pub fn stabilise(&self) {
        self.inner.stabilise();
    }

    pub fn stabilise_debug(&self, dot_file_prefix: &str) {
        self.inner.stabilise_debug(Some(dot_file_prefix));
    }

    #[inline]
    pub fn constant<T: Value>(&self, value: T) -> Incr<T> {
        self.inner.constant(value)
    }

    pub fn fold<F, T: Value, R: Value>(&self, vec: Vec<Incr<T>>, init: R, f: F) -> Incr<R>
    where
        F: FnMut(R, &T) -> R + 'static,
    {
        self.inner.fold(vec, init, f)
    }

    pub fn var<T: Value>(&self, value: T) -> Var<T> {
        self.inner.var_in_scope(value, scope::Scope::Top)
    }

    pub fn var_current_scope<T: Value>(&self, value: T) -> Var<T> {
        self.inner.var_in_scope(value, self.inner.current_scope())
    }

    pub fn unsubscribe(&self, token: SubscriptionToken) {
        self.inner.unsubscribe(token)
    }

    pub fn set_max_height_allowed(&self, new_max_height: usize) {
        self.inner.set_max_height_allowed(new_max_height)
    }

    pub fn within_scope<R>(&self, scope: Scope, f: impl FnOnce() -> R) -> R {
        self.inner.within_scope(scope.0, f)
    }

    pub fn save_dot_to_file(&self, named: &str) {
        self.inner.save_dot_to_file(named)
    }

    pub fn stats(&self) -> Stats {
        Stats {
            created: self.inner.num_nodes_created.get(),
            changed: self.inner.num_nodes_changed.get(),
            recomputed: self.inner.num_nodes_recomputed.get(),
            invalidated: self.inner.num_nodes_invalidated.get(),
            became_necessary: self.inner.num_nodes_became_necessary.get(),
            became_unnecessary: self.inner.num_nodes_became_unnecessary.get(),
            necessary: self.inner.num_nodes_became_necessary.get()
                - self.inner.num_nodes_became_unnecessary.get(),
        }
    }
}

/// Type to avoid Rc cyles on IncrState
#[derive(Debug, Clone)]
pub struct WeakState {
    pub(crate) inner: Weak<State>,
}
impl WeakState {
    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.inner.ptr_eq(&other.inner)
    }
    pub fn strong_count(&self) -> usize {
        self.inner.strong_count()
    }
    pub fn weak_count(&self) -> usize {
        self.inner.weak_count()
    }
    pub(crate) fn upgrade(&self) -> Option<Rc<State>> {
        self.inner.upgrade()
    }

    #[doc(hidden)]
    pub fn print_heap(&self) {
        let inner = self.upgrade().unwrap();
        println!("{:?}", inner.recompute_heap);
    }

    #[inline]
    pub fn constant<T: Value>(&self, value: T) -> Incr<T> {
        self.inner.upgrade().unwrap().constant(value)
    }

    pub fn fold<F, T: Value, R: Value>(&self, vec: Vec<Incr<T>>, init: R, f: F) -> Incr<R>
    where
        F: FnMut(R, &T) -> R + 'static,
    {
        self.inner.upgrade().unwrap().fold(vec, init, f)
    }

    pub fn var<T: Value>(&self, value: T) -> Var<T> {
        self.inner
            .upgrade()
            .unwrap()
            .var_in_scope(value, scope::Scope::Top)
    }

    pub fn var_current_scope<T: Value>(&self, value: T) -> Var<T> {
        let inner = self.inner.upgrade().unwrap();
        inner.var_in_scope(value, inner.current_scope())
    }

    pub fn within_scope<R>(&self, scope: Scope, f: impl FnOnce() -> R) -> R {
        self.inner.upgrade().unwrap().within_scope(scope.0, f)
    }

    pub fn save_dot_to_file(&self, named: &str) {
        self.inner.upgrade().unwrap().save_dot_to_file(named)
    }

    pub fn save_dot_to_string(&self) -> String {
        self.inner.upgrade().unwrap().save_dot_to_string()
    }

    pub fn unsubscribe(&self, token: SubscriptionToken) {
        self.inner.upgrade().unwrap().unsubscribe(token)
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

/// A helper trait for accepting either Incr or Var.
/// We already do Deref coercion from Var to Incr (i.e. its watch node),
/// so may as well accept Var anywhere we accept Incr.
pub trait IntoIncr<T> {
    fn into_incr(self) -> Incr<T>;
}

impl<T: Value> AsRef<Incr<T>> for Var<T> {
    #[inline]
    fn as_ref(&self) -> &Incr<T> {
        self.deref()
    }
}

impl<T: Value> AsRef<Incr<T>> for Incr<T> {
    #[inline]
    fn as_ref(&self) -> &Incr<T> {
        &self
    }
}

impl<T> IntoIncr<T> for Incr<T> {
    #[inline]
    fn into_incr(self) -> Incr<T> {
        self
    }
}

impl<T> IntoIncr<T> for &Incr<T> {
    #[inline]
    fn into_incr(self) -> Incr<T> {
        self.clone()
    }
}

impl<T: Value> IntoIncr<T> for Var<T> {
    #[inline]
    fn into_incr(self) -> Incr<T> {
        self.watch()
    }
}

/// And for var references. Because we don't need to consume self.
impl<T: Value> IntoIncr<T> for &Var<T> {
    #[inline]
    fn into_incr(self) -> Incr<T> {
        self.watch()
    }
}

// We don't need is_empty... this is only for stats, if that.
#[allow(clippy::len_without_is_empty)]
pub trait WeakMap {
    fn len(&self) -> usize;
    fn capacity(&self) -> usize;
    fn garbage_collect(&mut self);
}

pub type WeakHashMap<K, V> = HashMap<K, WeakIncr<V>>;

impl<K: Hash, V> WeakMap for WeakHashMap<K, V> {
    fn garbage_collect(&mut self) {
        self.retain(|_k, v| v.strong_count() != 0);
    }
    fn len(&self) -> usize {
        HashMap::len(self)
    }
    fn capacity(&self) -> usize {
        HashMap::capacity(self)
    }
}

#[cfg(feature = "slotmap")]
pub type WeakSlotMap<K, V> = slotmap::HopSlotMap<K, WeakIncr<V>>;

#[cfg(feature = "slotmap")]
impl<K: slotmap::Key, V> WeakMap for WeakSlotMap<K, V> {
    fn garbage_collect(&mut self) {
        self.retain(|_k, v| v.strong_count() != 0);
    }
    fn len(&self) -> usize {
        slotmap::HopSlotMap::len(self)
    }
    fn capacity(&self) -> usize {
        slotmap::HopSlotMap::capacity(self)
    }
}

#[derive(Clone)]
pub struct Scope(scope::Scope);

impl Debug for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Scope {
    pub const fn top() -> Self {
        Scope(scope::Scope::Top)
    }
}

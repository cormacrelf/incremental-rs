#![allow(unused_variables)]

mod adjust_heights_heap;
mod array_fold;
mod cutoff;
mod internal_observer;
mod kind;
mod node;
mod node_update;
mod recompute_heap;
mod scope;
mod stabilisation_num;
mod state;
mod symmetric_fold;
mod syntax;
mod unordered_fold;
mod var;

use crate::v_rch::kind::BindMainId;
use crate::v_rch::node::Incremental;
use crate::{GraphvizDot, IncrState};

use self::cutoff::Cutoff;
use self::kind::Kind;
use self::node::{ErasedNode, Node};
use self::scope::Scope;
use self::symmetric_fold::{DiffElement, GenericMap, SymmetricFoldMap, SymmetricMapMap};
use fmt::Debug;
use refl::refl;
use std::cell::RefCell;
use std::fmt;
use std::hash::Hash;
use std::rc::{Rc, Weak};

pub mod public;
use public::Observer;

use kind::NodeGenerics;
use node::Input;

/// Trait alias for `Debug + Clone `
pub trait Value: Debug + Clone + PartialEq + 'static {}
impl<T> Value for T where T: Debug + Clone + PartialEq + 'static {}
pub(crate) type NodeRef = Rc<dyn ErasedNode>;
pub(crate) type WeakNode = Weak<dyn ErasedNode>;

#[derive(Clone, Debug)]
#[must_use = "Incr<T> must be observed (.observe()) to be part of a computation."]
pub struct Incr<T> {
    node: Input<T>,
}

impl<T> From<Input<T>> for Incr<T> {
    fn from(node: Input<T>) -> Self {
        Self { node }
    }
}

impl<T> PartialEq for Incr<T> {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
    }
}

impl<T> Eq for Incr<T> { }

impl<T> Hash for Incr<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.node.id().hash(state)
    }
}

/// Type for use in weak hash maps of incrementals
#[derive(Debug, Clone)]
pub struct WeakIncr<T>(Weak<dyn Incremental<T>>);
impl<T> WeakIncr<T> {
    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }
    pub fn upgrade(&self) -> Option<Incr<T>> {
        self.0.upgrade().map(Incr::from)
    }
}

impl<T> Incr<T> {
    pub(crate) fn ptr_eq(&self, other: &Incr<T>) -> bool {
        Rc::ptr_eq(&self.node, &other.node)
    }
    pub fn weak(&self) -> WeakIncr<T> {
        WeakIncr(Rc::downgrade(&self.node))
    }
}

pub(crate) struct Map2Node<F, T1, T2, R>
where
    F: FnMut(&T1, &T2) -> R,
{
    one: Input<T1>,
    two: Input<T2>,
    mapper: RefCell<F>,
}

impl<F, T1, T2, R> NodeGenerics for Map2Node<F, T1, T2, R>
where
    T1: Value,
    T2: Value,
    R: Value,
    F: FnMut(&T1, &T2) -> R + 'static,
{
    type R = R;
    type BindLhs = ();
    type BindRhs = ();
    type I1 = T1;
    type I2 = T2;
    type F1 = fn(&Self::I1) -> R;
    type F2 = F;
    type B1 = fn(&Self::BindLhs) -> Incr<Self::BindRhs>;
    type Fold = fn(Self::R, &Self::I1) -> Self::R;
    type Update = fn(Self::R, &Self::I1, &Self::I1) -> Self::R;
    type WithOld = fn(Option<Self::R>, &Self::I1) -> (Self::R, bool);
}

impl<F, T1, T2, R> Debug for Map2Node<F, T1, T2, R>
where
    F: FnMut(&T1, &T2) -> R,
    R: Value,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Map2Node").finish()
    }
}

pub(crate) struct MapNode<F, T, R>
where
    F: FnMut(&T) -> R,
{
    input: Input<T>,
    mapper: RefCell<F>,
}

impl<F, T, R> NodeGenerics for MapNode<F, T, R>
where
    T: Value,
    R: Value,
    F: FnMut(&T) -> R + 'static,
{
    type R = R;
    type BindLhs = ();
    type BindRhs = ();
    type I1 = T;
    type I2 = ();
    type F1 = F;
    type F2 = fn(&Self::I1, &Self::I2) -> R;
    type B1 = fn(&Self::BindLhs) -> Incr<Self::BindRhs>;
    type Fold = fn(Self::R, &Self::I1) -> Self::R;
    type Update = fn(Self::R, &Self::I1, &Self::I1) -> Self::R;
    type WithOld = fn(Option<Self::R>, &Self::I1) -> (Self::R, bool);
}

impl<F, T, R> Debug for MapNode<F, T, R>
where
    F: FnMut(&T) -> R,
    R: Value,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MapNode").finish()
    }
}

/// Lets you dismantle the old R for parts.
pub(crate) struct MapWithOld<F, T, R>
where
    F: FnMut(Option<R>, &T) -> (R, bool),
{
    input: Input<T>,
    mapper: RefCell<F>,
    _p: std::marker::PhantomData<R>,
}

impl<F, T, R> NodeGenerics for MapWithOld<F, T, R>
where
    T: Value,
    R: Value,
    F: FnMut(Option<R>, &T) -> (R, bool) + 'static,
{
    type R = R;
    type BindLhs = ();
    type BindRhs = ();
    type I1 = T;
    type I2 = ();
    type F1 = fn(&Self::I1) -> R;
    type F2 = fn(&Self::I1, &Self::I2) -> R;
    type B1 = fn(&Self::BindLhs) -> Incr<Self::BindRhs>;
    type Fold = fn(Self::R, &Self::I1) -> Self::R;
    type Update = fn(Self::R, &Self::I1, &Self::I1) -> Self::R;
    type WithOld = F;
}

impl<F, T, R> Debug for MapWithOld<F, T, R>
where
    F: FnMut(Option<R>, &T) -> (R, bool),
    R: Value,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MapWithOld").finish()
    }
}

pub(crate) struct BindNode<F, T, R>
where
    R: Value,
    T: Value,
    F: FnMut(&T) -> Incr<R> + 'static,
{
    lhs_change: Rc<Node<BindLhsChangeNodeGenerics<F, T, R>>>,
    main: Weak<Node<BindNodeMainGenerics<F, T, R>>>,
    lhs: Input<T>,
    mapper: RefCell<F>,
    rhs: RefCell<Option<Incr<R>>>,
    rhs_scope: RefCell<Scope>,
    all_nodes_created_on_rhs: RefCell<Vec<WeakNode>>,
}

pub(crate) trait BindScope: Debug {
    fn is_valid(&self) -> bool;
    fn is_necessary(&self) -> bool;
    fn height(&self) -> i32;
    fn add_node(&self, node: WeakNode);
}

impl<F, T, R> BindScope for BindNode<F, T, R>
where
    R: Value,
    T: Value,
    F: FnMut(&T) -> Incr<R>,
{
    fn is_valid(&self) -> bool {
        let Some(main) = self.main.upgrade() else { return false };
        main.is_valid()
    }
    fn is_necessary(&self) -> bool {
        let Some(main) = self.main.upgrade() else { return false };
        main.is_necessary()
    }
    fn height(&self) -> i32 {
        self.lhs_change.height.get()
    }
    fn add_node(&self, node: WeakNode) {
        tracing::info!("added node to scope {self:?}: {:?}", node.upgrade());
        let mut all = self.all_nodes_created_on_rhs.borrow_mut();
        all.push(node);
    }
}

struct BindLhsChangeNodeGenerics<F, T, R> {
    _phantom: std::marker::PhantomData<(F, T, R)>,
}

impl<F, T, R> NodeGenerics for BindLhsChangeNodeGenerics<F, T, R>
where
    F: FnMut(&T) -> Incr<R> + 'static,
    T: Value,
    R: Value,
{
    // lhs change's Node stores () nothing. just a sentinel.
    type R = ();
    type BindLhs = T;
    type BindRhs = R;
    // swap order so we can use as_parent with I1
    type I1 = ();
    type I2 = T;
    type F1 = fn(&Self::I1) -> Self::R;
    type F2 = fn(&Self::I1, &Self::I2) -> Self::R;
    type B1 = F;
    type Fold = fn(Self::R, &Self::I1) -> Self::R;
    type Update = fn(Self::R, &Self::I1, &Self::I1) -> Self::R;
    type WithOld = fn(Option<Self::R>, &Self::I1) -> (Self::R, bool);
}

struct BindNodeMainGenerics<F, T, R> {
    _phantom: std::marker::PhantomData<(F, T, R)>,
}

impl<F, T, R> NodeGenerics for BindNodeMainGenerics<F, T, R>
where
    F: FnMut(&T) -> Incr<R> + 'static,
    T: Value,
    R: Value,
{
    // We copy the output of the Rhs
    type R = R;
    type BindLhs = T;
    type BindRhs = R;
    /// Rhs
    type I1 = R;
    /// BindLhsChange (a sentinel)
    type I2 = ();
    type F1 = fn(&Self::I1) -> Self::R;
    type F2 = fn(&Self::I1, &Self::I2) -> Self::R;
    type B1 = F;
    type Fold = fn(Self::R, &Self::I1) -> Self::R;
    type Update = fn(Self::R, &Self::I1, &Self::I1) -> Self::R;
    type WithOld = fn(Option<Self::R>, &Self::I1) -> (Self::R, bool);
}

impl<F, T, R> Debug for BindNode<F, T, R>
where
    F: FnMut(&T) -> Incr<R>,
    R: Value,
    T: Value,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BindNode")
            .field("output", &self.rhs.borrow().as_ref().map(|x| &x.node))
            .finish()
    }
}

impl<T: Value> Incr<T> {

    /// A convenience function for taking a function of type `fn(Incr<T>) -> Incr<R>` and
    /// applying it to self. This enables you to put your own functions
    /// into the middle of a chain of method calls on Incr.
    #[inline]
    pub fn pipe<R>(&self, mut f: impl FnMut(Incr<T>) -> Incr<R>) -> Incr<R> {
        // clones are cheap.
        f(self.clone())
    }

    pub fn pipe1<R, A1>(&self, mut f: impl FnMut(Incr<T>, A1) -> Incr<R>, arg1: A1) -> Incr<R> {
        f(self.clone(), arg1)
    }

    pub fn pipe2<R, A1, A2>(
        &self,
        mut f: impl FnMut(Incr<T>, A1, A2) -> Incr<R>,
        arg1: A1,
        arg2: A2,
    ) -> Incr<R> {
        f(self.clone(), arg1, arg2)
    }

    /// A simple variation on `Incr::map` that tells you how many
    /// times the incremental has recomputed before this time.
    pub fn enumerate<R, F>(&self, mut f: F) -> Incr<R>
    where
        R: Value,
        F: FnMut(usize, &T) -> R + 'static,
    {
        let mut counter = 0;
        self.map(move |x| {
            let v = f(counter, x);
            counter += 1;
            v
        })
    }

    fn with_old_input_output<R, F>(&self, mut f: F) -> Incr<R>
    where
        R: Value,
        F: FnMut(Option<(T, R)>, &T) -> (R, bool) + 'static,
    {
        let old_input: RefCell<Option<T>> = RefCell::new(None);
        self.map_with_old(move |old_opt, a| {
            let mut oi = old_input.borrow_mut();
            let (b, didchange) = f(old_opt.and_then(|x| oi.take().map(|oi| (oi, x))), a);
            *oi = Some(a.clone());
            (b, didchange)
        })
    }

    pub fn map<R: Value, F: FnMut(&T) -> R + 'static>(&self, f: F) -> Incr<R> {
        let mapper = MapNode {
            input: self.clone().node,
            mapper: f.into(),
        };
        let state = self.node.state();
        let node = Node::<MapNode<F, T, R>>::create(
            state.weak(),
            state.current_scope.borrow().clone(),
            Kind::Map(mapper),
        );
        let map = Incr { node };
        map
    }

    /// A version of `Incr::map` that allows reuse of the old
    /// value. You can use it to produce a new value. The main
    /// use case is avoiding allocation.
    ///
    /// The return type of the closure is `(R, bool)`. The boolean
    /// value is a replacement for the `Cutoff` system, because
    /// the `Cutoff` functions require access to an old value and
    /// a new value. With `map_with_old`, you must figure out yourself
    /// (without relying on PartialEq, for example) whether the
    /// incremental node should propagate its changes.
    ///
    pub fn map_with_old<R, F>(&self, f: F) -> Incr<R>
    where
        R: Value,
        F: FnMut(Option<R>, &T) -> (R, bool) + 'static,
    {
        let state = self.node.state();
        let node = Node::<MapWithOld<F, T, R>>::create(
            state.weak(),
            state.current_scope(),
            Kind::MapWithOld(MapWithOld {
                input: self.clone().node,
                mapper: f.into(),
                _p: Default::default(),
            }),
        );
        Incr { node }
    }

    pub fn map2<F, T2, R>(&self, other: &Incr<T2>, f: F) -> Incr<R>
    where
        T2: Value,
        R: Value,
        F: FnMut(&T, &T2) -> R + 'static,
    {
        let mapper = Map2Node {
            one: self.clone().node,
            two: other.clone().node,
            mapper: f.into(),
        };
        let state = self.node.state();
        let node = Node::<Map2Node<F, T, T2, R>>::create(
            state.weak(),
            state.current_scope.borrow().clone(),
            Kind::Map2(mapper),
        );
        let map = Incr { node };
        map
    }

    /// A version of bind that includes a copy of the `incremental::State`
    /// to help you construct new incrementals within the bind.
    pub fn binds<F, R>(&self, mut f: F) -> Incr<R>
    where
        R: Value,
        F: FnMut(&IncrState, &T) -> Incr<R> + 'static,
    {
        let cloned = self.node.state().public();
        self.bind(move |value: &T| f(&cloned, value))
    }

    pub fn bind<F, R>(&self, f: F) -> Incr<R>
    where
        R: Value,
        F: FnMut(&T) -> Incr<R> + 'static,
    {
        let state = self.node.state();
        let lhs_change = Node::<BindLhsChangeNodeGenerics<F, T, R>>::create(
            state.weak(),
            state.current_scope(),
            Kind::Uninitialised,
        );
        tracing::debug!(
            "creating bind with scope height {:?}",
            state.current_scope().height()
        );
        let main = Node::<BindNodeMainGenerics<F, T, R>>::create(
            state.weak(),
            state.current_scope(),
            Kind::Uninitialised,
        );
        let bind = Rc::new(BindNode {
            lhs: self.clone().node,
            mapper: f.into(),
            rhs: RefCell::new(None),
            rhs_scope: RefCell::new(Scope::Top),
            all_nodes_created_on_rhs: RefCell::new(vec![]),
            lhs_change,
            main: Rc::downgrade(&main),
        });
        {
            let mut rhs_scope = bind.rhs_scope.borrow_mut();
            *rhs_scope = Scope::Bind(Rc::downgrade(&bind) as Weak<dyn BindScope>);
        }

        let main_incr = Incr { node: main.clone() };
        {
            let mut main_kind = main.kind.borrow_mut();
            *main_kind = Kind::BindMain(
                BindMainId {
                    input_lhs_i2: refl(),
                    input_rhs_i1: refl(),
                    rhs_r: refl(),
                },
                bind.clone(),
            );
        }
        {
            let mut lhs_change_kind = bind.lhs_change.kind.borrow_mut();
            *lhs_change_kind = Kind::BindLhsChange(
                kind::BindLhsId {
                    r_unit: refl(),
                    input_lhs_i2: refl(),
                },
                Rc::downgrade(&bind),
            );
        }
        /* We set [lhs_change] to never cutoff so that whenever [lhs] changes, [main] is
        recomputed.  This is necessary to handle cases where [f] returns an existing stable
        node, in which case the [lhs_change] would be the only thing causing [main] to be
        stale. */
        bind.lhs_change.set_cutoff(Cutoff::Never);
        main_incr
    }

    /// Creates an observer for this incremental.
    ///
    /// Observers are the primary way to get data out of the computation.
    /// Their creation and lifetime inform Incremental which parts of the
    /// computation graph are necessary, such that if you create many
    /// variables and computations based on them, but only hook up some of
    /// that to an observer, only the parts transitively necessary to
    /// supply the observer with values are queued to be recomputed.
    ///
    /// That means, without an observer, `var.set(new_value)` does essentially
    /// nothing, even if you have created incrementals like
    /// `var.map(...).bind(...).map(...)`. In this fashion, you can safely
    /// set up computation graphs before you need them, or refuse to dismantle
    /// them, knowing the expensive computations they contain will not
    /// grace the CPU until they're explicitly put back under the purview
    /// of an Observer.
    ///
    /// Calling this multiple times on the same node produces multiple
    /// observers. Only one is necessary to keep a part of a computation
    /// graph alive and ticking.
    pub fn observe(&self) -> Observer<T> {
        let incr = self.clone();
        let internal = incr.node.state().observe(incr);
        Observer::new(internal)
    }

    /// Sets the cutoff function that determines (if it returns true)
    /// whether to stop (cut off) propagating changes through the graph.
    /// Note that this method can be called on `Var` as well as any
    /// other `Incr`.
    ///
    /// The default is `Cutoff::PartialEq`. So if your values do not change,
    /// they will cut off propagation. There is a bound on all T in
    /// `Incr<T>` used in incremental-rs, all values you pass around
    /// must be PartialEq.
    ///
    /// You can also supply your own comparison function. This will chiefly
    /// be useful for types like Rc<T>, not to avoid T: PartialEq (you
    /// can't avoid that) but rather to avoid comparing a large structure
    /// and simply compare the allocation's pointer value instead.
    /// In that case, you can:
    ///
    /// ```
    /// use std::rc::Rc;
    /// use incremental::{IncrState, Cutoff};
    /// let incr = IncrState::new();
    /// let var = incr.var(Rc::new(5));
    /// var.set_cutoff(Cutoff::Custom(Rc::ptr_eq));
    /// // but note that doing this will now cause the change below
    /// // to propagate, whereas before it would not as the two
    /// // numbers are == equal:
    /// var.set(Rc::new(5));
    /// ```
    ///
    pub fn set_cutoff(&self, cutoff: Cutoff<T>) {
        self.node.set_cutoff(cutoff);
    }

    pub fn save_dot_to_file(&self, named: &str) {
        GraphvizDot::new(self).save_to_file(named).unwrap();
    }
}

impl<T: Value> Incr<T> {
    #[inline]
    pub fn incr_map<F, K, V, V2>(&self, mut f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&V) -> V2 + 'static,
        T::OutputMap<V2>: Value,
    {
        self.incr_filter_mapi(move |_k, v| Some(f(v)))
    }

    #[inline]
    pub fn incr_filter_map<F, K, V, V2>(&self, mut f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&V) -> Option<V2> + 'static,
        T::OutputMap<V2>: Value,
    {
        self.incr_filter_mapi(move |_k, v| f(v))
    }

    #[inline]
    pub fn incr_mapi<F, K, V, V2>(&self, mut f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&K, &V) -> V2 + 'static,
        T::OutputMap<V2>: Value,
    {
        self.incr_filter_mapi(move |k, v| Some(f(k, v)))
    }

    pub fn incr_filter_mapi<F, K, V, V2>(&self, mut f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&K, &V) -> Option<V2> + 'static,
        T::OutputMap<V2>: Value,
    {
        self.with_old_input_output(move |old, input| match (old, input.len()) {
            (o @ _, 0) | (o @ None, _) => (input.filter_map_collect(&mut f), true),
            (Some((old_in, mut old_out)), _) => {
                let mut did_change = false;
                let old_out_mut = old_out.make_mut();
                let _: &mut <T::OutputMap<V2> as SymmetricMapMap<K, V2>>::UnderlyingMap = old_in
                    .symmetric_fold(input, old_out_mut, |out, change| {
                        did_change = true;
                        match change {
                            DiffElement::Left((key, _)) => {
                                out.remove(&key);
                            }
                            DiffElement::Right((key, newval)) => {
                                if let Some(v2) = f(key, newval) {
                                    out.insert(key.clone(), v2);
                                } else {
                                    out.remove(key);
                                }
                            }
                            DiffElement::Unequal((key, _), (_, newval)) => {
                                if let Some(v2) = f(key, newval) {
                                    out.insert(key.clone(), v2);
                                } else {
                                    out.remove(key);
                                }
                            }
                        }
                        out
                    });
                (old_out, did_change)
            }
        })
    }

    pub fn incr_unordered_fold<FAdd, FRemove, K, V, R>(
        &self,
        init: R,
        mut add: FAdd,
        mut remove: FRemove,
        revert_to_init_when_empty: bool,
    ) -> Incr<R>
    where
        T: SymmetricFoldMap<K, V>,
        K: Value + Ord,
        V: Value,
        R: Value,
        FAdd: FnMut(R, &K, &V) -> R + 'static,
        FRemove: FnMut(R, &K, &V) -> R + 'static,
    {
        self.with_old_input_output(move |old, new_in| match old {
            None => {
                let newmap = new_in.nonincremental_fold(init.clone(), |acc, (k, v)| add(acc, k, v));
                (newmap, true)
            }
            Some((old_in, old_out)) => {
                if revert_to_init_when_empty && new_in.is_empty() {
                    return (init.clone(), !old_in.is_empty());
                }
                let mut did_change = false;
                let folded: R = old_in.symmetric_fold(new_in, old_out, |mut acc, difference| {
                    did_change = true;
                    match difference {
                        DiffElement::Left((key, value)) => remove(acc, key, value),
                        DiffElement::Right((key, value)) => add(acc, key, value),
                        DiffElement::Unequal((lk, lv), (rk, rv)) => {
                            acc = remove(acc, lk, lv);
                            add(acc, rk, rv)
                        }
                    }
                });
                (folded, did_change)
            }
        })
    }
}

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
use crate::IncrState;

use self::cutoff::Cutoff;
use self::kind::Kind;
use self::node::{ErasedNode, Node};
use self::scope::Scope;
use self::symmetric_fold::{DiffElement, GenericMap, SymmetricFoldMap, SymmetricMapMap};
use fmt::Debug;
use refl::refl;
use std::cell::RefCell;
use std::fmt;
use std::rc::{Rc, Weak};

pub mod public;
use public::Observer;

use kind::NodeGenerics;
use node::Input;

/// Trait alias for `Debug + Clone + 'a`
pub trait Value<'a>: Debug + Clone + PartialEq + 'a {}
impl<'a, T> Value<'a> for T where T: Debug + Clone + PartialEq + 'a {}
pub(crate) type NodeRef<'a> = Rc<dyn ErasedNode<'a> + 'a>;
pub(crate) type WeakNode<'a> = Weak<dyn ErasedNode<'a> + 'a>;

#[derive(Clone, Debug)]
#[must_use = "Incr<T> must be observed (.observe()) to be part of a computation."]
pub struct Incr<'a, T> {
    node: Input<'a, T>,
}

pub(crate) struct Map2Node<'a, F, T1, T2, R>
where
    F: FnMut(&T1, &T2) -> R + 'a,
{
    one: Input<'a, T1>,
    two: Input<'a, T2>,
    mapper: RefCell<F>,
}

impl<'a, F, T1, T2, R> NodeGenerics<'a> for Map2Node<'a, F, T1, T2, R>
where
    T1: Value<'a>,
    T2: Value<'a>,
    R: Value<'a>,
    F: FnMut(&T1, &T2) -> R + 'a,
{
    type R = R;
    type BindLhs = ();
    type BindRhs = ();
    type I1 = T1;
    type I2 = T2;
    type F1 = fn(&Self::I1) -> R;
    type F2 = F;
    type B1 = fn(&Self::BindLhs) -> Incr<'a, Self::BindRhs>;
    type Fold = fn(Self::R, &Self::I1) -> Self::R;
    type Update = fn(Self::R, &Self::I1, &Self::I1) -> Self::R;
    type WithOld = fn(Option<Self::R>, &Self::I1) -> (Self::R, bool);
}

impl<'a, F, T1, T2, R> Debug for Map2Node<'a, F, T1, T2, R>
where
    F: FnMut(&T1, &T2) -> R + 'a,
    R: Value<'a>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Map2Node").finish()
    }
}

pub(crate) struct MapNode<'a, F, T, R>
where
    F: FnMut(&T) -> R + 'a,
{
    input: Input<'a, T>,
    mapper: RefCell<F>,
}

impl<'a, F, T, R> NodeGenerics<'a> for MapNode<'a, F, T, R>
where
    T: Value<'a>,
    R: Value<'a>,
    F: FnMut(&T) -> R + 'a,
{
    type R = R;
    type BindLhs = ();
    type BindRhs = ();
    type I1 = T;
    type I2 = ();
    type F1 = F;
    type F2 = fn(&Self::I1, &Self::I2) -> R;
    type B1 = fn(&Self::BindLhs) -> Incr<'a, Self::BindRhs>;
    type Fold = fn(Self::R, &Self::I1) -> Self::R;
    type Update = fn(Self::R, &Self::I1, &Self::I1) -> Self::R;
    type WithOld = fn(Option<Self::R>, &Self::I1) -> (Self::R, bool);
}

impl<'a, F, T, R> Debug for MapNode<'a, F, T, R>
where
    F: FnMut(&T) -> R + 'a,
    R: Value<'a>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MapNode").finish()
    }
}

/// Lets you dismantle the old R for parts.
pub(crate) struct MapWithOld<'a, F, T, R>
where
    F: FnMut(Option<R>, &T) -> (R, bool) + 'a,
{
    input: Input<'a, T>,
    mapper: RefCell<F>,
    _p: std::marker::PhantomData<R>,
}

impl<'a, F, T, R> NodeGenerics<'a> for MapWithOld<'a, F, T, R>
where
    T: Value<'a>,
    R: Value<'a>,
    F: FnMut(Option<R>, &T) -> (R, bool) + 'a,
{
    type R = R;
    type BindLhs = ();
    type BindRhs = ();
    type I1 = T;
    type I2 = ();
    type F1 = fn(&Self::I1) -> R;
    type F2 = fn(&Self::I1, &Self::I2) -> R;
    type B1 = fn(&Self::BindLhs) -> Incr<'a, Self::BindRhs>;
    type Fold = fn(Self::R, &Self::I1) -> Self::R;
    type Update = fn(Self::R, &Self::I1, &Self::I1) -> Self::R;
    type WithOld = F;
}

impl<'a, F, T, R> Debug for MapWithOld<'a, F, T, R>
where
    F: FnMut(Option<R>, &T) -> (R, bool) + 'a,
    R: Value<'a>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MapWithOld").finish()
    }
}

pub(crate) struct BindNode<'a, F, T, R>
where
    R: Value<'a>,
    T: Value<'a>,
    F: FnMut(&T) -> Incr<'a, R> + 'a,
{
    lhs_change: Rc<Node<'a, BindLhsChangeNodeGenerics<F, T, R>>>,
    main: Weak<Node<'a, BindNodeMainGenerics<F, T, R>>>,
    lhs: Input<'a, T>,
    mapper: RefCell<F>,
    rhs: RefCell<Option<Incr<'a, R>>>,
    rhs_scope: RefCell<Scope<'a>>,
    // to_disconnect: RefCell<Option<RefCell<Incr<'a, R>>>>,
    all_nodes_created_on_rhs: RefCell<Vec<WeakNode<'a>>>,
}

pub(crate) trait BindScope<'a>: Debug + 'a {
    fn is_valid(&self) -> bool;
    fn is_necessary(&self) -> bool;
    fn height(&self) -> i32;
    fn add_node(&self, node: WeakNode<'a>);
}

impl<'a, F, T, R> BindScope<'a> for BindNode<'a, F, T, R>
where
    R: Value<'a>,
    T: Value<'a>,
    F: FnMut(&T) -> Incr<'a, R> + 'a,
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
    fn add_node(&self, node: WeakNode<'a>) {
        let mut all = self.all_nodes_created_on_rhs.borrow_mut();
        all.push(node);
    }
}

struct BindLhsChangeNodeGenerics<F, T, R> {
    _phantom: std::marker::PhantomData<(F, T, R)>,
}

impl<'a, F, T, R> NodeGenerics<'a> for BindLhsChangeNodeGenerics<F, T, R>
where
    F: FnMut(&T) -> Incr<'a, R> + 'a,
    T: Value<'a>,
    R: Value<'a>,
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

impl<'a, F, T, R> NodeGenerics<'a> for BindNodeMainGenerics<F, T, R>
where
    F: FnMut(&T) -> Incr<'a, R> + 'a,
    T: Value<'a>,
    R: Value<'a>,
{
    // We copy the output of the Rhs
    type R = R;
    type BindLhs = T;
    type BindRhs = R;
    /// BindLhsChange (a sentinel)
    type I1 = R;
    /// Rhs
    type I2 = ();
    type F1 = fn(&Self::I1) -> Self::R;
    type F2 = fn(&Self::I1, &Self::I2) -> Self::R;
    type B1 = F;
    type Fold = fn(Self::R, &Self::I1) -> Self::R;
    type Update = fn(Self::R, &Self::I1, &Self::I1) -> Self::R;
    type WithOld = fn(Option<Self::R>, &Self::I1) -> (Self::R, bool);
}

impl<'a, F, T, R> Debug for BindNode<'a, F, T, R>
where
    F: FnMut(&T) -> Incr<'a, R> + 'a,
    R: Value<'a>,
    T: Value<'a>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BindNode")
            .field("output", &self.rhs.borrow().as_ref().map(|x| &x.node))
            .finish()
    }
}

impl<'a, T: Value<'a>> Incr<'a, T> {
    pub(crate) fn ptr_eq(&self, other: &Incr<'a, T>) -> bool {
        Rc::ptr_eq(&self.node, &other.node)
    }

    /// A convenience function for taking a function of type `fn(Incr<T>) -> Incr<R>` and
    /// applying it to self. This enables you to put your own functions
    /// into the middle of a chain of method calls on Incr.
    #[inline]
    pub fn pipe<R>(&self, mut f: impl FnMut(Incr<'a, T>) -> Incr<'a, R>) -> Incr<'a, R> {
        // clones are cheap.
        f(self.clone())
    }

    pub fn pipe1<R, A1>(
        &self,
        mut f: impl FnMut(Incr<'a, T>, A1) -> Incr<'a, R>,
        arg1: A1,
    ) -> Incr<'a, R> {
        f(self.clone(), arg1)
    }

    pub fn pipe2<R, A1, A2>(
        &self,
        mut f: impl FnMut(Incr<'a, T>, A1, A2) -> Incr<'a, R>,
        arg1: A1,
        arg2: A2,
    ) -> Incr<'a, R> {
        f(self.clone(), arg1, arg2)
    }

    /// A simple variation on `Incr::map` that tells you how many
    /// times the incremental has recomputed before this time.
    pub fn enumerate<R, F>(&self, mut f: F) -> Incr<'a, R>
    where
        R: Value<'a>,
        F: FnMut(usize, &T) -> R + 'a,
    {
        let mut counter = 0;
        self.map(move |x| {
            let v = f(counter, x);
            counter += 1;
            v
        })
    }

    fn with_old_input_output<R, F>(&self, mut f: F) -> Incr<'a, R>
    where
        R: Value<'a>,
        F: FnMut(Option<(T, R)>, &T) -> (R, bool) + 'a,
    {
        let old_input: RefCell<Option<T>> = RefCell::new(None);
        self.map_with_old(move |old_opt, a| {
            let mut oi = old_input.borrow_mut();
            let (b, didchange) = f(old_opt.and_then(|x| oi.take().map(|oi| (oi, x))), a);
            *oi = Some(a.clone());
            (b, didchange)
        })
    }

    pub fn map<R: Value<'a>, F: FnMut(&T) -> R + 'a>(&self, f: F) -> Incr<'a, R> {
        let mapper = MapNode {
            input: self.clone().node,
            mapper: f.into(),
        };
        let state = self.node.state();
        let node = Node::<MapNode<'a, F, T, R>>::create(
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
    pub fn map_with_old<R, F>(&self, f: F) -> Incr<'a, R>
    where
        R: Value<'a>,
        F: FnMut(Option<R>, &T) -> (R, bool) + 'a,
    {
        let state = self.node.state();
        let node = Node::<MapWithOld<'a, F, T, R>>::create(
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

    pub fn map2<F, T2, R>(&self, other: &Incr<'a, T2>, f: F) -> Incr<'a, R>
    where
        T2: Value<'a>,
        R: Value<'a>,
        F: FnMut(&T, &T2) -> R + 'a,
    {
        let mapper = Map2Node {
            one: self.clone().node,
            two: other.clone().node,
            mapper: f.into(),
        };
        let state = self.node.state();
        let node = Node::<Map2Node<'a, F, T, T2, R>>::create(
            state.weak(),
            state.current_scope.borrow().clone(),
            Kind::Map2(mapper),
        );
        let map = Incr { node };
        map
    }

    /// A version of bind that includes a copy of the `incremental::State`
    /// to help you construct new incrementals within the bind.
    pub fn binds<F, R>(&self, mut f: F) -> Incr<'a, R>
    where
        R: Value<'a>,
        F: FnMut(&IncrState<'a>, &T) -> Incr<'a, R> + 'a,
    {
        let cloned = self.node.state().public();
        self.bind(move |value: &T| f(&cloned, value))
    }

    pub fn bind<F, R>(&self, f: F) -> Incr<'a, R>
    where
        R: Value<'a>,
        F: FnMut(&T) -> Incr<'a, R> + 'a,
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
        let bind_scope = Scope::Bind(Rc::downgrade(&bind) as Weak<dyn BindScope<'a>>);
        let mut rhs_scope = bind.rhs_scope.borrow_mut();
        *rhs_scope = bind_scope;

        let main_incr = Incr { node: main.clone() };
        let mut main_kind = main.kind.borrow_mut();
        *main_kind = Kind::BindMain(
            BindMainId {
                input_lhs_i2: refl(),
                input_rhs_i1: refl(),
                rhs_r: refl(),
            },
            bind.clone(),
        );
        let mut lhs_change_kind = bind.lhs_change.kind.borrow_mut();
        *lhs_change_kind = Kind::BindLhsChange(
            kind::BindLhsId {
                r_unit: refl(),
                input_lhs_i2: refl(),
            },
            Rc::downgrade(&bind),
        );
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
    pub fn observe(&self) -> Observer<'a, T> {
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
}

impl<'a, T: Value<'a>> Incr<'a, T> {
    #[inline]
    pub fn incr_map<F, K, V, V2>(&self, mut f: F) -> Incr<'a, T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value<'a> + Ord,
        V: Value<'a> + Eq,
        V2: Value<'a> + Eq,
        F: FnMut(&V) -> V2 + 'a,
        T::OutputMap<V2>: Value<'a>,
    {
        self.incr_filter_mapi(move |_k, v| Some(f(v)))
    }

    #[inline]
    pub fn incr_filter_map<F, K, V, V2>(&self, mut f: F) -> Incr<'a, T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value<'a> + Ord,
        V: Value<'a> + Eq,
        V2: Value<'a> + Eq,
        F: FnMut(&V) -> Option<V2> + 'a,
        T::OutputMap<V2>: Value<'a>,
    {
        self.incr_filter_mapi(move |_k, v| f(v))
    }

    #[inline]
    pub fn incr_mapi<F, K, V, V2>(&self, mut f: F) -> Incr<'a, T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value<'a> + Ord,
        V: Value<'a> + Eq,
        V2: Value<'a> + Eq,
        F: FnMut(&K, &V) -> V2 + 'a,
        T::OutputMap<V2>: Value<'a>,
    {
        self.incr_filter_mapi(move |k, v| Some(f(k, v)))
    }

    pub fn incr_filter_mapi<F, K, V, V2>(&self, mut f: F) -> Incr<'a, T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value<'a> + Ord,
        V: Value<'a> + Eq,
        V2: Value<'a> + Eq,
        F: FnMut(&K, &V) -> Option<V2> + 'a,
        T::OutputMap<V2>: Value<'a>,
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
    ) -> Incr<'a, R>
    where
        T: SymmetricFoldMap<K, V>,
        K: Value<'a> + Ord,
        V: Value<'a> + Eq,
        R: Value<'a>,
        FAdd: FnMut(R, &K, &V) -> R + 'a,
        FRemove: FnMut(R, &K, &V) -> R + 'a,
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

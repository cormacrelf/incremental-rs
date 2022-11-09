#![allow(unused_variables)]

mod adjust_heights_heap;
mod array_fold;
mod cutoff;
mod expert;

mod incr_map;
mod internal_observer;
mod kind;
mod node;
mod node_update;
mod recompute_heap;
mod scope;
mod stabilisation_num;
mod state;
mod syntax;
mod var;

use crate::v_rch::kind::{BindMainId, BindLhsId};
use crate::v_rch::node::Incremental;
use crate::{GraphvizDot, WeakState};

use self::cutoff::Cutoff;
use self::kind::Kind;
use self::node::{ErasedNode, Node};
use self::scope::Scope;
use self::incr_map::symmetric_fold::{DiffElement, GenericMap, SymmetricFoldMap, SymmetricMapMap};
use fmt::Debug;
use refl::refl;
use std::cell::{RefCell, Cell};
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

/// Little helper trait for bumping a statistic.
pub(crate) trait CellIncrement {
    type Num;
    fn increment(&self);
    fn decrement(&self);
    // std is going to add Cell:update... someday...
    fn update_val(&self, f: impl FnOnce(Self::Num) -> Self::Num);
}

macro_rules! impl_cell_increment {
    ($num_ty:ty) => {
        impl CellIncrement for Cell<$num_ty> {
            type Num = $num_ty;
            #[inline]
            fn update_val(&self, f: impl FnOnce(Self::Num) -> Self::Num) {
                self.set(f(self.get()));
            }
            #[inline(always)]
            fn increment(&self) {
                self.update_val(|x| x + 1)
            }
            #[inline(always)]
            fn decrement(&self) {
                self.update_val(|x| x - 1)
            }
        }
    }
}
impl_cell_increment!(i32);
impl_cell_increment!(usize);

#[derive(Debug)]
#[must_use = "Incr<T> must be observed (.observe()) to be part of a computation."]
pub struct Incr<T> {
    node: Input<T>,
}

impl<T> Clone for Incr<T> {
    fn clone(&self) -> Self {
        Self { node: self.node.clone() }
    }
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
#[derive(Debug)]
pub struct WeakIncr<T>(pub(crate) Weak<dyn Incremental<T>>);
impl<T> WeakIncr<T> {
    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }
    pub fn upgrade(&self) -> Option<Incr<T>> {
        self.0.upgrade().map(Incr::from)
    }
    pub fn strong_count(&self) -> usize {
        self.0.strong_count()
    }
    pub fn weak_count(&self) -> usize {
        self.0.weak_count()
    }
}

impl<T> Clone for WeakIncr<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Incr<T> {
    pub(crate) fn ptr_eq(&self, other: &Incr<T>) -> bool {
        Rc::ptr_eq(&self.node, &other.node)
    }
    pub fn weak(&self) -> WeakIncr<T> {
        WeakIncr(Rc::downgrade(&self.node))
    }
    pub fn set_graphviz_user_data(&self, data: impl Debug + 'static) {
        self.node.set_graphviz_user_data(Box::new(data))
    }
    pub fn with_graphviz_user_data(self, data: impl Debug + 'static) -> Self {
        self.node.set_graphviz_user_data(Box::new(data));
        self
    }
    pub fn state(&self) -> WeakState {
        self.node.state().public_weak()
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
    type FRef = fn(&Self::I1) -> &Self::R;
    type Recompute = fn() -> Self::R;
    type ObsChange = fn(bool);
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
    type FRef = fn(&Self::I1) -> &Self::R;
    type Recompute = fn() -> Self::R;
    type ObsChange = fn(bool);
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
    // WARN: we ignore this boolean now
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
    type FRef = fn(&Self::I1) -> &Self::R;
    type Recompute = fn() -> Self::R;
    type ObsChange = fn(bool);
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
    lhs_change: RefCell<Weak<Node<BindLhsChangeNodeGenerics<F, T, R>>>>,
    main: RefCell<Weak<Node<BindNodeMainGenerics<F, T, R>>>>,
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
        let main_ = self.main.borrow();
        let Some(main) = main_.upgrade() else { return false };
        main.is_valid()
    }
    fn is_necessary(&self) -> bool {
        let main_ = self.main.borrow();
        let Some(main) = main_.upgrade() else { return false };
        main.is_necessary()
    }
    fn height(&self) -> i32 {
        let lhs_change_ = self.lhs_change.borrow();
        let lhs_change = lhs_change_.upgrade().unwrap();
        lhs_change.height.get()
    }
    fn add_node(&self, node: WeakNode) {
        tracing::info!("added node to scope {self:?}: {:?}", node.upgrade());
        let mut all = self.all_nodes_created_on_rhs.borrow_mut();
        all.push(node);
    }
}

pub(crate) struct BindLhsChangeNodeGenerics<F, T, R> {
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
    type FRef = fn(&Self::I1) -> &Self::R;
    type Recompute = fn() -> Self::R;
    type ObsChange = fn(bool);
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
    type FRef = fn(&Self::I1) -> &Self::R;
    type Recompute = fn() -> Self::R;
    type ObsChange = fn(bool);
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

    fn with_old_input_output2<R, T2, F>(&self, other: &Incr<T2>, mut f: F) -> Incr<R>
    where
        R: Value,
        T2: Value,
        F: FnMut(Option<(T, T2, R)>, &T, &T2) -> (R, bool) + 'static,
    {
        let old_input: RefCell<Option<(T, T2)>> = RefCell::new(None);
        // TODO: too much cloning
        self.map2(other, |a, b| (a.clone(), b.clone()))
            .map_with_old(move |old_opt, (a, b)| {
                let mut oi = old_input.borrow_mut();
                let (r, didchange) = f(
                    old_opt.and_then(|x| oi.take().map(|(oia, oib)| (oia, oib, x))),
                    a,
                    b
                );
                *oi = Some((a.clone(), b.clone()));
                (r, didchange)
            })
    }

    pub fn map<R: Value, F: FnMut(&T) -> R + 'static>(&self, f: F) -> Incr<R> {
        let mapper = MapNode {
            input: self.clone().node,
            mapper: f.into(),
        };
        let state = self.node.state();
        let node = Node::<MapNode<F, T, R>>::create_rc(
            state.weak(),
            state.current_scope.borrow().clone(),
            Kind::Map(mapper),
        );
        let map = Incr { node };
        map
    }

    /// A version of map that gives you a (weak) reference to the map node you're making, in the
    /// closure.
    ///
    /// Useful for advanced usage where you want to add manual dependencies with the
    /// `incremental::expert` constructs.
    pub fn map_cyclic<R: Value>(&self, mut cyclic: impl FnMut(WeakIncr<R>, &T) -> R + 'static) -> Incr<R>
    {
        let node = Rc::<Node<_>>::new_cyclic(move |node_weak| {
            let f = {
                let weak = WeakIncr(node_weak.clone());
                move |t: &T| {
                    cyclic(weak.clone(), t)
                }
            };
            let mapper = MapNode {
                input: self.clone().node,
                mapper: f.into(),
            };
            let state = self.node.state();
            let mut node = Node::<MapNode<_, T, R>>::create(
                state.weak(),
                state.current_scope.borrow().clone(),
                Kind::Map(mapper),
            );
            node.weak_self = node_weak.clone();
            node
        });
        node.created_in.add_node(node.clone());

        Incr { node }
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
        let node = Node::<MapWithOld<F, T, R>>::create_rc(
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
        let node = Node::<Map2Node<F, T, T2, R>>::create_rc(
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
        F: FnMut(&WeakState, &T) -> Incr<R> + 'static,
    {
        let cloned = self.node.state().public_weak();
        self.bind(move |value: &T| f(&cloned, value))
    }

    pub fn bind<F, R>(&self, f: F) -> Incr<R>
    where
        R: Value,
        F: FnMut(&T) -> Incr<R> + 'static,
    {
        let state = self.node.state();
        let bind = Rc::new_cyclic(|weak| BindNode {
            lhs: self.clone().node,
            mapper: f.into(),
            rhs: RefCell::new(None),
            rhs_scope: Scope::Bind(weak.clone() as Weak<dyn BindScope>).into(),
            all_nodes_created_on_rhs: RefCell::new(vec![]),
            lhs_change: Weak::new().into(),
            main: Weak::new().into(),
        });
        let lhs_change = Node::<BindLhsChangeNodeGenerics<F, T, R>>::create_rc(
            state.weak(),
            state.current_scope(),
            Kind::BindLhsChange(
                BindLhsId {
                    input_lhs_i2: refl(),
                    r_unit: refl(),
                },
                bind.clone()
            ),
        );
        let main = Node::<BindNodeMainGenerics<F, T, R>>::create_rc(
            state.weak(),
            state.current_scope(),
            Kind::BindMain(
                BindMainId {
                    rhs_r: refl(), input_rhs_i1: refl(), input_lhs_i2: refl()
                },
                bind.clone(),
                lhs_change.clone()
            ),
        );
        {
            let mut bind_lhs_change = bind.lhs_change.borrow_mut();
            let mut bind_main = bind.main.borrow_mut();
            *bind_lhs_change = Rc::downgrade(&lhs_change);
            *bind_main = Rc::downgrade(&main);
        }

        /* We set [lhs_change] to never cutoff so that whenever [lhs] changes, [main] is
        recomputed.  This is necessary to handle cases where [f] returns an existing stable
        node, in which case the [lhs_change] would be the only thing causing [main] to be
        stale. */
        lhs_change.set_cutoff(Cutoff::Never);
        Incr { node: main }
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
        let i = self.incr_filter_mapi(move |_k, v| Some(f(v)));
        #[cfg(debug_assertions)]
        i.node.set_graphviz_user_data(Box::new(format!("incr_map -> {}", std::any::type_name::<T::OutputMap<V2>>())));
        i
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
        let i = self.incr_filter_mapi(move |_k, v| f(v));
        #[cfg(debug_assertions)]
        i.node.set_graphviz_user_data(Box::new(format!("incr_filter_map -> {}", std::any::type_name::<T::OutputMap<V2>>())));
        i
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
        let i = self.incr_filter_mapi(move |k, v| Some(f(k, v)));
        #[cfg(debug_assertions)]
        i.node.set_graphviz_user_data(Box::new(format!("incr_mapi -> {}", std::any::type_name::<T::OutputMap<V2>>())));
        i
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
        let i = self.with_old_input_output(move |old, input| match (old, input.len()) {
            (o @ _, 0) | (o @ None, _) => (input.filter_map_collect(&mut f), true),
            (Some((old_in, mut old_out)), _) => {
                let mut did_change = false;
                let old_out_mut = old_out.make_mut();
                let _: &mut <T::OutputMap<V2> as SymmetricMapMap<K, V2>>::UnderlyingMap = old_in
                    .symmetric_fold(input, old_out_mut, |out, (key, change)| {
                        did_change = true;
                        match change {
                            DiffElement::Left(_) => {
                                out.remove(&key);
                            }
                            DiffElement::Right(newval) => {
                                if let Some(v2) = f(key, newval) {
                                    out.insert(key.clone(), v2);
                                } else {
                                    out.remove(key);
                                }
                            }
                            DiffElement::Unequal(_, newval) => {
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
        });
        #[cfg(debug_assertions)]
        i.node.set_graphviz_user_data(Box::new(format!("incr_filter_mapi -> {}", std::any::type_name::<T::OutputMap<V2>>())));
        i
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
        let i = self.with_old_input_output(move |old, new_in| match old {
            None => {
                let newmap = new_in.nonincremental_fold(init.clone(), |acc, (k, v)| add(acc, k, v));
                (newmap, true)
            }
            Some((old_in, old_out)) => {
                if revert_to_init_when_empty && new_in.is_empty() {
                    return (init.clone(), !old_in.is_empty());
                }
                let mut did_change = false;
                let folded: R = old_in.symmetric_fold(new_in, old_out, |mut acc, (key, difference)| {
                    did_change = true;
                    match difference {
                        DiffElement::Left(value) => remove(acc, key, value),
                        DiffElement::Right(value) => add(acc, key, value),
                        DiffElement::Unequal(lv, rv) => {
                            acc = remove(acc, key, lv);
                            add(acc, key, rv)
                        }
                    }
                });
                (folded, did_change)
            }
        });
        #[cfg(debug_assertions)]
        i.node.set_graphviz_user_data(Box::new(format!("incr_unordered_fold -> {}", std::any::type_name::<R>())));
        i
    }
}

impl<T> DiffElement<T> {
    fn new_data(self) -> Option<T> {
        match self {
            DiffElement::Left(_) => None,
            DiffElement::Right(r) | DiffElement::Unequal(_, r) => Some(r),
        }
    }
}
impl<T: Value> Incr<T> {
    pub fn map_ref<F, R: Value>(&self, f: F) -> Incr<R>
    where
        F: for<'a> Fn(&'a T) -> &'a R + 'static
    {
        let state = self.node.state();
        let node = Node::<MapRefNode<F, T, R>>::create_rc(
            state.weak(),
            state.current_scope(),
            Kind::MapRef(MapRefNode {
                input: self.node.clone(),
                did_change: true.into(),
                mapper: f,
            }),
        );
        Incr { node }
    }
}

pub(crate) struct MapRefNode<F, T, R>
where
    F: Fn(&T) -> &R + 'static,
{
    input: Input<T>,
    mapper: F,
    did_change: Cell<bool>,
}

impl<F, T, R> NodeGenerics for MapRefNode<F, T, R>
where
    T: Value,
    R: Value,
    F: Fn(&T) -> &R + 'static,
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
    type WithOld = fn(Option<Self::R>, &Self::I1) -> (Self::R, bool);
    type FRef = F;
    type Recompute = fn() -> Self::R;
    type ObsChange = fn(bool);
}

impl<F, T, R> Debug for MapRefNode<F, T, R>
where
    F: Fn(&T) -> &R + 'static,
    R: Value,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f
            .debug_struct("MapRefNode")
            .field("did_change", &self.did_change.get())
            .finish()
    }
}


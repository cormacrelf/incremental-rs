#![allow(unused_variables)]

mod adjust_heights_heap;
mod array_fold;
mod cutoff;
mod internal_observer;
mod kind;
mod node;
mod recompute_heap;
mod scope;
mod stabilisation_num;
mod state;
mod symmetric_fold;
mod unordered_fold;
mod var;

use crate::v_rch::kind::BindMainId;
use crate::v_rch::node::Incremental;
use crate::State;

use self::cutoff::Cutoff;
use self::kind::Kind;
use self::node::{ErasedNode, Node};
use self::scope::Scope;
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
    F: FnMut(T1, T2) -> R + 'a,
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
    F: FnMut(T1, T2) -> R + 'a,
{
    type R = R;
    type BindLhs = ();
    type BindRhs = ();
    type I1 = T1;
    type I2 = T2;
    type F1 = fn(Self::I1) -> R;
    type F2 = F;
    type B1 = fn(Self::BindLhs) -> Incr<'a, Self::BindRhs>;
    type Fold = fn(Self::R, Self::I1) -> Self::R;
    type Update = fn(Self::R, Self::I1, Self::I1) -> Self::R;
    type WithOld = fn(Option<Self::R>, Self::I1) -> (Self::R, bool);
}

impl<'a, F, T1, T2, R> Debug for Map2Node<'a, F, T1, T2, R>
where
    F: FnMut(T1, T2) -> R + 'a,
    R: Value<'a>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Map2Node").finish()
    }
}

pub(crate) struct MapNode<'a, F, T, R>
where
    F: FnMut(T) -> R + 'a,
{
    input: Input<'a, T>,
    mapper: RefCell<F>,
}

impl<'a, F, T, R> NodeGenerics<'a> for MapNode<'a, F, T, R>
where
    T: Value<'a>,
    R: Value<'a>,
    F: FnMut(T) -> R + 'a,
{
    type R = R;
    type BindLhs = ();
    type BindRhs = ();
    type I1 = T;
    type I2 = ();
    type F1 = F;
    type F2 = fn(Self::I1, Self::I2) -> R;
    type B1 = fn(Self::BindLhs) -> Incr<'a, Self::BindRhs>;
    type Fold = fn(Self::R, Self::I1) -> Self::R;
    type Update = fn(Self::R, Self::I1, Self::I1) -> Self::R;
    type WithOld = fn(Option<Self::R>, Self::I1) -> (Self::R, bool);
}

impl<'a, F, T, R> Debug for MapNode<'a, F, T, R>
where
    F: FnMut(T) -> R + 'a,
    R: Value<'a>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MapNode").finish()
    }
}

/// Lets you dismantle the old R for parts.
pub(crate) struct MapWithOld<'a, F, T, R>
where
    F: FnMut(Option<R>, T) -> (R, bool) + 'a,
{
    input: Input<'a, T>,
    mapper: RefCell<F>,
    _p: std::marker::PhantomData<R>,
}

impl<'a, F, T, R> NodeGenerics<'a> for MapWithOld<'a, F, T, R>
where
    T: Value<'a>,
    R: Value<'a>,
    F: FnMut(Option<R>, T) -> (R, bool) + 'a,
{
    type R = R;
    type BindLhs = ();
    type BindRhs = ();
    type I1 = T;
    type I2 = ();
    type F1 = fn(Self::I1) -> R;
    type F2 = fn(Self::I1, Self::I2) -> R;
    type B1 = fn(Self::BindLhs) -> Incr<'a, Self::BindRhs>;
    type Fold = fn(Self::R, Self::I1) -> Self::R;
    type Update = fn(Self::R, Self::I1, Self::I1) -> Self::R;
    type WithOld = F;
}

impl<'a, F, T, R> Debug for MapWithOld<'a, F, T, R>
where
    F: FnMut(Option<R>, T) -> (R, bool) + 'a,
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
    F: FnMut(T) -> Incr<'a, R> + 'a,
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
    F: FnMut(T) -> Incr<'a, R> + 'a,
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
    F: FnMut(T) -> Incr<'a, R> + 'a,
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
    type F1 = fn(Self::I1) -> Self::R;
    type F2 = fn(Self::I1, Self::I2) -> Self::R;
    type B1 = F;
    type Fold = fn(Self::R, Self::I1) -> Self::R;
    type Update = fn(Self::R, Self::I1, Self::I1) -> Self::R;
    type WithOld = fn(Option<Self::R>, Self::I1) -> (Self::R, bool);
}

struct BindNodeMainGenerics<F, T, R> {
    _phantom: std::marker::PhantomData<(F, T, R)>,
}

impl<'a, F, T, R> NodeGenerics<'a> for BindNodeMainGenerics<F, T, R>
where
    F: FnMut(T) -> Incr<'a, R> + 'a,
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
    type F1 = fn(Self::I1) -> Self::R;
    type F2 = fn(Self::I1, Self::I2) -> Self::R;
    type B1 = F;
    type Fold = fn(Self::R, Self::I1) -> Self::R;
    type Update = fn(Self::R, Self::I1, Self::I1) -> Self::R;
    type WithOld = fn(Option<Self::R>, Self::I1) -> (Self::R, bool);
}

impl<'a, F, T, R> Debug for BindNode<'a, F, T, R>
where
    F: FnMut(T) -> Incr<'a, R> + 'a,
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

    pub fn enumerate(&self) -> Incr<'a, (usize, T)> {
        let mut counter = 0;
        self.map(move |x| {
            let v = (counter, x);
            counter += 1;
            v
        })
    }

    // TODO: offer this with fewer clones, using a customised map1 node.
    pub fn with_old_borrowed<R, F>(&self, mut f: F) -> Incr<'a, R>
    where
        R: Value<'a>,
        F: FnMut(Option<(T, R)>, &T) -> R + 'a,
    {
        let old: RefCell<Option<(T, R)>> = RefCell::new(None);
        self.map(move |a| {
            let mut o = old.borrow_mut();
            let r: R = f(o.take(), &a);
            *o = Some((a, r.clone()));
            r
        })
    }

    pub fn with_old<R, F>(&self, mut f: F) -> Incr<'a, R>
    where
        R: Value<'a>,
        F: FnMut(Option<(T, R)>, T) -> (R, bool) + 'a,
    {
        let old_input: RefCell<Option<T>> = RefCell::new(None);
        self.map_with_old(move |old_opt, a| {
            let mut oi = old_input.borrow_mut();
            let (b, didchange) = f(old_opt.and_then(|x| oi.take().map(|oi| (oi, x))), a.clone());
            *oi = Some(a);
            (b, didchange)
        })
    }

    pub fn map<R: Value<'a>, F: FnMut(T) -> R + 'a>(&self, f: F) -> Incr<'a, R> {
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

    pub fn map_with_old<R, F>(&self, f: F) -> Incr<'a, R>
    where
        R: Value<'a>,
        F: FnMut(Option<R>, T) -> (R, bool) + 'a,
    {
        let state = self.node.state();
        let node = Node::<MapWithOld<'a, F, T, R>>::create(
            state.weak(),
            state.current_scope(),
            Kind::MapWithOld(MapWithOld {
                input: self.clone().node,
                mapper: f.into(),
                _p: Default::default(),
            })
        );
        Incr { node }
    }

    pub fn map2<F, T2, R>(&self, other: &Incr<'a, T2>, f: F) -> Incr<'a, R>
    where
        T2: Value<'a>,
        R: Value<'a>,
        F: FnMut(T, T2) -> R + 'a,
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

    pub fn binds<F, R>(&self, mut f: F) -> Incr<'a, R>
    where
        R: Value<'a>,
        F: FnMut(&Rc<State<'a>>, T) -> Incr<'a, R> + 'a,
    {
        let cloned = self.node.state();
        self.bind(move |value: T| f(&cloned, value))
    }

    pub fn bind<F, R>(&self, f: F) -> Incr<'a, R>
    where
        R: Value<'a>,
        F: FnMut(T) -> Incr<'a, R> + 'a,
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

    pub fn observe(&self) -> Observer<'a, T> {
        let incr = self.clone();
        let internal = incr.node.state().observe(incr);
        Observer::new(internal)
    }

    pub fn set_cutoff(&self, cutoff: Cutoff<T>) {
        self.node.set_cutoff(cutoff);
    }
    pub fn cutoff(self, cutoff: Cutoff<T>) -> Self {
        self.node.set_cutoff(cutoff);
        self
    }
}

#[test]
fn cutoff() {
    let incr = State::new();
    let rc = Rc::new(10);
    let v = incr.var(rc.clone());
    let w = v.watch().set_cutoff(Cutoff::Custom(Rc::ptr_eq));
}

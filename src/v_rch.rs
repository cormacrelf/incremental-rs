#![allow(unused_variables)]

mod adjust_heights_heap;
mod array_fold;
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
use crate::State;

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
pub trait Value<'a>: Debug + Clone + 'a {}
impl<'a, T> Value<'a> for T where T: Debug + Clone + 'a {}
pub(crate) type NodeRef<'a> = Rc<dyn ErasedNode<'a> + 'a>;
pub(crate) type WeakNode<'a> = Weak<dyn ErasedNode<'a> + 'a>;

#[derive(Clone, Debug)]
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
    T1: Debug + Clone + 'a,
    T2: Debug + Clone + 'a,
    R: Debug + Clone + 'a,
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
}

impl<'a, F, T1, T2, R> Debug for Map2Node<'a, F, T1, T2, R>
where
    F: FnMut(T1, T2) -> R + 'a,
    R: Debug + 'a,
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
    T: Debug + Clone + 'a,
    R: Debug + Clone + 'a,
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
}

impl<'a, F, T, R> Debug for MapNode<'a, F, T, R>
where
    F: FnMut(T) -> R + 'a,
    R: Debug + 'a,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MapNode").finish()
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
    R: Debug + Clone + 'a,
    T: Debug + Clone + 'a,
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
    T: Debug + Clone + 'a,
    R: Debug + Clone + 'a,
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
}

struct BindNodeMainGenerics<F, T, R> {
    _phantom: std::marker::PhantomData<(F, T, R)>,
}

impl<'a, F, T, R> NodeGenerics<'a> for BindNodeMainGenerics<F, T, R>
where
    F: FnMut(T) -> Incr<'a, R> + 'a,
    T: Debug + Clone + 'a,
    R: Debug + Clone + 'a,
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
}

impl<'a, F, T, R> Debug for BindNode<'a, F, T, R>
where
    F: FnMut(T) -> Incr<'a, R> + 'a,
    R: Debug + Clone + 'a,
    T: Debug + Clone + 'a,
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

    // pub fn new(value: T) -> Self {
    //     let raw = Rc::new(RawValue {
    //         value: RefCell::new(value),
    //     });
    //     let node = Node::create(self.node.state(), node::Kind::<T, RawValue<T>>::Raw)
    //     Incr { node: raw }
    // }

    pub fn enumerate(&self) -> Incr<'a, (usize, T)> {
        let mut counter = 0;
        self.map(move |x| {
            let v = (counter, x);
            counter += 1;
            v
        })
    }

    pub fn with_old<R, F>(&self, mut f: F) -> Incr<'a, R>
    where
        R: Value<'a>,
        F: FnMut(Option<(T, R)>, T) -> R + 'a,
    {
        let old: RefCell<Option<(T, R)>> = RefCell::new(None);
        self.map(move |a| {
            let mut o = old.borrow_mut();
            let b: R = f(o.take(), a.clone());
            *o = Some((a, b.clone()));
            b
        })
    }

    pub fn map<R: Value<'a>, F: FnMut(T) -> R + 'a>(&self, f: F) -> Incr<'a, R> {
        let mapper = MapNode {
            input: self.clone().node,
            mapper: f.into(),
        };
        let state = self.node.state();
        let node = Node::<MapNode<'a, F, T, R>>::create(
            state.clone(),
            state.current_scope.borrow().clone(),
            Kind::Map(mapper),
        );
        let map = Incr { node };
        map
    }

    pub fn map2<F, T2, R>(&self, other: &Incr<'a, T2>, f: F) -> Incr<'a, R>
    where
        T2: Clone + Debug + 'a,
        R: Clone + Debug + 'a,
        F: FnMut(T, T2) -> R + 'a,
    {
        let mapper = Map2Node {
            one: self.clone().node,
            two: other.clone().node,
            mapper: f.into(),
        };
        let state = self.node.state();
        let node = Node::<Map2Node<'a, F, T, T2, R>>::create(
            state.clone(),
            state.current_scope.borrow().clone(),
            Kind::Map2(mapper),
        );
        let map = Incr { node };
        map
    }

    // pub fn list_all(list: Vec<Incr<'a, T>>) -> Incr<'a, Vec<T>> {
    //     let output = list.iter().map(|input| input.node.latest()).collect();
    //     let cloned = list.clone();
    //     let listall = ListAllNode {
    //         inputs: list,
    //         output: RefCell::new(output),
    //         prev: RefCell::new(None),
    //     };
    //     let new = Incr {
    //         node: Rc::new(listall),
    //     };
    //     for inp in cloned.iter() {
    //         inp.node.add_descendant(new.node.clone().as_any());
    //     }
    //     new
    // }

    pub fn binds<F, R>(&self, mut f: F) -> Incr<'a, R>
    where
        R: Debug + Clone + 'a,
        F: FnMut(&Rc<State<'a>>, T) -> Incr<'a, R> + 'a,
    {
        let cloned = self.node.state();
        self.bind(move |value: T| f(&cloned, value))
    }

    pub fn bind<F, R>(&self, mut f: F) -> Incr<'a, R>
    where
        R: Debug + Clone + 'a,
        F: FnMut(T) -> Incr<'a, R> + 'a,
    {
        let state = self.node.state();
        let lhs_change = Node::<BindLhsChangeNodeGenerics<F, T, R>>::create(
            state.clone(),
            state.current_scope(),
            Kind::Uninitialised,
        );
        println!(
            "creating bind lhs with scope height {:?}",
            state.current_scope().height()
        );
        let main = Node::<BindNodeMainGenerics<F, T, R>>::create(
            self.node.state(),
            state.current_scope(),
            Kind::Uninitialised,
        );
        println!(
            "creating bind main with scope height {:?}",
            state.current_scope().height()
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
            bind.clone(),
        );
        main_incr
    }

    pub fn observe(&self) -> Observer<'a, T> {
        let incr = self.clone();
        let internal = incr.node.state().observe(incr);
        Observer::new(internal)
    }

    pub fn filter(&self, should_emit: impl Fn(&T, &T) -> bool + 'a) -> Incr<'a, T> {
        let cutoff = CutoffNode {
            input: self.node.clone(),
            should_emit: Box::new(should_emit),
        };
        let state = self.node.state();
        let node = Node::<CutoffNode<'a, T>>::create(
            state.clone(),
            state.current_scope(),
            Kind::Cutoff(cutoff),
        );
        Incr { node }
    }
}

pub(crate) struct CutoffNode<'a, R> {
    input: Input<'a, R>,
    should_emit: Box<dyn Fn(&R, &R) -> bool + 'a>,
}

impl<'a, R: Debug + Clone + 'a> NodeGenerics<'a> for CutoffNode<'a, R> {
    type R = R;
    type BindRhs = ();
    type BindLhs = ();
    type I1 = R;
    type I2 = ();
    type F1 = fn(Self::I1) -> R;
    type F2 = fn(Self::I1, Self::I2) -> R;
    type B1 = fn(Self::BindLhs) -> Incr<'a, Self::BindRhs>;
    type Fold = fn(Self::R, Self::I1) -> Self::R;
    type Update = fn(Self::R, Self::I1, Self::I1) -> Self::R;
}

impl<'a, R: Debug + 'a> Debug for CutoffNode<'a, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CutoffNode")
            .field("value", &self.input.latest())
            .finish()
    }
}

struct RawValue<T> {
    value: T,
}

impl<T> Debug for RawValue<T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RawValue")
            .field("value", &self.value)
            .finish()
    }
}

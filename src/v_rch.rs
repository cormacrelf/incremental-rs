#![allow(unused_variables)]

mod adjust_heights_heap;
mod internal_observer;
mod node;
mod recompute_heap;
mod scope;
mod stabilisation_num;
mod state;
mod var;

use self::adjust_heights_heap::NodeRef;
use self::node::{ErasedNode, Node, NodeGenerics, WeakNode};
use self::scope::Scope;
use fmt::Debug;
use std::cell::RefCell;
use std::fmt;
use std::rc::{Rc, Weak};

pub mod public;
use public::Observer;

use node::Input;

#[derive(Clone, Debug)]
pub struct Incr<T> {
    node: Input<T>,
}

pub(crate) struct Map2Node<F, T1, T2, R>
where
    F: Fn(T1, T2) -> R,
{
    one: Input<T1>,
    two: Input<T2>,
    mapper: F,
}

impl<F, T1, T2, R> NodeGenerics for Map2Node<F, T1, T2, R>
where
    T1: Debug + Clone + 'static,
    T2: Debug + Clone + 'static,
    R: Debug + Clone + 'static,
    F: Fn(T1, T2) -> R + 'static,
{
    type Output = R;
    type R = R;
    type I1 = T1;
    type I2 = T2;
    type F1 = fn(Self::I1) -> R;
    type F2 = F;
    type B1 = fn(Self::I1) -> Incr<R>;
}

impl<F, T1, T2, R> Debug for Map2Node<F, T1, T2, R>
where
    F: Fn(T1, T2) -> R,
    R: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Map2Node").finish()
    }
}

pub(crate) struct MapNode<F, T, R>
where
    F: Fn(T) -> R,
{
    input: Input<T>,
    mapper: F,
}

impl<F, T, R> NodeGenerics for MapNode<F, T, R>
where
    T: Debug + Clone + 'static,
    R: Debug + Clone + 'static,
    F: Fn(T) -> R + 'static,
{
    type Output = R;
    type R = R;
    type I1 = T;
    type I2 = ();
    type F1 = F;
    type F2 = fn(Self::I1, Self::I2) -> R;
    type B1 = fn(Self::I1) -> Incr<R>;
}

impl<F, T, R> Debug for MapNode<F, T, R>
where
    F: Fn(T) -> R,
    R: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MapNode").finish()
    }
}

pub(crate) struct BindNode<F, T, R>
where
    R: Debug + Clone + 'static,
    T: Debug + Clone + 'static,
    F: Fn(T) -> Incr<R> + 'static,
{
    lhs_change: Rc<Node<BindLhsChangeNodeGenerics<F, T, R>>>,
    main: Weak<Node<BindNodeGenerics<F, T, R>>>,
    lhs: Input<T>,
    mapper: F,
    rhs: RefCell<Option<Incr<R>>>,
    rhs_scope: RefCell<Scope>,
    // to_disconnect: RefCell<Option<RefCell<Incr<R>>>>,
    all_nodes_created_on_rhs: RefCell<Vec<Weak<dyn ErasedNode>>>,
}

pub(crate) trait BindScope: Debug {
    fn is_valid(&self) -> bool;
    fn is_necessary(&self) -> bool;
    fn height(&self) -> i32;
    fn add_node(&self, node: WeakNode);
}

impl<F, T, R> BindScope for BindNode<F, T, R>
where
    R: Debug + Clone + 'static,
    T: Debug + Clone + 'static,
    F: Fn(T) -> Incr<R> + 'static,
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
        let mut all = self.all_nodes_created_on_rhs.borrow_mut();
        all.push(node);
    }
}

struct BindLhsChangeNodeGenerics<F, T, R> {
    _phantom: std::marker::PhantomData<(F, T, R)>,
}

impl<F, T, R> NodeGenerics for BindLhsChangeNodeGenerics<F, T, R>
where
    F: Fn(T) -> Incr<R> + 'static,
    T: Debug + Clone + 'static,
    R: Debug + Clone + 'static,
{
    type Output = ();
    type R = R;
    type I1 = T;
    type I2 = ();
    type F1 = fn(Self::I1) -> R;
    type F2 = fn(Self::I1, Self::I2) -> R;
    type B1 = F;
}

struct BindNodeGenerics<F, T, R> {
    _phantom: std::marker::PhantomData<(F, T, R)>,
}

impl<F, T, R> NodeGenerics for BindNodeGenerics<F, T, R>
where
    F: Fn(T) -> Incr<R> + 'static,
    T: Debug + Clone + 'static,
    R: Debug + Clone + 'static,
{
    type Output = R;
    type R = R;
    type I1 = T;
    type I2 = ();
    type F1 = fn(Self::I1) -> R;
    type F2 = fn(Self::I1, Self::I2) -> R;
    type B1 = F;
}

impl<F, T, R> Debug for BindNode<F, T, R>
where
    F: Fn(T) -> Incr<R> + 'static,
    R: Debug + Clone + 'static,
    T: Debug + Clone + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BindNode")
            .field("output", &self.rhs.borrow().as_ref().map(|x| &x.node))
            .finish()
    }
}

pub(crate) struct ListAllNode<R> {
    inputs: Vec<Incr<R>>,
    output: RefCell<Vec<R>>,
    prev: RefCell<Option<Vec<Incr<R>>>>,
}

impl<R> Debug for ListAllNode<R>
where
    R: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ListAllNode")
            // .field("inputs", &self.inputs)
            .finish()
    }
}
impl<T: Clone + 'static + Debug> Incr<T> {
    pub(crate) fn ptr_eq(&self, other: &Incr<T>) -> bool {
        Rc::ptr_eq(&self.node, &other.node)
    }
    // pub fn new(value: T) -> Self {
    //     let raw = Rc::new(RawValue {
    //         value: RefCell::new(value),
    //     });
    //     let node = Node::create(self.node.state(), node::Kind::<T, RawValue<T>>::Raw)
    //     Incr { node: raw }
    // }

    pub fn map<R: Clone + 'static + Debug, F: Fn(T) -> R + 'static>(&self, f: F) -> Incr<R> {
        let mapper = MapNode {
            input: self.clone().node,
            mapper: f,
        };
        let state = self.node.state();
        let node = Node::<MapNode<F, T, R>>::create(
            state.clone(),
            state.current_scope.borrow().clone(),
            node::Kind::Map(mapper),
        );
        let map = Incr {
            node: node.into_rc(),
        };
        map
    }

    pub fn map2<F, T2, R>(&self, other: &Incr<T2>, f: F) -> Incr<R>
    where
        T2: Clone + 'static + Debug,
        R: Clone + 'static + Debug,
        F: Fn(T, T2) -> R + 'static,
    {
        let mapper = Map2Node {
            one: self.clone().node,
            two: other.clone().node,
            mapper: f,
        };
        let state = self.node.state();
        let node = Node::<Map2Node<F, T, T2, R>>::create(
            state.clone(),
            state.current_scope.borrow().clone(),
            node::Kind::Map2(mapper),
        );
        let map = Incr {
            node: node.into_rc(),
        };
        map
    }

    // pub fn list_all(list: Vec<Incr<T>>) -> Incr<Vec<T>> {
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

    pub fn bind<F, R>(&self, f: F) -> Incr<R>
    where
        R: Debug + Clone + 'static,
        F: Fn(T) -> Incr<R> + 'static,
    {
        let state = self.node.state();
        let lhs_change = Node::<BindLhsChangeNodeGenerics<F, T, R>>::create(
            state.clone(),
            state.current_scope(),
            node::Kind::Uninitialised,
        )
        .into_rc();
        println!(
            "creating bind lhs with scope height {:?}",
            state.current_scope().height()
        );
        let main = Node::<BindNodeGenerics<F, T, R>>::create(
            self.node.state(),
            state.current_scope(),
            node::Kind::Uninitialised,
        )
        .into_rc();
        println!(
            "creating bind main with scope height {:?}",
            state.current_scope().height()
        );
        let bind = Rc::new(BindNode {
            lhs: self.clone().node,
            mapper: f,
            rhs: RefCell::new(None),
            rhs_scope: RefCell::new(Scope::Top),
            all_nodes_created_on_rhs: RefCell::new(vec![]),
            lhs_change,
            main: Rc::downgrade(&main),
        });
        let bind_scope = Scope::Bind(Rc::downgrade(&bind) as Weak<dyn BindScope>);
        let mut rhs_scope = bind.rhs_scope.borrow_mut();
        *rhs_scope = bind_scope;

        let main_incr = Incr { node: main.clone() };
        let mut main_kind = main.kind.borrow_mut();
        *main_kind = node::Kind::BindMain(bind.clone());
        let mut lhs_change_kind = bind.lhs_change.kind.borrow_mut();
        *lhs_change_kind = node::Kind::BindLhsChange(bind.clone());
        main_incr
    }

    pub(crate) fn value(&self) -> T {
        self.node.latest()
    }

    pub(crate) fn value_opt(&self) -> T {
        self.node.latest()
    }

    pub fn observe(&self) -> Observer<T> {
        let incr = self.clone();
        let internal = incr.node.state().observe(incr);
        Observer::new(internal)
    }

    pub fn filter(&self, should_emit: impl Fn(&T, &T) -> bool + 'static) -> Incr<T> {
        let cutoff = CutoffNode {
            input: self.node.clone(),
            should_emit: Box::new(should_emit),
        };
        let state = self.node.state();
        let node = Node::<CutoffNode<T>>::create(
            state.clone(),
            state.current_scope(),
            node::Kind::Cutoff(cutoff),
        );
        Incr {
            node: node.into_rc(),
        }
    }
}

pub(crate) struct CutoffNode<R> {
    input: Input<R>,
    should_emit: Box<dyn Fn(&R, &R) -> bool>,
}

impl<R: Debug + Clone + 'static> NodeGenerics for CutoffNode<R> {
    type Output = R;
    type R = R;
    type I1 = R;
    type I2 = ();
    type F1 = fn(Self::I1) -> R;
    type F2 = fn(Self::I1, Self::I2) -> R;
    type B1 = fn(Self::I1) -> Incr<R>;
}

impl<R: Debug + 'static> Debug for CutoffNode<R> {
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

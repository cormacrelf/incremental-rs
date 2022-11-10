#![allow(dead_code)]
#![allow(unused_variables)]
//! This is version 1.
//!
//! We have dynamic incremental update graphs here, due to a pretty simple mechanism:
//!
//! 1. When you create a new node from an existing incremental node, e.g. via map, the new node
//!    becomes a "descendant" of the original. This entails adding the new node to a list on the
//!    parent node.
//! 2. When you call stabilise on a node, it will walk through the entire DAG of its descendants.
//!    It will do this in two passes at each level:
//!
//!    - First, adding +1 to the dirty count on each of its direct descendants, and recursively
//!    stabilising that descendant node.
//!    - Second, subtracting 1 from the dirty count on each of its direct descendants. If it
//!    reaches 0, then it calls eval() on the descendant.
//!
//!    In this fashion, if you have 10,000 inputs and half of them are updated in one go, and you
//!    then stabilise every one of those updated inputs, then you go through two passes where much
//!    of the call graph is +5,000, and eventually gets back to zero and we re-evaluate. Those
//!    dirty counts track whether a node has seen all the new values it's supposed to receive.
//!
//!
//! The main problem with V1 is that there's no way to cut off computation of values when the
//! eval() has not changed. Hence v2.

use fmt::Debug;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

#[cfg(test)]
mod test;

/// Incr<R> is meant to be the "sentinel" as in the 7 implementations talk.
/// The problem that sentinels solve is a lot easier in Rust with explicit reference counting and
/// std::rc::Weak. Because all the 'descendants' are Weak pointers, when an Incr (with a strong Rc
/// pointer) is dropped then all those weak pointers might end up dead. This way, Incr is the root
/// of ownership of the computation graph. All the parent pointers inside the node types are also
/// strong Rc pointers, so this works the same way. This is that parent pointer type.
///
/// We also do not suffer the problem where the GC hasn't kicked in yet to collect a sentinel, and
/// therefore not only is the memory for all that discarded graph (from a bind that's just thrown
/// out a whole subgraph, e.g.) not freed, but the entire subgraph stays in action and keeps
/// getting updated, dirtied, stabilised and evaluated. In Rust, Rc dies when it dies. There is no
/// lag between when we get rid of a subgraph and when it stops computing itself.
type Input<T> = Rc<dyn Thunk<T>>;

/// Weak pointer to any kind of thunk, from a parent node perspective. All we need to know about it
/// is how to increment/decrement its dirty count and make it re-evaluate however it wishes to do
/// that.
type Descendant = Weak<dyn AnyThunk>;

struct Flags {
    dirty_count: Cell<usize>,
    descendants: RefCell<Vec<Descendant>>,
}

impl Flags {
    fn empty() -> Self {
        Flags {
            dirty_count: Cell::new(0),
            descendants: RefCell::new(vec![]),
        }
    }
}

fn do_propagate_dirty_count<A: AnyThunk + ?Sized>(thunk: &A) {
    let mut descendants = thunk.flags().descendants.borrow_mut();
    descendants.retain(|p_weak| {
        if let Some(parent) = p_weak.upgrade() {
            let val = parent.flags().dirty_count.get();
            // println!("dirty count: {}", val);
            parent.flags().dirty_count.set(val + 1);
            parent.propagate_dirty_count();
            true
        } else {
            false
        }
    });
    drop(descendants);
}
fn do_stabilise<A: AnyThunk + ?Sized>(thunk: &A) {
    let mut descendants = thunk.flags().descendants.borrow_mut();
    let mut to_reconnect = Vec::new();
    for desc_weak in descendants.iter_mut() {
        if let Some(desc) = Weak::upgrade(desc_weak) {
            let mut val = desc.flags().dirty_count.get();
            if val == 0 {
                return;
            }
            // println!("dirty count: {}", val);
            val = val - 1;
            desc.flags().dirty_count.set(val);
            if val == 0 {
                if desc.eval() {
                    to_reconnect.push(desc_weak.clone());
                }
            }
        }
    }
    drop(descendants);
    for desc in to_reconnect {
        if let Some(d) = Weak::upgrade(&desc) {
            d.reconnect();
        }
    }
}

trait AnyThunk: Any + Debug {
    fn eval(&self) -> bool;
    fn reconnect(self: Rc<Self>) {}
    fn flags(&self) -> &Flags;
    /// What Var::set() calls.
    fn propagate_dirty_count(&self) {
        do_propagate_dirty_count(self);
    }
    /// Resolve all the variables that have dirtied the graph.
    fn stabilise(&self) {
        do_stabilise(self);
    }
}

trait Thunk<T>: AnyThunk {
    fn latest(&self) -> T;
    // fn as_any(self: Rc<Self>) -> Parent;
    fn as_any(self: Rc<Self>) -> Descendant;
    fn add_descendant(&self, p: Descendant) {
        if let Ok(mut descendants) = self.flags().descendants.try_borrow_mut() {
            descendants.push(p);
        } else {
            panic!()
        }
    }
    fn remove_descendant(&self, p: Descendant) {
        if let Ok(mut descendants) = self.flags().descendants.try_borrow_mut() {
            descendants.retain(|d| !d.ptr_eq(&p))
        } else {
            panic!()
        }
    }
}

struct RawValue<T> {
    value: RefCell<T>,
    self_dirty: Cell<bool>,
    flags: Flags,
}

impl<T> Debug for RawValue<T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RawValue")
            .field("value", &self.value.borrow())
            .field("self_dirty", &self.self_dirty.get())
            .finish()
    }
}

impl<T: 'static + Debug> AnyThunk for RawValue<T> {
    fn eval(&self) -> bool {
        false
    }
    fn flags(&self) -> &Flags {
        &self.flags
    }
    /// Slight modification of the default, to set self_dirty
    fn propagate_dirty_count(&self) {
        // called from set(), so yeah, it's changed
        self.self_dirty.set(true);
        do_propagate_dirty_count(self);
    }
    fn stabilise(&self) {
        if !self.self_dirty.get() {
            return;
        }
        self.self_dirty.set(false);
        do_stabilise(self);
    }
}
impl<T: Clone + 'static + Debug> Thunk<T> for RawValue<T> {
    fn latest(&self) -> T {
        self.value.borrow().clone()
    }
    fn as_any(self: Rc<Self>) -> Descendant {
        let weak = Rc::downgrade(&self);
        weak as Descendant
    }
}

struct Map2Node<F, T1, T2, R>
where
    F: Fn(T1, T2) -> R,
{
    one: Input<T1>,
    two: Input<T2>,
    mapper: F,
    value: RefCell<R>,
    flags: Flags,
}

impl<F, T1, T2, R> Debug for Map2Node<F, T1, T2, R>
where
    F: Fn(T1, T2) -> R,
    R: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Map2Node")
            .field("value", &self.value)
            .field("flags.dirty_count", &self.flags.dirty_count.get())
            .finish()
    }
}

impl<F, T1: Clone + 'static + Debug, T2: Clone + 'static + Debug, R: Clone + 'static + Debug>
    AnyThunk for Map2Node<F, T1, T2, R>
where
    F: Fn(T1, T2) -> R + 'static,
{
    fn eval(&self) -> bool {
        let mut value = self.value.borrow_mut();
        *value = (self.mapper)(self.one.latest(), self.two.latest());
        false
    }
    fn flags(&self) -> &Flags {
        &self.flags
    }
}
impl<F, T1: Clone + 'static + Debug, T2: Clone + 'static + Debug, R: Clone + 'static + Debug>
    Thunk<R> for Map2Node<F, T1, T2, R>
where
    F: Fn(T1, T2) -> R + 'static,
{
    fn latest(&self) -> R {
        self.value.borrow().clone()
    }
    fn as_any(self: Rc<Self>) -> Descendant {
        let weak = Rc::downgrade(&self);
        weak as Descendant
    }
}

struct MapNode<F, T, R>
where
    F: Fn(T) -> R,
{
    input: Input<T>,
    mapper: F,
    value: RefCell<R>,
    flags: Flags,
}

use std::fmt;
impl<F, T, R> Debug for MapNode<F, T, R>
where
    F: Fn(T) -> R,
    R: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MapNode")
            .field("value", &self.value)
            .field("flags.dirty_count", &self.flags.dirty_count.get())
            .finish()
    }
}

impl<F, T: Clone + 'static + Debug, R: Clone + 'static + Debug> AnyThunk for MapNode<F, T, R>
where
    F: Fn(T) -> R + 'static,
{
    fn eval(&self) -> bool {
        // println!("eval {:?}", self);
        let mut val = self.value.borrow_mut();
        *val = (self.mapper)(self.input.latest());
        false
    }
    fn flags(&self) -> &Flags {
        &self.flags
    }
}
impl<F, T: Clone + 'static + Debug, R: Clone + 'static + Debug> Thunk<R> for MapNode<F, T, R>
where
    F: Fn(T) -> R + 'static,
{
    fn latest(&self) -> R {
        self.value.borrow().clone()
    }
    fn as_any(self: Rc<Self>) -> Descendant {
        let weak = Rc::downgrade(&self);
        weak as Descendant
    }
}

struct BindNode<F, T, R>
where
    F: Fn(T) -> Incr<R>,
{
    input: Input<T>,
    mapper: F,
    output: RefCell<Incr<R>>,
    to_disconnect: RefCell<Option<RefCell<Incr<R>>>>,
    flags: Flags,
}

impl<F, T, R> Debug for BindNode<F, T, R>
where
    F: Fn(T) -> Incr<R>,
    R: Debug + Clone + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BindNode")
            .field("output", &self.output.borrow().value())
            .field("flags.dirty_count", &self.flags.dirty_count.get())
            .finish()
    }
}

impl<F, T, R> AnyThunk for BindNode<F, T, R>
where
    T: Clone + 'static + Debug,
    R: Clone + 'static + Debug,
    F: Fn(T) -> Incr<R> + 'static,
{
    fn eval(&self) -> bool {
        println!("BindNode eval triggered");
        let new_output = (self.mapper)(self.input.latest());
        if !self.output.borrow().ptr_eq(&new_output) {
            let orig = self.output.clone();
            let mut o = self.output.borrow_mut();
            let mut todisc = self.to_disconnect.borrow_mut();
            *todisc = Some(orig);
            *o = new_output;
        }
        true
    }
    fn reconnect(self: Rc<Self>) {
        let mut maybe_todisc = self.to_disconnect.borrow_mut();
        if let Some(todisc) = maybe_todisc.take() {
            // Disconnect previous
            // println!("disconnecting BindNode from previous output {:?}", todisc);
            todisc
                .borrow_mut()
                .thunk
                .remove_descendant(self.clone().as_any());
            // Connect new one
            let output = self.output.borrow_mut();
            // println!("reconnecting BindNode to output {:?}", output);
            // We want the bind node to be a descendant of whichever Incr was returned by the mapper
            // function.
            output.thunk.add_descendant(self.clone().as_any());
        }
    }
    fn flags(&self) -> &Flags {
        &self.flags
    }
}

impl<F, T, R> Thunk<R> for BindNode<F, T, R>
where
    T: Clone + 'static + Debug,
    R: Clone + 'static + Debug,
    F: Fn(T) -> Incr<R> + 'static,
{
    fn latest(&self) -> R {
        self.output.borrow().value()
    }
    fn as_any(self: Rc<Self>) -> Descendant {
        let weak = Rc::downgrade(&self);
        weak as Descendant
    }
}

struct ListAllNode<R> {
    inputs: Vec<Incr<R>>,
    output: RefCell<Vec<R>>,
    prev: RefCell<Option<Vec<Incr<R>>>>,
    flags: Flags,
}

impl<R> Debug for ListAllNode<R>
where
    R: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ListAllNode")
            .field("inputs", &self.inputs)
            .field("flags.dirty_count", &self.flags.dirty_count.get())
            .finish()
    }
}
impl<R> AnyThunk for ListAllNode<R>
where
    R: Clone + 'static + Debug,
{
    fn eval(&self) -> bool {
        println!("ListAllNode eval triggered");
        let output: Vec<R> = self.inputs.iter().map(|inp| inp.thunk.latest()).collect();
        let mut o = self.output.borrow_mut();
        *o = output;
        true
    }
    fn reconnect(self: Rc<Self>) {
        let mut prev = self.prev.borrow_mut();
        if let Some(prev) = prev.take() {
            for p in prev {
                p.thunk.remove_descendant(self.clone().as_any());
            }
            for i in self.inputs.iter() {
                i.thunk.add_descendant(self.clone().as_any());
            }
        }
    }

    fn flags(&self) -> &Flags {
        &self.flags
    }
}

impl<R: Clone + 'static + Debug> Thunk<Vec<R>> for ListAllNode<R> {
    fn as_any(self: Rc<Self>) -> Descendant {
        let weak = Rc::downgrade(&self);
        weak as Descendant
    }

    fn latest(&self) -> Vec<R> {
        self.output.borrow().clone()
    }
}

#[derive(Clone, Debug)]
pub struct Incr<T> {
    thunk: Input<T>,
}

impl<T: Clone + 'static + Debug> Incr<T> {
    pub fn ptr_eq(&self, other: &Incr<T>) -> bool {
        crate::rc_fat_ptr_eq(&self.thunk, &other.thunk)
    }
    pub fn new(value: T) -> Self {
        let raw = Rc::new(RawValue {
            value: RefCell::new(value),
            self_dirty: Cell::new(false),
            flags: Flags {
                dirty_count: Cell::new(0),
                descendants: RefCell::new(vec![]),
            },
        });
        Incr { thunk: raw }
    }
    pub fn map<R: Clone + 'static + Debug>(&self, f: impl Fn(T) -> R + 'static) -> Incr<R> {
        let value = f(self.value());
        let node = MapNode {
            input: self.clone().thunk,
            mapper: f,
            value: RefCell::new(value),
            flags: Flags {
                dirty_count: Cell::new(0),
                descendants: RefCell::new(vec![]),
            },
        };
        let map = Incr {
            thunk: Rc::new(node),
        };
        self.thunk.add_descendant(map.thunk.clone().as_any());
        map
    }
    pub fn map2<T2: Clone + 'static + Debug, R: Clone + 'static + Debug>(
        &self,
        other: &Incr<T2>,
        f: impl Fn(T, T2) -> R + 'static,
    ) -> Incr<R> {
        let value = f(self.value(), other.value());
        let map = Map2Node {
            one: self.clone().thunk,
            two: other.clone().thunk,
            mapper: f,
            value: RefCell::new(value),
            flags: Flags::empty(),
        };
        let map = Incr {
            thunk: Rc::new(map),
        };
        self.thunk.add_descendant(map.thunk.clone().as_any());
        map
    }
    pub fn list_all(list: Vec<Incr<T>>) -> Incr<Vec<T>> {
        let output = list.iter().map(|input| input.thunk.latest()).collect();
        let cloned = list.clone();
        let listall = ListAllNode {
            inputs: list,
            output: RefCell::new(output),
            prev: RefCell::new(None),
            flags: Flags::empty(),
        };
        let new = Incr {
            thunk: Rc::new(listall),
        };
        for inp in cloned.iter() {
            inp.thunk.add_descendant(new.thunk.clone().as_any());
        }
        new
    }
    pub fn bind<R: Clone + 'static + Debug>(&self, f: impl Fn(T) -> Incr<R> + 'static) -> Incr<R> {
        let output = f(self.value());
        let mut output = RefCell::new(output);
        let mapper = BindNode {
            input: self.clone().thunk,
            mapper: f,
            output: output.clone(),
            to_disconnect: RefCell::new(None),
            flags: Flags {
                dirty_count: Cell::new(0),
                descendants: RefCell::new(vec![]),
            },
        };
        let bind = Incr {
            thunk: Rc::new(mapper),
        };
        // We add the bind as a descendant of self, so the BindNode is a descendant of the original
        // incr.
        self.thunk.add_descendant(bind.thunk.clone().as_any());
        // But we also want it to be a descendant of whichever Incr returned by the mapper
        // function.
        output
            .get_mut()
            .thunk
            .add_descendant(bind.thunk.clone().as_any());
        bind
    }
    pub fn stabilise(&self) {
        self.thunk.stabilise();
    }
    pub fn value(&self) -> T {
        self.thunk.latest()
    }
    pub fn on_update(&self, callback: Box<dyn Fn(T)>) -> T {
        todo!()
    }
}

pub struct Var<T> {
    incr: Incr<T>,
    raw: Rc<RawValue<T>>,
}

impl<T: Clone + 'static + Debug> Var<T> {
    pub fn create(a: T) -> Self {
        let raw = Rc::new(RawValue {
            value: RefCell::new(a),
            self_dirty: Cell::new(false),
            flags: Flags {
                dirty_count: Cell::new(0),
                descendants: RefCell::new(vec![]),
            },
        });
        let incr = Incr { thunk: raw.clone() };
        Var { incr, raw }
    }
    pub fn set(&self, new_val: T) {
        let mut val_ref = self.raw.value.borrow_mut();
        *val_ref = new_val;
        self.incr.thunk.propagate_dirty_count();
    }
    pub fn read(&self) -> Incr<T> {
        self.incr.clone()
    }
}

use core::ops::Deref;
impl<T: Clone + 'static + Debug> Deref for Var<T> {
    type Target = Incr<T>;
    fn deref(&self) -> &Self::Target {
        &self.incr
    }
}

pub trait ShouldEmit {
    fn should_emit(&self, next_value: &Self) -> bool;
}

impl<T> ShouldEmit for T
where
    T: PartialEq,
{
    fn should_emit(&self, next_value: &Self) -> bool {
        self != next_value
    }
}

#![allow(unused_variables)]

use fmt::Debug;
use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

type Input<T> = Rc<dyn Thunk<T>>;
type Descendant = Weak<dyn AnyThunk>;

struct Flags {
    dirty_count: Cell<usize>,
    descendants: RefCell<Vec<Descendant>>,
}

trait AnyThunk: Any + Debug {
    fn eval(self: Rc<Self>) -> bool;
    fn reconnect(self: Rc<Self>) {}
    fn flags(&self) -> &Flags;
    fn stabilise(&self) {
        let mut descendants = self.flags().descendants.borrow_mut();
        descendants.retain(|p_weak| {
            if let Some(parent) = p_weak.upgrade() {
                let val = parent.flags().dirty_count.get();
                parent.flags().dirty_count.set(val + 1);
                parent.stabilise();
                true
            } else {
                false
            }
        });
        drop(descendants);
        let mut descendants = self.flags().descendants.borrow_mut();
        let mut to_reconnect = Vec::new();
        for desc_weak in descendants.iter_mut() {
            if let Some(desc) = Weak::upgrade(desc_weak) {
                let mut val = desc.flags().dirty_count.get();
                if val == 0 {
                    return;
                }
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
    flags: Flags,
}

impl<T> Debug for RawValue<T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("RawValue")
            .field("value", &self.value.borrow())
            .finish()
    }
}

impl<T: 'static + Debug> AnyThunk for RawValue<T> {
    fn eval(self: Rc<Self>) -> bool {
        false
    }
    fn flags(&self) -> &Flags {
        &self.flags
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
    fn eval(self: Rc<Self>) -> bool {
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
    fn eval(self: Rc<Self>) -> bool {
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
    fn eval(self: Rc<Self>) -> bool {
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

#[derive(Clone, Debug)]
pub struct Incr<T> {
    thunk: Input<T>,
}

impl<T: Clone + 'static + Debug> Incr<T> {
    pub fn ptr_eq(&self, other: &Incr<T>) -> bool {
        Rc::ptr_eq(&self.thunk, &other.thunk)
    }
    pub fn new(value: T) -> Self {
        let raw = Rc::new(RawValue {
            value: RefCell::new(value),
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
            flags: Flags {
                dirty_count: Cell::new(0),
                descendants: RefCell::new(vec![]),
            },
        };
        let map = Incr {
            thunk: Rc::new(map),
        };
        self.thunk.add_descendant(map.thunk.clone().as_any());
        map
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

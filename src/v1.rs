#![allow(unused_variables)]

use std::any::Any;
use std::rc::{Rc, Weak};
use std::cell::Cell;

#[derive(Default)]
struct Flags {
    dirty: Cell<usize>,
}

trait AnyThunk: Any {
    fn flags(&self) -> &Flags;
}

trait Thunk<T>: AnyThunk {
    fn eval(&mut self);
    fn latest(&self) -> T;
    // fn as_any(self: Rc<Self>) -> Weak<dyn AnyThunk>;
    fn as_any(self: Rc<Self>) -> Weak<dyn AnyThunk>;
}

struct RawValue<T> {
    value: T,
}

impl<T: 'static> AnyThunk for RawValue<T> {
    fn flags(&self) -> &Flags {
        todo!()
    }
}
impl<T: Clone + 'static> Thunk<T> for RawValue<T> {
    fn eval(&mut self) {
        // noop
    }
    fn latest(&self) -> T {
        self.value.clone()
    }
    fn as_any(self: Rc<Self>) -> Weak<dyn AnyThunk> {
        let weak = Rc::downgrade(&self);
        weak as Weak<dyn AnyThunk>
    }
}

struct Map2Node<F, T1, T2, R> where F: Fn(T1, T2) -> R {
    one: Incr<T1>,
    two: Incr<T2>,
    mapper: F,
    value: R,
    flags: Flags,
}

impl<F: Fn(T1, T2) -> R + 'static, T1: 'static, T2: 'static, R: 'static> AnyThunk for Map2Node<F, T1, T2, R> {
    fn flags(&self) -> &Flags {
        &self.flags
    }
}
impl<F, T1: Clone + 'static, T2: Clone + 'static, R: Clone + 'static> Thunk<R> for Map2Node<F, T1, T2, R> where F: Fn(T1, T2) -> R + 'static {
    fn eval(&mut self) {
        self.value = (self.mapper)(self.one.value(), self.two.value());
    }

    fn latest(&self) -> R {
        self.value.clone()
    }
    fn as_any(self: Rc<Self>) -> Weak<dyn AnyThunk> {
        let weak = Rc::downgrade(&self);
        weak as Weak<dyn AnyThunk>
    }
}

struct MapNode<F, T, R> where F: Fn(T) -> R {
    input: Incr<T>,
    mapper: F,
    value: R,
    flags: Flags,
}

impl<F: Fn(T1) -> R + 'static, T1: 'static, R: 'static> AnyThunk for MapNode<F, T1, R> {
    fn flags(&self) -> &Flags {
        &self.flags
    }
}
impl<F, T: Clone + 'static, R: Clone + 'static> Thunk<R> for MapNode<F, T, R> where F: Fn(T) -> R + 'static {
    fn eval(&mut self) {
        self.value = (self.mapper)(self.input.value());
    }
    fn latest(&self) -> R {
        self.value.clone()
    }
    fn as_any(self: Rc<Self>) -> Weak<dyn AnyThunk> {
        let weak = Rc::downgrade(&self);
        weak as Weak<dyn AnyThunk>
    }
}

struct BindNode<F, T, R> where F: Fn(T) -> Incr<R> {
    input: Incr<T>,
    mapper: F,
    output: Incr<R>,
    flags: Flags,
}

impl<F, T: Clone + 'static, R: Clone + 'static> AnyThunk for BindNode<F, T, R> where F: Fn(T) -> Incr<R> + 'static {
    fn flags(&self) -> &Flags {
        &self.flags
    }
}
impl<F, T: Clone + 'static, R: Clone + 'static> Thunk<R> for BindNode<F, T, R> where F: Fn(T) -> Incr<R> + 'static {
    fn eval(&mut self) {
        self.output = (self.mapper)(self.input.value());
    }
    fn latest(&self) -> R {
        self.output.value()
    }
    fn as_any(self: Rc<Self>) -> Weak<dyn AnyThunk> {
        let weak = Rc::downgrade(&self);
        weak as Weak<dyn AnyThunk>
    }
}

#[derive(Clone)]
pub struct Incr<T> {
    thunk: Rc<dyn Thunk<T>>,
    parent: Option<Weak<dyn AnyThunk>>,
}

impl<T: Clone + 'static> Incr<T> {
    pub fn new(value: T) -> Self where T: 'static {
        Incr {
            thunk: Rc::new(RawValue { value }),
            parent: None,
        }
    }
    pub fn map<R: Clone + 'static>(mut self, f: impl Fn(T) -> R + 'static) -> Incr<R> {
        let value = f(self.value());
        let node = MapNode {
            input: self.clone(),
            mapper: f,
            value,
            flags: Flags::default(),
        };
        let map = Incr {
            thunk: Rc::new(node),
            parent: None
        };
        self.parent = Some(map.thunk.clone().as_any());
        map
    }
    pub fn map2<T2: Clone + 'static, R: Clone + 'static>(mut self, other: &Incr<T2>, f: impl Fn(T, T2) -> R + 'static) -> Incr<R> {
        let value = f(self.value(), other.value());
        let map = Map2Node {
            one: self.clone(),
            two: other.clone(),
            mapper: f,
            value,
            flags: Flags::default(),
        };
        let map = Incr {
            thunk: Rc::new(map),
            parent: None,
        };
        self.parent = Some(map.thunk.clone().as_any());
        map
    }
    pub fn bind<R: Clone + 'static>(mut self, f: impl Fn(T) -> Incr<R> + 'static) -> Incr<R> {
        let output = f(self.value());
        let mapper = BindNode {
            input: self.clone(),
            mapper: f,
            output,
            flags: Flags::default(),
        };
        let bind = Incr {
            thunk: Rc::new(mapper),
            parent: None
        };
        self.parent = Some(bind.thunk.clone().as_any());
        bind
    }
    pub fn stabilise(&self) {
        // self.rc.eval();
        todo!()
    }
    pub fn value(&self) -> T {
        self.thunk.latest().clone()
    }
    pub fn on_update(&self, callback: Box<dyn Fn(T)>) -> T {
        todo!()
    }
}

pub struct Var<T> {
    raw: Incr<T>,
}

impl<T: Clone + 'static> Var<T> {
    pub fn create(a: T) -> Self {
        let raw = Incr::new(a);
        Var { raw }
    }
    pub fn set(&mut self, new_val: T) -> Self {
        todo!()
    }
    pub fn read(&self) -> Incr<T> {
        self.raw.clone()
    }
}



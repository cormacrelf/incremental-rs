#![allow(unused_variables)]

use std::any::Any;
use std::rc::{Rc, Weak};
use std::cell::{Cell, RefCell};

struct Flags {
    dirty: Cell<usize>,
    parent: RefCell<Vec<Weak<dyn AnyThunk>>>,
}

trait AnyThunk: Any {
    fn flags(&self) -> &Flags;
}

trait Thunk<T>: AnyThunk {
    fn eval(&mut self);
    fn latest(&self) -> T;
    // fn as_any(self: Rc<Self>) -> Weak<dyn AnyThunk>;
    fn as_any(self: Rc<Self>) -> Weak<dyn AnyThunk>;
    fn add_parent(&self, p: Weak<dyn AnyThunk>) {
        if let Ok(mut parent) = self.flags().parent.try_borrow_mut() {
            parent.push(p);
        } else {
            panic!()
        }
    }
}

struct RawValue<T> {
    value: RefCell<T>,
    flags: Flags,
}

impl<T: 'static> AnyThunk for RawValue<T> {
    fn flags(&self) -> &Flags {
        &self.flags
    }
}
impl<T: Clone + 'static> Thunk<T> for RawValue<T> {
    fn eval(&mut self) {
        // noop
    }
    fn latest(&self) -> T {
        self.value.borrow().clone()
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
}

impl<T: Clone + 'static> Incr<T> {
    fn new(value: T) -> (Self, Rc<RawValue<T>>) where T: 'static {
        let raw = Rc::new(RawValue {
            value: RefCell::new(value),
            flags: Flags { dirty: Cell::new(0), parent: RefCell::new(vec![]), }
        });
        let incr = Incr {
            thunk: raw.clone(),
        };
        (incr, raw)
    }
    pub fn map<R: Clone + 'static>(&self, f: impl Fn(T) -> R + 'static) -> Incr<R> {
        let value = f(self.value());
        let node = MapNode {
            input: self.clone(),
            mapper: f,
            value,
            flags: Flags { dirty: Cell::new(0), parent: RefCell::new(vec![]), }
        };
        let map = Incr {
            thunk: Rc::new(node),
        };
        self.thunk.add_parent(map.thunk.clone().as_any());
        map
    }
    pub fn map2<T2: Clone + 'static, R: Clone + 'static>(&self, other: &Incr<T2>, f: impl Fn(T, T2) -> R + 'static) -> Incr<R> {
        let value = f(self.value(), other.value());
        let map = Map2Node {
            one: self.clone(),
            two: other.clone(),
            mapper: f,
            value,
            flags: Flags { dirty: Cell::new(0), parent: RefCell::new(vec![]), }
        };
        let map = Incr {
            thunk: Rc::new(map),
        };
        self.thunk.add_parent(map.thunk.clone().as_any());
        map
    }
    pub fn bind<R: Clone + 'static>(&self, f: impl Fn(T) -> Incr<R> + 'static) -> Incr<R> {
        let output = f(self.value());
        let mapper = BindNode {
            input: self.clone(),
            mapper: f,
            output,
            flags: Flags { dirty: Cell::new(0), parent: RefCell::new(vec![]), }
        };
        let bind = Incr {
            thunk: Rc::new(mapper),
        };
        self.thunk.add_parent(bind.thunk.clone().as_any());
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
    incr: Incr<T>,
    raw: Rc<RawValue<T>>,
}

impl<T: Clone + 'static> Var<T> {
    pub fn create(a: T) -> Self {
        let (incr, raw) = Incr::new(a);
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



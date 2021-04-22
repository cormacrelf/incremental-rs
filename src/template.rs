#![allow(unused_variables)]

use std::marker::PhantomData;
pub struct Incr<T> {
    _phantom: PhantomData<T>,
}

impl<T> Incr<T> {
    pub fn new(a: T) -> Self {
        todo!()
    }
    pub fn map<R, F: Fn(T) -> R>(&self, f: impl Fn(T) -> R) -> Incr<R> {
        todo!();
    }
    pub fn map2<T2, R>(&self, other: &Self, f: impl Fn(T, T2) -> R) -> Incr<R> {
        todo!();
    }
    pub fn bind<R>(&self, f: impl Fn(T) -> Incr<R>) -> Incr<R> {
        todo!();
    }
    pub fn stabilise() {
        todo!()
    }
    pub fn value(&self) -> T {
        todo!()
    }
    pub fn on_update(&self, callback: Box<dyn Fn(T)>) -> T {
        todo!()
    }
}

pub struct Var<T> {
    _phantom: PhantomData<T>,
}

impl<T> Var<T> {
    pub fn create(a: T) -> Self {
        todo!()
    }
    pub fn set(&mut self, new_val: T) -> Self {
        todo!()
    }
    pub fn read(&self) -> T {
        todo!()
    }
}



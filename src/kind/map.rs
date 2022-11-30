use std::cell::RefCell;
use std::marker::PhantomData;
use std::{cell::Cell, fmt};

use super::NodeGenerics;
use crate::node::Input;
use crate::{Incr, Value};

pub(crate) struct MapNode<F, T, R>
where
    F: FnMut(&T) -> R,
{
    pub input: Input<T>,
    pub mapper: RefCell<F>,
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

impl<F, T, R> fmt::Debug for MapNode<F, T, R>
where
    F: FnMut(&T) -> R,
    R: Value,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MapNode").finish()
    }
}

pub(crate) struct MapRefNode<F, T, R>
where
    F: Fn(&T) -> &R + 'static,
{
    pub(crate) input: Input<T>,
    pub(crate) mapper: F,
    pub(crate) did_change: Cell<bool>,
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

impl<F, T, R> fmt::Debug for MapRefNode<F, T, R>
where
    F: Fn(&T) -> &R + 'static,
    R: Value,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MapRefNode")
            .field("did_change", &self.did_change.get())
            .finish()
    }
}

pub(crate) struct Map2Node<F, T1, T2, R>
where
    F: FnMut(&T1, &T2) -> R,
{
    pub(crate) one: Input<T1>,
    pub(crate) two: Input<T2>,
    pub(crate) mapper: RefCell<F>,
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

impl<F, T1, T2, R> fmt::Debug for Map2Node<F, T1, T2, R>
where
    F: FnMut(&T1, &T2) -> R,
    R: Value,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Map2Node").finish()
    }
}

/// Lets you dismantle the old R for parts.
pub(crate) struct MapWithOld<F, T, R>
where
    F: FnMut(Option<R>, &T) -> (R, bool),
{
    pub input: Input<T>,
    pub mapper: RefCell<F>,
    pub _p: PhantomData<R>,
}

impl<F, T, R> MapWithOld<F, T, R>
where
    F: FnMut(Option<R>, &T) -> (R, bool),
{
    pub fn new(input: Input<T>, mapper: F) -> Self {
        Self {
            input,
            mapper: RefCell::new(mapper),
            _p: PhantomData,
        }
    }
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

impl<F, T, R> fmt::Debug for MapWithOld<F, T, R>
where
    F: FnMut(Option<R>, &T) -> (R, bool),
    R: Value,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MapWithOld").finish()
    }
}

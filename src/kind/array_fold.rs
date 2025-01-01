use std::cell::RefCell;
use std::fmt::{self, Debug};

use super::NodeGenerics;
use super::{Incr, Value};
use crate::incrsan::NotObserver;

pub(crate) trait FoldFunction<I, R>: FnMut(R, &I) -> R + NotObserver {}
impl<F, I, R> FoldFunction<I, R> for F where F: FnMut(R, &I) -> R + NotObserver {}

pub(crate) struct ArrayFold<I, R> {
    pub(crate) init: R,
    pub(crate) fold: RefCell<Box<dyn FoldFunction<I, R>>>,
    pub(crate) children: Vec<Incr<I>>,
}

impl<I, R> ArrayFold<I, R>
where
    I: Value,
    R: Value,
{
    pub(crate) fn compute(&self) -> R {
        let acc = self.init.clone();
        let mut f = self.fold.borrow_mut();
        self.children.iter().fold(acc, |acc, x| {
            let v = x.node.value_as_ref().unwrap();
            f(acc, &v)
        })
    }
}

impl<I: Value, R: Value> NodeGenerics for ArrayFold<I, R> {
    type R = R;
    type I1 = I;
    node_generics_default! { I2, I3, I4, I5, I6 }
    node_generics_default! { F1, F2, F3, F4, F5, F6 }
    node_generics_default! { B1, BindLhs, BindRhs, Update, WithOld, FRef, Recompute, ObsChange }
}

impl<I, R> Debug for ArrayFold<I, R>
where
    R: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ArrayFold")
            .field("len", &self.children.len())
            .finish()
    }
}

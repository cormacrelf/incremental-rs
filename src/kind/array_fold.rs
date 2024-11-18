use std::cell::RefCell;
use std::fmt::{self, Debug};

use super::NodeGenerics;
use super::{Incr, Value};
use crate::incrsan::NotObserver;

pub(crate) struct ArrayFold<F, I, R> {
    pub(crate) init: R,
    pub(crate) fold: RefCell<F>,
    pub(crate) children: Vec<Incr<I>>,
}

impl<F, I, R> ArrayFold<F, I, R>
where
    F: FnMut(R, &I) -> R + NotObserver,
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

impl<F, I: Value, R: Value> NodeGenerics for ArrayFold<F, I, R>
where
    F: FnMut(R, &I) -> R + 'static + NotObserver,
{
    type R = R;
    type I1 = I;
    type Fold = F;
    node_generics_default! { I2, I3, I4, I5, I6 }
    node_generics_default! { F1, F2, F3, F4, F5, F6 }
    node_generics_default! { B1, BindLhs, BindRhs, Update, WithOld, FRef, Recompute, ObsChange }
}

impl<F, I, R> Debug for ArrayFold<F, I, R>
where
    R: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ArrayFold")
            .field("len", &self.children.len())
            .finish()
    }
}

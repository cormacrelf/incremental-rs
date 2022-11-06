use super::kind::NodeGenerics;
use super::{Incr, Value};
use std::cell::RefCell;
use std::fmt::{self, Debug};

pub(crate) struct ArrayFold<F, I, R> {
    pub(crate) init: R,
    pub(crate) fold: RefCell<F>,
    pub(crate) children: Vec<Incr<I>>,
}

impl<F, I, R> ArrayFold<F, I, R>
where
    F: FnMut(R, &I) -> R,
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
    F: FnMut(R, &I) -> R + 'static,
{
    type R = R;
    type BindLhs = ();
    type BindRhs = ();
    type I1 = I;
    type I2 = ();
    type F1 = fn(&Self::I1) -> R;
    type F2 = fn(&Self::I1, &Self::I2) -> R;
    type B1 = fn(&Self::BindLhs) -> Incr<Self::BindRhs>;
    type Fold = F;
    type Update = fn(Self::R, &Self::I1, &Self::I1) -> Self::R;
    type WithOld = fn(Option<Self::R>, &Self::I1) -> (Self::R, bool);
    type FRef = fn(&Self::I1) -> &Self::R;
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

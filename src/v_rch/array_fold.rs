use super::node::NodeGenerics;
use super::{Incr, Value};
use std::cell::RefCell;
use std::fmt::{self, Debug};

pub(crate) struct ArrayFold<'a, F, I, R> {
    pub(crate) init: R,
    pub(crate) fold: RefCell<F>,
    pub(crate) children: Vec<Incr<'a, I>>,
}

impl<'a, F, I, R> ArrayFold<'a, F, I, R>
where
    F: FnMut(R, I) -> R + 'a,
    I: Value<'a>,
    R: Value<'a>,
{
    pub(crate) fn compute(&self) -> R {
        let acc = self.init.clone();
        let mut f = self.fold.borrow_mut();
        self.children.iter().fold(acc, |acc, x| {
            let v = x.node.latest();
            f(acc, v)
        })
    }
}

impl<'a, F, I: Value<'a>, R: Value<'a>> NodeGenerics<'a> for ArrayFold<'a, F, I, R>
where
    F: FnMut(R, I) -> R + 'a,
{
    type R = R;
    type D = ();
    type I1 = I;
    type I2 = ();
    type F1 = fn(Self::I1) -> R;
    type F2 = fn(Self::I1, Self::I2) -> R;
    type B1 = fn(Self::I1) -> Incr<'a, Self::D>;
    type Fold = F;
}

impl<'a, F, I, R> Debug for ArrayFold<'a, F, I, R>
where
    R: Debug + 'a,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ListAllNode")
            // .field("inputs", &self.inputs)
            .finish()
    }
}

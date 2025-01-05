use std::any::Any;
use std::cell::RefCell;
use std::fmt::{self, Debug};

use miny::Miny;

use super::NodeGenerics;
use super::{Incr, Value};
use crate::incrsan::NotObserver;
use crate::kind::KindTrait;
use crate::NodeRef;

pub(crate) struct ArrayFold<F, I, R> {
    pub(crate) init: R,
    pub(crate) fold: RefCell<F>,
    pub(crate) children: Vec<Incr<I>>,
}

impl<F, I, R> KindTrait for ArrayFold<F, I, R>
where
    F: FnMut(R, &I) -> R + 'static + NotObserver,
    I: Value,
    R: Value,
{
    fn compute(&self) -> Miny<dyn Any> {
        let mut acc = self.init.clone();
        let mut f = self.fold.borrow_mut();
        acc = self.children.iter().fold(acc, |acc, x| {
            let v = x.node.value_as_ref().unwrap();
            f(acc, &v)
        });
        Miny::new_unsized(acc)
    }
    fn children_len(&self) -> usize {
        self.children.len()
    }
    fn iter_children_packed(&self) -> Box<dyn Iterator<Item = NodeRef> + '_> {
        Box::new(self.children.iter().map(|x| x.node.packed()))
    }
    fn slow_get_child(&self, index: usize) -> NodeRef {
        self.children[index].node.packed()
    }
    fn debug_ty(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ArrayFold<[{}] -> {}>",
            std::any::type_name::<I>(),
            std::any::type_name::<R>()
        )
    }
}

impl<F, I: Value, R: Value> NodeGenerics for ArrayFold<F, I, R>
where
    F: 'static + NotObserver,
{
    type R = R;
    node_generics_default! { I1, I2, I3, I4, I5, I6 }
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

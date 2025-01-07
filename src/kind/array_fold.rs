use std::cell::RefCell;
use std::fmt::{self, Debug};

use super::{Incr, Value};
use crate::boxes::{new_unsized, SmallBox};
use crate::incrsan::NotObserver;
use crate::kind::KindTrait;
use crate::{NodeRef, ValueInternal};

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
    fn compute(&self) -> SmallBox<dyn ValueInternal> {
        let mut acc = self.init.clone();
        let mut f = self.fold.borrow_mut();
        acc = self.children.iter().fold(acc, |acc, x| {
            let v = x.node.value_as_ref().unwrap();
            f(acc, &v)
        });
        new_unsized!(acc)
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

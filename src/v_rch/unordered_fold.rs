use std::{cell::RefCell, rc::Rc};

use crate::{Incr, Value};

use super::kind::NodeGenerics;
use super::node::Input;

enum Update<'a, A, B, FInv, FUpd>
where
    FInv: FnMut(B, A) -> B + 'a,
    FUpd: FnMut(B, A, A) -> B + 'a,
{
    FInverse(FInv),
    Update(FUpd),
    _Phantom(
        std::convert::Infallible,
        std::marker::PhantomData<&'a (A, B)>,
    ),
}

impl<'a, A, B, FInv, FUpd> Update<'a, A, B, FInv, FUpd>
where
    FInv: FnMut(B, A) -> B + 'a,
    FUpd: FnMut(B, A, A) -> B + 'a,
{
    fn update<F: FnMut(B, A) -> B + 'a>(mut self, mut f: F) -> impl FnMut(B, A, A) -> B + 'a {
        move |fold_value, old_value, new_value| match &mut self {
            Self::FInverse(ref mut f_inv) => f(f_inv(fold_value, old_value), new_value),
            Self::Update(ref mut update) => update(fold_value, old_value, new_value),
            Self::_Phantom(..) => unreachable!(),
        }
    }
}

pub(crate) struct UnorderedArrayFold<'a, F, U, I, R> {
    pub(crate) init: R,
    pub(crate) fold: RefCell<F>,
    pub(crate) update: RefCell<U>,
    pub(crate) fold_value: RefCell<Option<R>>,
    pub(crate) children: Vec<Incr<'a, I>>,
}

impl<'a, F, U, I, R> std::fmt::Debug for UnorderedArrayFold<'a, F, U, I, R>
where
    I: Value<'a>,
    R: Value<'a>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnorderedArrayFold")
            .field("init", &self.init)
            .field("num_children", &self.children.len())
            .field("fold_value", &self.fold_value)
            .finish()
    }
}

impl<'a, F, U, I, R> UnorderedArrayFold<'a, F, U, I, R>
where
    F: FnMut(R, I) -> R + 'a,
    U: FnMut(R, I, I) -> R + 'a,
    I: Value<'a>,
    R: Value<'a>,
{
    pub(crate) fn full_compute(&self) -> R {
        let acc = self.init.clone();
        let mut f = self.fold.borrow_mut();
        self.children.iter().fold(acc, |acc, x| {
            let v = x.node.latest();
            f(acc, v)
        })
    }
    pub(crate) fn compute(&mut self) {
        /*  let compute t =
          if t.num_changes_since_last_full_compute = t.full_compute_every_n_changes
          then (
            t.num_changes_since_last_full_compute <- 0;
            t.fold_value <- Uopt.some (full_compute t));
        */
        self.fold_value.replace(Some(self.full_compute()));
    }
    pub(crate) fn child_changed(
        &self,
        child: &Input<'a, I>,
        child_index: i32,
        old_value_opt: Option<I>,
        new_value: I,
    ) {
        let own_child = &self.children[child_index as usize];
        assert!(Rc::ptr_eq(&child.packed(), &own_child.node.packed()));
        // if t.num_changes_since_last_full_compute < t.full_compute_every_n_changes - 1
        // then (
        let _old = self.fold_value.replace_with(|old| {
            let mut u = self.update.borrow_mut();
            /* We only reach this case if we have already done a full compute, in which case
            [Uopt.is_some t.fold_value] and [Uopt.is_some old_value_opt]. */
            let x = (u)(
                old.as_ref().unwrap().clone(),
                old_value_opt.unwrap(),
                new_value,
            );
            Some(x)
        });
    }
}

impl<'a, F, U, I: Value<'a>, R: Value<'a>> NodeGenerics<'a> for UnorderedArrayFold<'a, F, U, I, R>
where
    F: FnMut(R, I) -> R + 'a,
    U: FnMut(R, I, I) -> R + 'a,
{
    type R = R;
    type BindRhs = ();
    type BindLhs = ();
    type I1 = I;
    type I2 = ();
    type F1 = fn(Self::I1) -> R;
    type F2 = fn(Self::I1, Self::I2) -> R;
    type B1 = fn(Self::BindLhs) -> Incr<'a, Self::BindRhs>;
    type Fold = F;
    type Update = U;
}

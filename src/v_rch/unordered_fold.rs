use std::cell::Cell;
use std::{cell::RefCell, rc::Rc};
#[cfg(test)]
use test_log::test;

use crate::{Incr, State, Value};

use super::kind::NodeGenerics;
use super::node::Input;

#[derive(Copy, Clone, Debug)]
enum Cycle {
    EveryN { current: u32, n: u32 },
    Never,
}

impl Cycle {
    /// New cycle counter.
    /// - None makes it never trigger.
    /// - Some makes an EveryN cycle starting at N.
    fn new(every_n: Option<u32>) -> Self {
        match every_n {
            Some(n) => Self::EveryN { current: 0, n },
            None => Self::Never,
        }
    }
    fn reset(cell: &Cell<Self>) {
        let v = match cell.get() {
            Self::EveryN { n, .. } => Self::EveryN { current: n, n },
            Self::Never => Self::Never,
        };
        cell.set(v);
    }
    fn increment(cell: &Cell<Self>) -> bool {
        let mut v = cell.get();
        let w = v.next();
        cell.set(v);
        w
    }
    fn next(&mut self) -> bool {
        match self {
            Self::EveryN { current, n } if *current + 1 >= *n => {
                *current = 0;
                true
            }
            Self::EveryN { current, .. } => {
                *current += 1;
                false
            }
            Self::Never => false,
        }
    }
}

#[test]
fn test_cycle() {
    let mut cycle = Cycle::EveryN { current: 8, n: 10 };
    let wrapped = cycle.next();
    assert_eq!(wrapped, false);
    let wrapped = cycle.next();
    assert_eq!(wrapped, true);
    let wrapped = cycle.next();
    assert_eq!(wrapped, false);
}

pub(crate) fn make_update_fn_from_inverse<B, A, F, FInv>(
    mut f: F,
    mut f_inv: FInv,
) -> impl FnMut(B, A, A) -> B
where
    F: FnMut(B, A) -> B,
    FInv: FnMut(B, A) -> B,
{
    move |fold_value, old_value, new_value| {
        // imagine f     is |a, x| a + x
        //         f_inv is |a, x| a - x
        // this produces n update function
        //         |a, old, new| (a - old) + new
        f(f_inv(fold_value, old_value), new_value)
    }
}

pub(crate) struct UnorderedArrayFold<'a, F, U, I, R> {
    pub(crate) init: R,
    pub(crate) fold: RefCell<F>,
    pub(crate) update: RefCell<U>,
    pub(crate) fold_value: RefCell<Option<R>>,
    pub(crate) children: Vec<Incr<'a, I>>,
    cycle: Cell<Cycle>,
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
            .field("fold_value", &self.fold_value.borrow())
            .field("cycle", &self.cycle.get())
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
    pub(crate) fn create_node(
        state: &Rc<State<'a>>,
        vec: Vec<Incr<'a, I>>,
        init: R,
        f: F,
        update: U,
        full_compute_every_n_changes: Option<u32>,
    ) -> Incr<'a, R> {
        let node = super::node::Node::<UnorderedArrayFold<'a, F, _, I, R>>::create(
            state.weak(),
            state.current_scope(),
            super::kind::Kind::UnorderedArrayFold(UnorderedArrayFold {
                init,
                update: update.into(),
                fold: f.into(),
                children: vec,
                fold_value: None.into(),
                cycle: Cycle::new(full_compute_every_n_changes).into(),
            }),
        );
        Incr { node }
    }

    pub(crate) fn full_compute(&self) -> R {
        let acc = self.init.clone();
        let mut f = self.fold.borrow_mut();
        self.children.iter().fold(acc, |acc, x| {
            let v = x.node.latest();
            f(acc, v)
        })
    }

    pub(crate) fn compute(&self) -> R {
        let mut fv = self.fold_value.borrow_mut();
        if fv.is_none() {
            Cycle::reset(&self.cycle);
            fv.replace(self.full_compute());
        }
        fv.as_ref().cloned().unwrap()
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

        self.fold_value.replace_with(|old| {
            let wrapped = Cycle::increment(&self.cycle);
            if wrapped || old.is_none() {
                None
            } else {
                let mut update = self.update.borrow_mut();
                /* We only reach this case if we have already done a full compute, in which case
                [Uopt.is_some t.fold_value] and [Uopt.is_some old_value_opt]. */
                let x = update(
                    old.as_ref().unwrap().clone(),
                    old_value_opt.unwrap(),
                    new_value,
                );
                Some(x)
            }
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

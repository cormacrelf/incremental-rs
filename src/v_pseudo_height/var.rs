use crate::v_pseudo_height::node::ErasedNode;
use crate::v_pseudo_height::state::IncrStatus;

use super::node::{Incremental, Node, NodeGenerics};
use super::stabilisation_num::StabilisationNum;
use super::state::State;
use super::Incr;
use core::fmt::Debug;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub(crate) struct VarGenerics<T: Debug + Clone + 'static>(std::marker::PhantomData<T>);
impl<R: Debug + Clone + 'static> NodeGenerics for VarGenerics<R> {
    type Output = R;
    type R = R;
    type I1 = ();
    type I2 = ();
    type F1 = fn(Self::I1) -> R;
    type F2 = fn(Self::I1, Self::I2) -> R;
    type B1 = fn(Self::I1) -> Incr<R>;
}

pub struct Var<T: Debug + Clone + 'static> {
    pub(crate) state: Rc<State>,
    pub(crate) value: RefCell<T>,
    pub(crate) set_at: Cell<StabilisationNum>,
    pub(crate) node: Rc<Node<VarGenerics<T>>>,
}

impl<T: Debug + Clone + 'static> Debug for Var<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Var")
            .field("set_at", &self.set_at.get())
            .field("value", &self.value.borrow())
            .finish()
    }
}

impl<T: Debug + Clone + 'static> Var<T> {
    pub fn get(&self) -> T {
        self.value.borrow().clone()
    }
    pub fn set(&self, value: T) {
        let t = self.node.inner().borrow().state.clone();
        match t.status.get() {
            IncrStatus::NotStabilising => {
                t.num_var_sets.set(t.num_var_sets.get() + 1);
                let mut value_slot = self.value.borrow_mut();
                *value_slot = value;
                println!(
                    "set_at {:?}, stab_num {:?}",
                    self.set_at.get(),
                    t.stabilisation_num.get()
                );
                if self.set_at.get() < t.stabilisation_num.get() {
                    self.set_at.set(t.stabilisation_num.get());
                    let watch = self.node.clone();
                    debug_assert!(watch.is_stale());
                    if watch.is_necessary() && !watch.is_in_recompute_heap() {
                        let mut heap = t.recompute_heap.borrow_mut();
                        heap.insert(watch.weak());
                    }
                }
            }
            _ => todo!("setting variable while stabilising..."),
        }
    }
    pub fn watch(&self) -> Incr<T> {
        Incr {
            node: self.node.clone().as_input(),
        }
    }
}

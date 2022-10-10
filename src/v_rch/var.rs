use super::node::ErasedNode;
use super::state::IncrStatus;

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
    pub(crate) node: RefCell<Option<Rc<Node<VarGenerics<T>>>>>,
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
        let Some(watch) = self.node.borrow().clone() else {
            panic!("uninitialised var");
        };
        let t = watch.inner().borrow().state.clone();
        match t.status.get() {
            IncrStatus::NotStabilising => {
                t.num_var_sets.set(t.num_var_sets.get() + 1);
                let mut value_slot = self.value.borrow_mut();
                *value_slot = value;
                if self.set_at.get() < t.stabilisation_num.get() {
                    println!(
                        "variable set at t={:?}, current revision is t={:?}",
                        self.set_at.get().0,
                        t.stabilisation_num.get().0
                    );
                    self.set_at.set(t.stabilisation_num.get());
                    debug_assert!(watch.is_stale());
                    if watch.is_necessary() && !watch.is_in_recompute_heap() {
                        println!(
                            "inserting var watch into recompute heap at height {:?}",
                            watch.height()
                        );
                        let mut heap = t.recompute_heap.borrow_mut();
                        heap.insert(watch.packed());
                    }
                }
            }
            _ => todo!("setting variable while stabilising..."),
        }
    }
    pub fn watch(&self) -> Incr<T> {
        Incr {
            node: self
                .node
                .borrow()
                .clone()
                .expect("var was not initialised")
                .as_input(),
        }
    }
}

#[cfg(test)]
impl<T: Debug + Clone + 'static> Drop for Var<T> {
    fn drop(&mut self) {
        println!(
            "$$$$$$$$$ Dropping var with id {:?}",
            self.node.borrow().as_ref().map(|n| n.id)
        );
    }
}

#[test]
fn var_drop() {
    let incr = State::new();
    println!("before watch created");
    let w = incr.var(10).watch();
    println!("watch created");
    let o = w.observe();
    incr.stabilise();
    assert_eq!(o.value(), Ok(10));
}


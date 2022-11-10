use crate::Value;
#[cfg(test)]
use test_log::test;

use super::kind::NodeGenerics;
use super::node::{ErasedNode, Incremental, Node, NodeId};
use super::stabilisation_num::StabilisationNum;
use super::state::IncrStatus;
use super::state::State;
use super::{CellIncrement, Incr};
use core::fmt::Debug;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

pub(crate) struct VarGenerics<T: Value>(std::marker::PhantomData<T>);
impl<R: Value> NodeGenerics for VarGenerics<R> {
    type R = R;
    type BindLhs = ();
    type BindRhs = ();
    type I1 = ();
    type I2 = ();
    type F1 = fn(&Self::I1) -> Self::R;
    type F2 = fn(&Self::I1, &Self::I2) -> Self::R;
    type B1 = fn(&Self::BindLhs) -> Incr<Self::BindRhs>;
    type Fold = fn(Self::R, &Self::I1) -> Self::R;
    type Update = fn(Self::R, &Self::I1, &Self::I1) -> Self::R;
    type WithOld = fn(Option<Self::R>, &Self::I1) -> (Self::R, bool);
    type FRef = fn(&Self::I1) -> &Self::R;
    type Recompute = fn() -> Self::R;
    type ObsChange = fn(bool);
}

// For the delayed variable set list (set_during_stabilisation).
// We use Weak to ensure we don't interfere with the manual
// Rc-cycle-breaking on public::Var.
pub(crate) type WeakVar = Weak<dyn ErasedVariable>;

pub(crate) trait ErasedVariable: Debug {
    fn set_var_stabilise_end(&self);
    fn id(&self) -> NodeId;
    fn break_rc_cycle(&self);
}

impl<T: Value> ErasedVariable for Var<T> {
    fn set_var_stabilise_end(&self) {
        let v_opt = self.value_set_during_stabilisation.borrow_mut().take();
        // if it's None, then we were simply pushed onto the
        // value_set_during_stabilisation stack twice. So ignore.
        if let Some(v) = v_opt {
            self.set_var_while_not_stabilising(v);
        }
    }
    fn id(&self) -> NodeId {
        self.node_id.get()
    }
    fn break_rc_cycle(&self) {
        self.node.take();
    }
}

pub struct Var<T: Value> {
    pub(crate) state: Weak<State>,
    pub(crate) value: RefCell<T>,
    pub(crate) value_set_during_stabilisation: RefCell<Option<T>>,
    pub(crate) set_at: Cell<StabilisationNum>,
    // mutable for initialisation
    pub(crate) node: RefCell<Option<Rc<Node<VarGenerics<T>>>>>,
    // mutable for initialisation
    pub(crate) node_id: Cell<NodeId>,
}

impl<T: Value> Debug for Var<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Var")
            .field("set_at", &self.set_at.get())
            .field("value", &self.value.borrow())
            .finish()
    }
}

impl<T: Value> Var<T> {
    pub(crate) fn erased(self: &Rc<Self>) -> WeakVar {
        Rc::downgrade(self) as WeakVar
    }

    pub(crate) fn get(&self) -> T {
        self.value.borrow().clone()
    }

    pub(crate) fn update(self: &Rc<Self>, f: impl FnOnce(T) -> T)
    where
        T: Default,
    {
        let t = self.state.upgrade().unwrap();
        match t.status.get() {
            IncrStatus::NotStabilising | IncrStatus::RunningOnUpdateHandlers => {
                {
                    let mut value = self.value.borrow_mut();
                    // T: Default. So we can save a clone by writing e.g. an empty vec or map in there.
                    let taken = std::mem::take(&mut *value);
                    *value = f(taken);
                }
                self.did_set_var_while_not_stabilising();
            }
            IncrStatus::Stabilising => {
                let mut delayed_slot = self.value_set_during_stabilisation.borrow_mut();
                if let Some(delayed) = &mut *delayed_slot {
                    // T: Default. So we can save a clone by writing e.g. an empty vec or map in there.
                    let taken = std::mem::take(delayed);
                    *delayed = f(taken);
                } else {
                    let mut stack = t.set_during_stabilisation.borrow_mut();
                    stack.push(self.erased());
                    // we have to clone, because we don't want to mem::take the value
                    // that some nodes might still need to read during this stabilisation.
                    let cloned = (*self.value.borrow()).clone();
                    delayed_slot.replace(f(cloned));
                }
            }
        };
    }

    pub(crate) fn replace_with(self: &Rc<Self>, f: impl FnOnce(&mut T) -> T) -> T {
        let t = self.state.upgrade().unwrap();
        match t.status.get() {
            IncrStatus::NotStabilising | IncrStatus::RunningOnUpdateHandlers => {
                let old = {
                    let v = &mut *self.value.borrow_mut();
                    let new = f(v);
                    std::mem::replace(v, new)
                };
                self.did_set_var_while_not_stabilising();
                old
            }
            IncrStatus::Stabilising => {
                let mut v = self.value_set_during_stabilisation.borrow_mut();
                if let Some(v) = &mut *v {
                    let new = f(v);
                    std::mem::replace(v, new)
                } else {
                    let mut stack = t.set_during_stabilisation.borrow_mut();
                    stack.push(self.erased());
                    let mut cloned = (*self.value.borrow()).clone();
                    let new = f(&mut cloned);
                    std::mem::replace(&mut cloned, new)
                }
            }
        }
    }

    pub(crate) fn modify(self: &Rc<Self>, f: impl FnOnce(&mut T)) {
        let t = self.state.upgrade().unwrap();
        match t.status.get() {
            IncrStatus::NotStabilising | IncrStatus::RunningOnUpdateHandlers => {
                {
                    let mut v = self.value.borrow_mut();
                    f(&mut v);
                }
                self.did_set_var_while_not_stabilising();
            }
            IncrStatus::Stabilising => {
                let mut v = self.value_set_during_stabilisation.borrow_mut();
                if let Some(v) = &mut *v {
                    f(v);
                } else {
                    let mut stack = t.set_during_stabilisation.borrow_mut();
                    stack.push(self.erased());
                    let mut cloned = (*self.value.borrow()).clone();
                    f(&mut cloned);
                    v.replace(cloned);
                }
            }
        };
    }

    pub(crate) fn set(self: &Rc<Self>, value: T) {
        let t = self.state.upgrade().unwrap();
        match t.status.get() {
            IncrStatus::RunningOnUpdateHandlers | IncrStatus::NotStabilising => {
                self.set_var_while_not_stabilising(value);
            }
            IncrStatus::Stabilising => {
                let mut v = self.value_set_during_stabilisation.borrow_mut();
                if v.is_none() {
                    let mut stack = t.set_during_stabilisation.borrow_mut();
                    stack.push(self.erased());
                }
                *v = Some(value);
            }
        }
    }

    fn set_var_while_not_stabilising(&self, value: T) {
        {
            let mut value_slot = self.value.borrow_mut();
            *value_slot = value;
        }
        self.did_set_var_while_not_stabilising();
    }

    fn did_set_var_while_not_stabilising(&self) {
        let Some(watch) = self.node.borrow().clone() else {
            panic!("uninitialised var or abandoned watch node (had {:?})", self.node_id)
        };
        let t = self.state.upgrade().unwrap();
        t.num_var_sets.increment();
        if self.set_at.get() < t.stabilisation_num.get() {
            tracing::info!(
                "variable set at t={:?}, current revision is t={:?}",
                self.set_at.get().0,
                t.stabilisation_num.get().0
            );
            self.set_at.set(t.stabilisation_num.get());
            debug_assert!(watch.is_stale());
            if watch.is_necessary() && !watch.is_in_recompute_heap() {
                tracing::info!(
                    "inserting var watch into recompute heap at height {:?}",
                    watch.height()
                );
                t.recompute_heap.insert(watch.packed());
            }
        }
    }

    pub(crate) fn watch(&self) -> Incr<T> {
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
thread_local! {
    static DID_DROP: Cell<u32> = Cell::new(0);
}

#[cfg(test)]
impl<T: Value> Drop for Var<T> {
    fn drop(&mut self) {
        tracing::trace!("Dropping var with id {:?}", self.node_id);
        DID_DROP.with(|cell| cell.set(cell.get() + 1));
    }
}

#[test]
fn var_drop() {
    DID_DROP.with(|cell| cell.set(0));
    {
        let incr = crate::IncrState::new();
        println!("before var created");
        let v = incr.var(10);
        println!("before watch created");
        let w = v.watch();
        drop(v);
        println!("watch created, public::Var dropped");
        let o = w.observe();
        incr.stabilise();
        assert_eq!(o.value(), Ok(10));
    }
    assert_eq!(DID_DROP.with(|cell| cell.get()), 1);
}

#[test]
fn var_drop_delayed() {
    DID_DROP.with(|cell| cell.set(0));
    {
        let incr = crate::IncrState::new();
        let v = incr.var(10);
        let w = v.watch();
        let c = incr.constant(9).bind(move |x| {
            v.set(99);
            w.clone()
        });
        let o = c.observe();
        incr.stabilise();
        assert_eq!(o.value(), Ok(10));
        incr.stabilise();
        assert_eq!(o.value(), Ok(99));
    }
    assert_eq!(DID_DROP.with(|cell| cell.get()), 1);
}

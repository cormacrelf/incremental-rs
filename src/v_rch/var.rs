use crate::Value;
#[cfg(test)]
use test_log::test;

use super::kind::NodeGenerics;
use super::node::{ErasedNode, Incremental, Node, NodeId};
use super::stabilisation_num::StabilisationNum;
use super::state::IncrStatus;
use super::state::State;
use super::Incr;
use core::fmt::Debug;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

pub(crate) struct VarGenerics<'a, T: Value<'a>>(std::marker::PhantomData<&'a T>);
impl<'a, R: Value<'a>> NodeGenerics<'a> for VarGenerics<'a, R> {
    type R = R;
    type BindLhs = ();
    type BindRhs = ();
    type I1 = ();
    type I2 = ();
    type F1 = fn(Self::I1) -> R;
    type F2 = fn(Self::I1, Self::I2) -> R;
    type B1 = fn(Self::BindLhs) -> Incr<'a, Self::BindRhs>;
    type Fold = fn(Self::R, Self::I1) -> Self::R;
    type Update = fn(Self::R, Self::I1, Self::I1) -> Self::R;
}

// For the delayed variable set list (set_during_stabilisation).
// We use Weak to ensure we don't interfere with the manual
// Rc-cycle-breaking on public::Var.
pub(crate) type WeakVar<'a> = Weak<dyn ErasedVariable<'a> + 'a>;

pub(crate) trait ErasedVariable<'a>: Debug {
    fn set_var_stabilise_end(&self);
    fn id(&self) -> NodeId;
    fn break_rc_cycle(&self);
}

impl<'a, T: Value<'a>> ErasedVariable<'a> for Var<'a, T> {
    fn set_var_stabilise_end(&self) {
        let v_opt = self.value_set_during_stabilisation.borrow_mut().take();
        // if it's None, then we were simply pushed onto the
        // value_set_during_stabilisation stack twice. So ignore.
        if let Some(v) = v_opt {
            self.set_var_while_not_stabilising(v);
        }
    }
    fn id(&self) -> NodeId {
        self.node_id
    }
    fn break_rc_cycle(&self) {
        self.node.take();
    }
}

pub struct Var<'a, T: Value<'a>> {
    pub(crate) state: Weak<State<'a>>,
    pub(crate) value: RefCell<T>,
    pub(crate) value_set_during_stabilisation: RefCell<Option<T>>,
    pub(crate) set_at: Cell<StabilisationNum>,
    pub(crate) node: RefCell<Option<Rc<Node<'a, VarGenerics<'a, T>>>>>,
    pub(crate) node_id: NodeId,
}

impl<'a, T: Value<'a>> Debug for Var<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Var")
            .field("set_at", &self.set_at.get())
            .field("value", &self.value.borrow())
            .finish()
    }
}

impl<'a, T: Value<'a>> Var<'a, T> {
    pub(crate) fn erased(self: &Rc<Self>) -> WeakVar<'a> {
        Rc::downgrade(self) as WeakVar<'a>
    }

    pub(crate) fn get(&self) -> T {
        self.value.borrow().clone()
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
        let Some(watch) = self.node.borrow().clone() else {
            panic!("uninitialised var or abandoned watch node (had {:?})", self.node_id)
        };
        let t = self.state.upgrade().unwrap();
        t.num_var_sets.set(t.num_var_sets.get() + 1);
        let mut value_slot = self.value.borrow_mut();
        *value_slot = value;
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
                let mut heap = t.recompute_heap.borrow_mut();
                heap.insert(watch.packed());
            }
        }
    }

    pub(crate) fn watch(&self) -> Incr<'a, T> {
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
impl<'a, T: Value<'a>> Drop for Var<'a, T> {
    fn drop(&mut self) {
        tracing::info!("Dropping var with id {:?}", self.node_id);
        DID_DROP.with(|cell| cell.set(cell.get() + 1));
    }
}

#[test]
fn var_drop() {
    DID_DROP.with(|cell| cell.set(0));
    {
        let incr = State::new();
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
        let incr = State::new();
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

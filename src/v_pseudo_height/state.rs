use crate::v_pseudo_height::node::PackedNode;

use super::internal_observer::{InternalObserver, Observer, WeakObserver};
use super::node::{Kind, Node, Scope};
use super::var::Var;
use super::Incr;
use super::{recompute_heap::RecomputeHeap, stabilisation_num::StabilisationNum};
use core::fmt::Debug;
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::rc::{Rc, Weak};

#[derive(Debug)]
pub struct State {
    pub(crate) stabilisation_num: Cell<StabilisationNum>,
    pub(crate) recompute_heap: RefCell<RecomputeHeap>,
    pub(crate) status: Cell<IncrStatus>,
    pub(crate) num_var_sets: Cell<usize>,
    pub(crate) num_nodes_recomputed: Cell<usize>,
    pub(crate) num_nodes_changed: Cell<usize>,
    pub(crate) num_nodes_became_necessary: Cell<usize>,
    pub(crate) new_observers: Rc<RefCell<Vec<WeakObserver>>>,
    pub(crate) all_observers: Rc<RefCell<Vec<WeakObserver>>>,
    pub(crate) current_scope: RefCell<Scope>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum IncrStatus {
    RunningOnUpdateHandlers,
    NotStabilising,
    Stabilising,
    StabilisePreviouslyRaised,
}

impl State {
    pub(crate) fn current_scope(&self) -> Scope {
        self.current_scope.borrow().clone()
    }
    pub fn new() -> Rc<Self> {
        Rc::new(State {
            recompute_heap: RefCell::new(RecomputeHeap::new()),
            stabilisation_num: Cell::new(StabilisationNum(0)),
            num_var_sets: Cell::new(0),
            num_nodes_recomputed: Cell::new(0),
            num_nodes_changed: Cell::new(0),
            num_nodes_became_necessary: Cell::new(0),
            status: Cell::new(IncrStatus::NotStabilising),
            new_observers: Rc::new(RefCell::new(Vec::new())),
            all_observers: Rc::new(RefCell::new(Vec::new())),
            current_scope: RefCell::new(Scope::Top),
        })
    }

    pub fn var<T: Debug + Clone + 'static>(self: &Rc<Self>, value: T) -> Rc<Var<T>> {
        let node = Node::<super::var::VarGenerics<T>>::create(
            self.clone(),
            Scope::Top,
            Kind::Var(Weak::new()),
        );
        let var = Rc::new(Var {
            state: self.clone(),
            set_at: Cell::new(self.stabilisation_num.get()),
            value: RefCell::new(value),
            node: Rc::new(node),
        });
        {
            let mut kind = var.node.kind.borrow_mut();
            *kind = Kind::Var(Rc::downgrade(&var));
        }
        var
    }

    pub fn observe<T: Debug + Clone + 'static>(&self, incr: Incr<T>) -> Rc<InternalObserver<T>> {
        let state = incr.node.state();
        let internal_observer = InternalObserver::new(incr);
        let mut no = state.new_observers.borrow_mut();
        let rc: Rc<InternalObserver<T>> = Rc::new(internal_observer);
        no.push(Rc::downgrade(&rc) as Weak<dyn Observer>);
        rc
    }

    // pub fn set_var<T: Debug + Clone + 'static>(&self, var: &mut Var<T>, value: T) {
    //     match self.status.get() {
    //         IncrStatus::NotStabilising => {
    //             self.num_var_sets.set(self.num_var_sets.get() + 1);
    //             let mut value_slot = var.value.borrow_mut();
    //             *value_slot = value;
    //             debug_assert!(var.node.is_stale());
    //             if var.set_at.get().less_than(&self.stabilisation_num.get()) {
    //                 var.set_at.set(self.stabilisation_num.get());
    //                 let watch = var.node.clone();
    //                 debug_assert!(watch.is_stale());
    //                 if watch.is_necessary() && watch.is_in_recompute_heap() {
    //                     let mut heap = self.recompute_heap.borrow_mut();
    //                     heap.insert(watch.weak());
    //                 }
    //             }
    //         }
    //         _ => todo!("setting variable while stabilising..."),
    //     }
    // }

    fn add_new_observers(&self) {
        let mut no = self.new_observers.borrow_mut();
        println!("add_new_observers: {:?}", no);
        let mut ao = self.all_observers.borrow_mut();
        for weak in no.drain(..) {
            let Some(obs) = weak.upgrade() else { continue };
            use super::internal_observer::State as ObState;
            match obs.state().get() {
                ObState::InUse | ObState::Disallowed => panic!(),
                ObState::Unlinked => {}
                ObState::Created => {
                    obs.state().set(ObState::InUse);
                    let src: PackedNode = obs.observing();
                    let was_necessary = src.is_necessary();
                    println!("observed was necessary?: {:?}", was_necessary);
                    {
                        let inner = src.inner();
                        let mut i = inner.borrow_mut();
                        i.observers.push(weak.clone());
                    }
                    ao.push(weak);
                    debug_assert!(src.is_necessary());
                    if !was_necessary {
                        src.became_necessary();
                    }
                }
            }
        }
    }
    fn remove_disallowed_observers(&self) {}
    fn stabilise_start(&self) {
        println!("stabilise_start");
        self.status.set(IncrStatus::Stabilising);
        self.add_new_observers();
        self.remove_disallowed_observers();
    }
    fn stabilise_end(&self) {
        self.stabilisation_num
            .set(self.stabilisation_num.get().add1());
        // while not (Stack.is_empty t.set_during_stabilization) do
        //   let (T var) = Stack.pop_exn t.set_during_stabilization in
        //   let value = Uopt.value_exn var.value_set_during_stabilization in
        //   var.value_set_during_stabilization <- Uopt.none;
        //   set_var_while_not_stabilizing var value
        // done;
        // while not (Stack.is_empty t.handle_after_stabilization) do
        self.status.set(IncrStatus::RunningOnUpdateHandlers);
        self.status.set(IncrStatus::NotStabilising);
    }
    pub fn stabilise(&self) {
        assert_eq!(self.status.get(), IncrStatus::NotStabilising);
        println!("stabilise");
        let mut stdout = std::io::stdout();
        stdout.flush().unwrap();
        self.stabilise_start();
        let mut rch = self.recompute_heap.borrow_mut();
        while let Some(min) = rch.remove_min() {
            if let Some(node) = min.upgrade() {
                node.recompute();
            }
        }
        self.stabilise_end();
    }
}

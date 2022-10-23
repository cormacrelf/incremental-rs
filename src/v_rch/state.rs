use super::adjust_heights_heap::AdjustHeightsHeap;
use super::array_fold::ArrayFold;
use super::symmetric_fold::BTreeMapSymmetricFold;
use super::unordered_fold::UnorderedArrayFold;
use super::{NodeRef, Value};

use super::internal_observer::{ErasedObserver, InternalObserver, WeakObserver};
use super::kind::Kind;
use super::node::Node;
use super::scope::Scope;
use super::var::Var;
use super::{public, Incr};
use super::{recompute_heap::RecomputeHeap, stabilisation_num::StabilisationNum};
use core::fmt::Debug;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::Write;
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

#[derive(Debug)]
pub struct State<'a> {
    pub(crate) stabilisation_num: Cell<StabilisationNum>,
    pub(crate) adjust_heights_heap: RefCell<AdjustHeightsHeap<'a>>,
    pub(crate) recompute_heap: RefCell<RecomputeHeap<'a>>,
    pub(crate) status: Cell<IncrStatus>,
    pub(crate) num_var_sets: Cell<usize>,
    pub(crate) num_nodes_recomputed: Cell<usize>,
    pub(crate) num_nodes_changed: Cell<usize>,
    pub(crate) num_nodes_became_necessary: Cell<usize>,
    pub(crate) num_nodes_became_unnecessary: Cell<usize>,
    pub(crate) num_nodes_invalidated: Cell<usize>,
    pub(crate) new_observers: Rc<RefCell<Vec<WeakObserver<'a>>>>,
    pub(crate) all_observers: Rc<RefCell<Vec<WeakObserver<'a>>>>,
    pub(crate) current_scope: RefCell<Scope<'a>>,
    _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum IncrStatus {
    RunningOnUpdateHandlers,
    NotStabilising,
    Stabilising,
    StabilisePreviouslyRaised,
}

impl<'a> State<'a> {
    pub(crate) fn current_scope(&self) -> Scope<'a> {
        self.current_scope.borrow().clone()
    }
    pub fn new() -> Rc<Self> {
        const DEFAULT_MAX_HEIGHT_ALLOWED: usize = 128;
        Rc::new(State {
            recompute_heap: RefCell::new(RecomputeHeap::new(DEFAULT_MAX_HEIGHT_ALLOWED)),
            adjust_heights_heap: RefCell::new(AdjustHeightsHeap::new(DEFAULT_MAX_HEIGHT_ALLOWED)),
            stabilisation_num: Cell::new(StabilisationNum(0)),
            num_var_sets: Cell::new(0),
            num_nodes_recomputed: Cell::new(0),
            num_nodes_changed: Cell::new(0),
            num_nodes_became_necessary: Cell::new(0),
            num_nodes_became_unnecessary: Cell::new(0),
            num_nodes_invalidated: Cell::new(0),
            status: Cell::new(IncrStatus::NotStabilising),
            new_observers: Rc::new(RefCell::new(Vec::new())),
            all_observers: Rc::new(RefCell::new(Vec::new())),
            current_scope: RefCell::new(Scope::Top),
            _phantom: Default::default(),
        })
    }

    pub fn fold<F, T: Value<'a>, R: Value<'a>>(
        self: &Rc<Self>,
        vec: Vec<Incr<'a, T>>,
        init: R,
        f: F,
    ) -> Incr<'a, R>
    where
        F: FnMut(R, T) -> R + 'a,
    {
        let node = Node::<ArrayFold<'a, F, T, R>>::create(
            self.clone(),
            self.current_scope(),
            Kind::ArrayFold(ArrayFold {
                init,
                fold: f.into(),
                children: vec,
            }),
        );
        Incr { node }
    }

    pub fn unordered_fold<F, U, T: Value<'a>, R: Value<'a>>(
        self: &Rc<Self>,
        vec: Vec<Incr<'a, T>>,
        init: R,
        f: F,
        update: U,
    ) -> Incr<'a, R>
    where
        F: FnMut(R, T) -> R + 'a,
        U: FnMut(R, T, T) -> R + 'a,
    {
        let node = Node::<UnorderedArrayFold<'a, F, U, T, R>>::create(
            self.clone(),
            self.current_scope(),
            Kind::ArrayFold(ArrayFold {
                init,
                fold: f.into(),
                children: vec,
            }),
        );
        Incr { node }
    }

    pub fn btreemap_unordered_fold<FAdd, FRemove, K, V, R>(
        self: &Rc<Self>,
        map: Incr<'a, BTreeMap<K, V>>,
        init: R,
        mut add: FAdd,
        mut remove: FRemove,
        revert_to_init_when_empty: bool,
    ) -> Incr<'a, R>
    where
        K: Value<'a> + Ord,
        V: Value<'a> + Eq,
        R: Value<'a>,
        FAdd: FnMut(R, &K, &V) -> R + 'a,
        FRemove: FnMut(R, &K, &V) -> R + 'a,
    {
        map.with_old(move |old, new_in| match old {
            None => new_in
                .iter()
                .fold(init.clone(), |acc, (k, v)| add(acc, k, v)),
            Some((old_in, old_out)) => {
                if revert_to_init_when_empty && new_in.is_empty() {
                    return init.clone();
                }
                old_in.symmetric_fold(&new_in, old_out, &mut add, &mut remove)
            }
        })
    }

    pub fn var<T: Value<'a>>(self: &Rc<Self>, value: T) -> public::Var<'a, T> {
        let node = Node::<super::var::VarGenerics<T>>::create(
            self.clone(),
            self.current_scope(),
            Kind::Uninitialised,
        );
        let var = Rc::new(Var {
            state: self.clone(),
            set_at: Cell::new(self.stabilisation_num.get()),
            value: RefCell::new(value),
            node: RefCell::new(Some(node)),
        });
        {
            let node = var.node.borrow();

            let n = node.as_ref().expect("we just created var's node as Some");
            let mut kind = n.kind.borrow_mut();
            *kind = Kind::Var(var.clone());
        }
        public::Var::new(var)
    }

    pub(crate) fn observe<T: Value<'a>>(&self, incr: Incr<'a, T>) -> Rc<InternalObserver<'a, T>> {
        let state = incr.node.state();
        let internal_observer = InternalObserver::new(incr);
        let mut no = state.new_observers.borrow_mut();
        let rc = Rc::new(internal_observer);
        no.push(Rc::downgrade(&rc) as Weak<dyn ErasedObserver>);
        rc
    }

    // pub fn set_var<T: Debug + Clone + 'a>(&self, var: &mut Var<T>, value: T) {
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
        let mut ao = self.all_observers.borrow_mut();
        for weak in no.drain(..) {
            let Some(obs) = weak.upgrade() else { continue };
            use super::internal_observer::State as ObState;
            match obs.state().get() {
                ObState::InUse | ObState::Disallowed => panic!(),
                ObState::Unlinked => {}
                ObState::Created => {
                    obs.state().set(ObState::InUse);
                    let src: NodeRef<'a> = obs.observing();
                    let was_necessary = src.is_necessary();
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
        while let Some(min) = {
            // we need to access rch in recompute() > maybe_change_value()
            // so fine grained access here
            let mut rch = self.recompute_heap.borrow_mut();
            rch.remove_min()
        } {
            min.recompute();
        }
        self.stabilise_end();
    }
    pub(crate) fn propagate_invalidity(&self) {
        // todo!();
        eprintln!("WARNING: propagate_invalidity not implemented");
    }
}

use super::adjust_heights_heap::AdjustHeightsHeap;
use super::array_fold::ArrayFold;
use super::symmetric_fold::{DiffElement, SymmetricDiffMap, SymmetricDiffMapOwned};
use super::unordered_fold::UnorderedArrayFold;
use super::{NodeRef, Value};

use super::internal_observer::{ErasedObserver, InternalObserver, WeakObserver};
use super::kind::{Constant, Kind};
use super::node::Node;
use super::scope::Scope;
use super::var::{ErasedVar, Var};
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
    pub(crate) set_during_stabilisation: RefCell<Vec<ErasedVar<'a>>>,
    _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum IncrStatus {
    RunningOnUpdateHandlers,
    NotStabilising,
    Stabilising,
    // This is a bit like Mutex panic poisoning. We don't currently need it because
    // we don't use any std::panic::catch_unwind.
    // StabilisePreviouslyRaised,
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
            set_during_stabilisation: Default::default(),
            _phantom: Default::default(),
        })
    }

    pub fn constant<T: Value<'a>>(self: &Rc<Self>, value: T) -> Incr<'a, T> {
        let node = Node::<Constant<'a, T>>::create(
            self.clone(),
            self.current_scope(),
            Kind::Constant(value),
        );
        Incr { node }
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

    pub fn unordered_fold_inverse<F, FInv, T: Value<'a>, R: Value<'a>>(
        self: &Rc<Self>,
        vec: Vec<Incr<'a, T>>,
        init: R,
        f: F,
        f_inverse: FInv,
        full_compute_every_n_changes: Option<u32>,
    ) -> Incr<'a, R>
    where
        F: FnMut(R, T) -> R + Clone + 'a,
        FInv: FnMut(R, T) -> R + 'a,
    {
        let update = super::unordered_fold::make_update_fn_from_inverse(f.clone(), f_inverse);
        UnorderedArrayFold::create_node(self, vec, init, f, update, full_compute_every_n_changes)
    }

    pub fn unordered_fold<F, U, T: Value<'a>, R: Value<'a>>(
        self: &Rc<Self>,
        vec: Vec<Incr<'a, T>>,
        init: R,
        f: F,
        update: U,
        full_compute_every_n_changes: Option<u32>,
    ) -> Incr<'a, R>
    where
        F: FnMut(R, T) -> R + 'a,
        U: FnMut(R, T, T) -> R + 'a,
    {
        UnorderedArrayFold::create_node(self, vec, init, f, update, full_compute_every_n_changes)
    }

    pub fn btreemap_filter_mapi<F, K, V, V2>(
        self: &Rc<Self>,
        map: Incr<'a, BTreeMap<K, V>>,
        mut f: F,
    ) -> Incr<'a, BTreeMap<K, V2>>
    where
        K: Value<'a> + Ord,
        V: Value<'a> + Eq,
        V2: Value<'a>,
        F: FnMut(&K, &V) -> Option<V2> + 'a,
    {
        map.with_old_borrowed(move |old, input| {
            match (old, input.len()) {
                (_, 0) | (None, _) => input
                    .iter()
                    .filter_map(|(k, v)| f(k, v).map(|v2| (k.clone(), v2)))
                    .collect(),
                (Some((old_in, mut old_out)), _) => {
                    let _: &mut BTreeMap<K, V2> =
                        old_in
                            .symmetric_diff(input)
                            .fold(&mut old_out, |out, change| {
                                match change {
                                    DiffElement::Left((key, _)) => {
                                        out.remove(&key);
                                    }
                                    DiffElement::Right((key, newval)) => match f(key, newval) {
                                        None => {
                                            out.remove(&key);
                                        }
                                        Some(v) => {
                                            out.insert(key.clone(), v);
                                        }
                                    },
                                    DiffElement::Unequal((k1, _), (k2, newval)) => {
                                        // we get two keys this time. Avoid the clone.
                                        match f(k1, newval) {
                                            None => {
                                                out.remove(k2);
                                            }
                                            Some(v) => {
                                                out.insert(k2.clone(), v);
                                            }
                                        }
                                    }
                                }
                                out
                            });
                    old_out
                }
            }
        })
    }

    pub fn btreemap_mapi<F, K, V, V2>(
        self: &Rc<Self>,
        map: Incr<'a, BTreeMap<K, V>>,
        mut f: F,
    ) -> Incr<'a, BTreeMap<K, V2>>
    where
        K: Value<'a> + Ord,
        V: Value<'a> + Eq,
        V2: Value<'a>,
        F: FnMut(K, V) -> V2 + 'a,
    {
        map.with_old(move |old, input| match (old, input.len()) {
            (_, 0) | (None, _) => input
                .into_iter()
                .map(|(k, v)| (k.clone(), f(k, v)))
                .collect(),
            (Some((old_in, mut old_out)), _) => {
                let _: &mut BTreeMap<K, V2> =
                    old_in
                        .symmetric_diff_owned(input)
                        .fold(&mut old_out, |out, change| {
                            match change {
                                DiffElement::Left((key, _)) => {
                                    out.remove(&key);
                                }
                                DiffElement::Right((key, newval)) => {
                                    out.insert(key.clone(), f(key, newval));
                                }
                                DiffElement::Unequal((k1, _), (k2, newval)) => {
                                    out.insert(k1, f(k2, newval));
                                }
                            }
                            out
                        });
                old_out
            }
        })
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
        FAdd: FnMut(R, K, V) -> R + 'a,
        FRemove: FnMut(R, K, V) -> R + 'a,
    {
        map.with_old(move |old, new_in| match old {
            None => new_in
                .into_iter()
                .fold(init.clone(), |acc, (k, v)| add(acc, k, v)),
            Some((old_in, old_out)) => {
                if revert_to_init_when_empty && new_in.is_empty() {
                    return init.clone();
                }
                old_in.symmetric_fold_owned(new_in, old_out, &mut add, &mut remove)
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
            node_id: node.id,
            node: RefCell::new(Some(node)),
            value_set_during_stabilisation: RefCell::new(None),
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
        let mut stack = self.set_during_stabilisation.borrow_mut();
        while let Some(var) = stack.pop() {
            var.set_var_stabilise_end();
        }
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

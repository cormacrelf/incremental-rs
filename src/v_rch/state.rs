use super::adjust_heights_heap::AdjustHeightsHeap;
use super::array_fold::ArrayFold;
use super::symmetric_fold::{DiffElement, SymmetricDiffMap, SymmetricDiffMapOwned};
use super::unordered_fold::UnorderedArrayFold;
use super::{NodeRef, Value, WeakNode};

use super::internal_observer::{ErasedObserver, InternalObserver, ObserverId, ObserverState, StrongObserver, WeakObserver};
use super::kind::{Constant, Kind};
use super::node::Node;
use super::scope::Scope;
use super::var::{WeakVar, Var};
use super::{public, Incr};
use super::{recompute_heap::RecomputeHeap, stabilisation_num::StabilisationNum};
use core::fmt::Debug;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
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
    pub(crate) num_active_observers: Cell<usize>,
    pub(crate) propagate_invalidity: RefCell<Vec<WeakNode<'a>>>,
    pub(crate) new_observers: RefCell<Vec<WeakObserver<'a>>>,
    // use a strong reference, because the InternalObserver for a dropped public::Observer
    // should stay alive until we've cleaned up after it.
    pub(crate) all_observers: RefCell<HashMap<ObserverId, StrongObserver<'a>>>,
    pub(crate) disallowed_observers: RefCell<Vec<WeakObserver<'a>>>,
    pub(crate) current_scope: RefCell<Scope<'a>>,
    pub(crate) set_during_stabilisation: RefCell<Vec<WeakVar<'a>>>,
    pub(crate) dead_vars: RefCell<Vec<WeakVar<'a>>>,
    _phantom: PhantomData<&'a ()>,

    #[cfg(debug_assertions)]
    pub(crate) only_in_debug: OnlyInDebug<'a>,
}

#[cfg(debug_assertions)]
#[derive(Debug, Default)]
pub(crate) struct OnlyInDebug<'a> {
    currently_running_node: RefCell<Option<WeakNode<'a>>>,
}

#[cfg(debug_assertions)]
impl<'a> OnlyInDebug<'a> {
    pub(crate) fn currently_running_node_exn(&self, name: &'static str) -> WeakNode<'a> {
        let crn = self.currently_running_node.borrow();
        match &*crn {
            None => panic!("can only call {} during stabilisation", name),
            Some(current) => current.clone()
        }
    }
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
    pub(crate) fn weak(self: &Rc<Self>) -> Weak<Self> {
        Rc::downgrade(self)
    }
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
            num_active_observers: Cell::new(0),
            propagate_invalidity: RefCell::new(vec![]),
            status: Cell::new(IncrStatus::NotStabilising),
            all_observers: RefCell::new(HashMap::new()),
            new_observers: RefCell::new(Vec::new()),
            disallowed_observers: RefCell::new(Vec::new()),
            current_scope: RefCell::new(Scope::Top),
            set_during_stabilisation: RefCell::new(vec![]),
            dead_vars: RefCell::new(vec![]),
            _phantom: Default::default(),
            #[cfg(debug_assertions)]
            only_in_debug: OnlyInDebug::default(),
        })
    }

    pub fn constant<T: Value<'a>>(self: &Rc<Self>, value: T) -> Incr<'a, T> {
        let node = Node::<Constant<'a, T>>::create(
            self.weak(),
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
            self.weak(),
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
            self.weak(),
            self.current_scope(),
            Kind::Uninitialised,
        );
        let var = Rc::new(Var {
            state: self.weak(),
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
        let internal_observer = InternalObserver::new(incr);
        self.num_active_observers.set(self.num_active_observers.get() + 1);
        let mut no = self.new_observers.borrow_mut();
        no.push(Rc::downgrade(&internal_observer) as Weak<dyn ErasedObserver>);
        internal_observer
    }

    fn add_new_observers(&self) {
        let mut no = self.new_observers.borrow_mut();
        let mut ao = self.all_observers.borrow_mut();
        for weak in no.drain(..) {
            let Some(obs) = weak.upgrade() else { continue };
            match obs.state().get() {
                ObserverState::InUse | ObserverState::Disallowed => panic!(),
                ObserverState::Unlinked => {}
                ObserverState::Created => {
                    obs.state().set(ObserverState::InUse);
                    let src: NodeRef<'a> = obs.observing();
                    let was_necessary = src.is_necessary();
                    {
                        let inner = src.inner();
                        let mut i = inner.borrow_mut();
                        i.observers.insert(obs.id(), weak.clone());
                    }
                    ao.insert(obs.id(), obs);
                    debug_assert!(src.is_necessary());
                    if !was_necessary {
                        src.became_necessary_propagate();
                    }
                }
            }
        }
    }

    // not required. We don't have a GC with dead-but-still-participating objects to account for.
    // fn disallow_finalized_observers(&self) {}

    fn unlink_disallowed_observers(&self) {
        let mut disallowed = self.disallowed_observers.borrow_mut();
        for obs_weak in disallowed.drain(..) {
            let Some(obs) = obs_weak.upgrade() else { continue };
            debug_assert_eq!(obs.state().get(), ObserverState::Disallowed);
            obs.state().set(ObserverState::Unlinked);
            // get a strong ref to the node, before we drop its owning InternalObserver
            let observing = obs.observing();
            {
                // remove from the observed node's list of observers
                let inner = observing.inner();
                let mut i = inner.borrow_mut();
                i.observers.remove(&obs.id());

                // remove from all_observers (this finally drops the InternalObserver)
                let mut ao = self.all_observers.borrow_mut();
                ao.remove(&obs.id());
            }
            observing.check_if_unnecessary();
        }
    }

    fn stabilise_start(&self) {
        self.status.set(IncrStatus::Stabilising);
        // self.disallow_finalized_observers();
        self.add_new_observers();
        self.unlink_disallowed_observers();
    }
    fn stabilise_end(&self) {
        self.stabilisation_num
            .set(self.stabilisation_num.get().add1());
        let mut stack = self.set_during_stabilisation.borrow_mut();
        while let Some(var) = stack.pop() {
            let Some(var) = var.upgrade() else {
                continue
            };
            println!("set_during_stabilisation: found var with {:?}", var.id());
            var.set_var_stabilise_end();
        }
        drop(stack);
        // we may have the same var appear in the set_during_stabilisation stack,
        // and also the dead_vars stack. That's ok! Being in dead_vars means it will
        // never be set again, as the public::Var is gone & nobody can set it from
        // outside any more. So killing the Var's internal reference to the watch Node
        // will not be a problem, because the last time that was needed was back a few
        // lines ago when we ran var.set_var_stabilise_end().
        let mut stack = self.dead_vars.borrow_mut();
        for var in stack.drain(..) {
            let Some(var) = var.upgrade() else {
                continue
            };
            println!("---------------------- dead_vars: found var with {:?}", var.id());
            var.break_rc_cycle();
        }
        drop(stack);
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
        while let Some(node) = {
            let mut pi = self.propagate_invalidity.borrow_mut();
            pi.pop()
        } {
            let Some(node) = node.upgrade() else { continue };
            if node.is_valid() {
                if node.should_be_invalidated() {
                    node.invalidate_node();
                } else {
                    /* [Node.needs_to_be_computed node] is true because
                       - node is necessary. This is because children can only point to necessary parents
                       - node is stale. This is because: For bind, if, join, this is true because
                       - either the invalidation is caused by the lhs changing (in which case the
                         lhs-change node being newer makes us stale).
                       - or a child became invalid this stabilization cycle, in which case it has
                         t.changed_at of [t.stabilization_num], and so [node] is stale
                       - or [node] just became necessary and tried connecting to an already invalid
                       child. In that case, [child.changed_at > node.recomputed_at] for that child,
                       because if we had been recomputed when that child changed, we would have been
                       made invalid back then.  For expert nodes, the argument is the same, except
                       that instead of lhs-change nodes make the expert nodes stale, it's made stale
                       explicitely when adding or removing children. */
                    debug_assert!(node.needs_to_be_computed());

                    // ...
                    node.propagate_invalidity_helper();

                    /* We do not check [Node.needs_to_be_computed node] here, because it should be
                       true, and because computing it takes O(number of children), node can be pushed
                       on the stack once per child, and expert nodes can have lots of children. */
                    if !node.is_in_recompute_heap() {
                        let mut rch = self.recompute_heap.borrow_mut();
                        rch.insert(node);
                    }
                }
            }
        }
    }
}

impl<'a> Drop for State<'a> {
    fn drop(&mut self) {
        let dead_vars = self.dead_vars.get_mut();
        for var in dead_vars.drain(..).filter_map(|x| x.upgrade()) {
            var.break_rc_cycle();
        }
    }
}

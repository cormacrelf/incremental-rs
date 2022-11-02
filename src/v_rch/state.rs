use super::adjust_heights_heap::AdjustHeightsHeap;
use super::array_fold::ArrayFold;
use super::node_update::NodeUpdateDelayed;
use super::unordered_fold::UnorderedArrayFold;
use super::{NodeRef, Value, WeakNode};
use crate::SubscriptionToken;

use super::internal_observer::{
    ErasedObserver, InternalObserver, ObserverId, ObserverState, StrongObserver, WeakObserver,
};
use super::kind::{Constant, Kind};
use super::node::Node;
use super::scope::Scope;
use super::var::{Var, WeakVar};
use super::{public, Incr};
use super::stabilisation_num::StabilisationNum;
use super::recompute_heap::RecomputeHeap;
use core::fmt::Debug;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::Write;
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

#[derive(Debug)]
pub(crate) struct State<'a> {
    pub(crate) stabilisation_num: Cell<StabilisationNum>,
    pub(crate) adjust_heights_heap: RefCell<AdjustHeightsHeap<'a>>,
    pub(crate) recompute_heap: RecomputeHeap<'a>,
    pub(crate) status: Cell<IncrStatus>,
    pub(crate) num_var_sets: Cell<usize>,
    pub(crate) num_nodes_recomputed: Cell<usize>,
    pub(crate) num_nodes_created: Cell<usize>,
    pub(crate) num_nodes_changed: Cell<usize>,
    pub(crate) num_nodes_became_necessary: Cell<usize>,
    pub(crate) num_nodes_became_unnecessary: Cell<usize>,
    pub(crate) num_nodes_invalidated: Cell<usize>,
    pub(crate) num_active_observers: Cell<usize>,
    pub(crate) propagate_invalidity: RefCell<Vec<WeakNode<'a>>>,
    pub(crate) run_on_update_handlers: RefCell<Vec<(WeakNode<'a>, NodeUpdateDelayed)>>,
    pub(crate) handle_after_stabilisation: RefCell<Vec<WeakNode<'a>>>,
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
            Some(current) => current.clone(),
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
    pub(crate) fn public(self: &Rc<Self>) -> public::IncrState<'a> {
        public::IncrState(self.clone())
    }
    pub(crate) fn weak(self: &Rc<Self>) -> Weak<Self> {
        Rc::downgrade(self)
    }
    pub(crate) fn current_scope(&self) -> Scope<'a> {
        self.current_scope.borrow().clone()
    }
    pub(crate) fn new() -> Rc<Self> {
        const DEFAULT_MAX_HEIGHT_ALLOWED: usize = 128;
        Rc::new(State {
            recompute_heap: RecomputeHeap::new(DEFAULT_MAX_HEIGHT_ALLOWED),
            adjust_heights_heap: RefCell::new(AdjustHeightsHeap::new(DEFAULT_MAX_HEIGHT_ALLOWED)),
            stabilisation_num: Cell::new(StabilisationNum(0)),
            num_var_sets: Cell::new(0),
            num_nodes_recomputed: Cell::new(0),
            num_nodes_created: Cell::new(0),
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
            handle_after_stabilisation: RefCell::new(vec![]),
            run_on_update_handlers: RefCell::new(vec![]),
            _phantom: Default::default(),
            #[cfg(debug_assertions)]
            only_in_debug: OnlyInDebug::default(),
        })
    }

    pub(crate) fn constant<T: Value<'a>>(self: &Rc<Self>, value: T) -> Incr<'a, T> {
        let node = Node::<Constant<'a, T>>::create(
            self.weak(),
            self.current_scope(),
            Kind::Constant(value),
        );
        Incr { node }
    }

    pub(crate) fn fold<F, T: Value<'a>, R: Value<'a>>(
        self: &Rc<Self>,
        vec: Vec<Incr<'a, T>>,
        init: R,
        f: F,
    ) -> Incr<'a, R>
    where
        F: FnMut(R, &T) -> R + 'a,
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

    pub(crate) fn unordered_fold_inverse<F, FInv, T: Value<'a>, R: Value<'a>>(
        self: &Rc<Self>,
        vec: Vec<Incr<'a, T>>,
        init: R,
        f: F,
        f_inverse: FInv,
        full_compute_every_n_changes: Option<u32>,
    ) -> Incr<'a, R>
    where
        F: FnMut(R, &T) -> R + Clone + 'a,
        FInv: FnMut(R, &T) -> R + 'a,
    {
        let update = super::unordered_fold::make_update_fn_from_inverse(f.clone(), f_inverse);
        UnorderedArrayFold::create_node(self, vec, init, f, update, full_compute_every_n_changes)
    }

    pub(crate) fn unordered_fold<F, U, T: Value<'a>, R: Value<'a>>(
        self: &Rc<Self>,
        vec: Vec<Incr<'a, T>>,
        init: R,
        f: F,
        update: U,
        full_compute_every_n_changes: Option<u32>,
    ) -> Incr<'a, R>
    where
        F: FnMut(R, &T) -> R + 'a,
        U: FnMut(R, &T, &T) -> R + 'a,
    {
        UnorderedArrayFold::create_node(self, vec, init, f, update, full_compute_every_n_changes)
    }

    pub(crate) fn var_in_scope<T: Value<'a>>(
        self: &Rc<Self>,
        value: T,
        scope: Scope<'a>,
    ) -> public::Var<'a, T> {
        let node =
            Node::<super::var::VarGenerics<T>>::create(self.weak(), scope, Kind::Uninitialised);
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
        self.num_active_observers
            .set(self.num_active_observers.get() + 1);
        let mut no = self.new_observers.borrow_mut();
        no.push(Rc::downgrade(&internal_observer) as Weak<dyn ErasedObserver>);
        internal_observer
    }

    fn add_new_observers(&self) {
        let mut no = self.new_observers.borrow_mut();
        for weak in no.drain(..) {
            let Some(obs) = weak.upgrade() else { continue };
            match obs.state().get() {
                ObserverState::InUse | ObserverState::Disallowed => panic!(),
                ObserverState::Unlinked => {}
                ObserverState::Created => {
                    obs.state().set(ObserverState::InUse);
                    let node = obs.observing();
                    let was_necessary = node.is_necessary();
                    {
                        let mut ao = self.all_observers.borrow_mut();
                        ao.insert(obs.id(), obs.clone());
                    }
                    obs.add_to_observed_node();
                    /* By adding [internal_observer] to [observing.observers], we may have added
                    on-update handlers to [observing].  We need to handle [observing] after this
                    stabilization to give those handlers a chance to run. */
                    node.handle_after_stabilisation();
                    debug_assert!(node.is_necessary());
                    if !was_necessary {
                        node.became_necessary_propagate();
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
                obs.remove_from_observed_node();
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
        tracing::info_span!("set_during_stabilisation").in_scope(|| {
            let mut stack = self.set_during_stabilisation.borrow_mut();
            while let Some(var) = stack.pop() {
                let Some(var) = var.upgrade() else {
                    continue
                };
                tracing::debug!("set_during_stabilisation: found var with {:?}", var.id());
                var.set_var_stabilise_end();
            }
        });
        // we may have the same var appear in the set_during_stabilisation stack,
        // and also the dead_vars stack. That's ok! Being in dead_vars means it will
        // never be set again, as the public::Var is gone & nobody can set it from
        // outside any more. So killing the Var's internal reference to the watch Node
        // will not be a problem, because the last time that was needed was back a few
        // lines ago when we ran var.set_var_stabilise_end().
        tracing::info_span!("dead_vars").in_scope(|| {
            let mut stack = self.dead_vars.borrow_mut();
            for var in stack.drain(..) {
                let Some(var) = var.upgrade() else {
                    continue
                };
                tracing::debug!("dead_vars: found var with {:?}", var.id());
                var.break_rc_cycle();
            }
        });
        tracing::info_span!("handle_after_stabilisation").in_scope(|| {
            let mut stack = self.handle_after_stabilisation.borrow_mut();
            for node in stack.drain(..).filter_map(|node| node.upgrade()) {
                node.is_in_handle_after_stabilisation().set(false);
                let node_update = node.node_update();
                let mut run_queue = self.run_on_update_handlers.borrow_mut();
                run_queue.push((node.weak(), node_update))
            }
        });
        tracing::info_span!("run_on_update_handlers").in_scope(|| {
            self.status.set(IncrStatus::RunningOnUpdateHandlers);
            let now = self.stabilisation_num.get();
            let mut stack = self.run_on_update_handlers.borrow_mut();
            for (node, node_update) in stack
                .drain(..)
                .filter_map(|(node, node_update)| node.upgrade().map(|n| (n, node_update)))
            {
                node.run_on_update_handlers(node_update, now)
            }
        });
        self.status.set(IncrStatus::NotStabilising);
    }

    pub(crate) fn stabilise(&self) {
        let span = tracing::info_span!("stabilise");
        span.in_scope(|| {
            assert_eq!(self.status.get(), IncrStatus::NotStabilising);
            let mut stdout = std::io::stdout();
            stdout.flush().unwrap();
            self.stabilise_start();
            while let Some(min) = {
                // we need to access rch in recompute() > maybe_change_value()
                // so fine grained access here
                self.recompute_heap.remove_min()
            } {
                min.recompute();
            }
            self.stabilise_end();
        });
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
                        self.recompute_heap.insert(node);
                    }
                }
            }
        }
    }

    pub(crate) fn unsubscribe(&self, token: SubscriptionToken) {
        let all_obs = self.all_observers.borrow();
        if let Some(obs) = all_obs.get(&token.observer_id()) {
            obs.unsubscribe(token).unwrap();
        }
    }

    pub(crate) fn set_max_height_allowed(&self, new_max_height: usize) {
        if self.status.get() == IncrStatus::Stabilising {
            panic!("tried to set_max_height_allowed during stabilisation");
        }
        // let mut ah_heap = self.adjust_heights_heap.borrow_mut();
        // ah_heap.set_max_height_allowed(new_max_height);
        // drop(ah_heap);
        // let mut rc_heap = self.recompute_heap.borrow_mut();
        // rc_heap.set_max_height_allowed(new_max_height);
        // drop(rc_heap);
    }

    pub(crate) fn set_height(&self, node: NodeRef<'a>, height: i32) {
        let mut ah_heap = self.adjust_heights_heap.borrow_mut();
        ah_heap.set_height(&node, height);
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

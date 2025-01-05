use super::adjust_heights_heap::AdjustHeightsHeap;
use super::kind;
use super::node_update::NodeUpdateDelayed;
use super::{CellIncrement, NodeRef, Value, WeakNode};
use crate::incrsan::NotObserver;
use crate::{SubscriptionToken, WeakMap};

use super::internal_observer::{
    ErasedObserver, InternalObserver, ObserverId, ObserverState, StrongObserver, WeakObserver,
};
use super::kind::{Constant, Kind};
use super::node::{Node, NodeId};
use super::recompute_heap::RecomputeHeap;
use super::scope::Scope;
use super::stabilisation_num::StabilisationNum;
use super::var::{Var, WeakVar};
use super::{public, Incr};
use core::fmt::Debug;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

pub(crate) mod expert;

pub(crate) struct State {
    pub(crate) stabilisation_num: Cell<StabilisationNum>,
    pub(crate) adjust_heights_heap: RefCell<AdjustHeightsHeap>,
    pub(crate) recompute_heap: RecomputeHeap,
    pub(crate) status: Cell<IncrStatus>,
    pub(crate) num_var_sets: Cell<usize>,
    pub(crate) num_nodes_recomputed: Cell<usize>,
    pub(crate) num_nodes_created: Cell<usize>,
    pub(crate) num_nodes_changed: Cell<usize>,
    pub(crate) num_nodes_became_necessary: Cell<usize>,
    pub(crate) num_nodes_became_unnecessary: Cell<usize>,
    pub(crate) num_nodes_invalidated: Cell<usize>,
    pub(crate) num_active_observers: Cell<usize>,
    pub(crate) propagate_invalidity: RefCell<Vec<WeakNode>>,
    pub(crate) run_on_update_handlers: RefCell<Vec<(WeakNode, NodeUpdateDelayed)>>,
    pub(crate) handle_after_stabilisation: RefCell<Vec<WeakNode>>,
    pub(crate) new_observers: RefCell<Vec<WeakObserver>>,
    pub(crate) all_observers: RefCell<HashMap<ObserverId, StrongObserver>>,
    pub(crate) disallowed_observers: RefCell<Vec<WeakObserver>>,
    pub(crate) current_scope: RefCell<Scope>,
    pub(crate) set_during_stabilisation: RefCell<Vec<WeakVar>>,
    pub(crate) dead_vars: RefCell<Vec<WeakVar>>,
    /// Buffer for dropping vars
    ///
    /// If you have a Var<Var<i32>>, and then you drop the outer one, the outer one gets put in the
    /// dead_vars bucket. But then, when you stabilise, the code that actually drops the internal
    /// Vars wants to push the inner Var onto the dead_vars list. So we can't have it borrowed
    /// while we execute the Drop code.
    pub(crate) dead_vars_alt: RefCell<Vec<WeakVar>>,

    pub(crate) weak_maps: RefCell<Vec<Rc<RefCell<dyn WeakMap>>>>,
    pub(crate) weak_self: Weak<Self>,

    #[cfg(debug_assertions)]
    pub(crate) only_in_debug: OnlyInDebug,
}

impl Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State").finish()
    }
}

#[cfg(debug_assertions)]
#[derive(Debug, Default)]
pub(crate) struct OnlyInDebug {
    pub currently_running_node: RefCell<Option<WeakNode>>,
}

#[cfg(debug_assertions)]
impl OnlyInDebug {
    pub(crate) fn currently_running_node_exn(&self, name: &'static str) -> WeakNode {
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

impl State {
    pub(crate) fn public_weak(self: &Rc<Self>) -> public::WeakState {
        public::WeakState {
            inner: Rc::downgrade(self),
        }
    }
    pub(crate) fn weak(&self) -> Weak<Self> {
        self.weak_self.clone()
    }
    pub(crate) fn current_scope(&self) -> Scope {
        self.current_scope.borrow().clone()
    }

    pub(crate) fn within_scope<R>(&self, scope: Scope, f: impl FnOnce() -> R) -> R {
        if !scope.is_valid() {
            panic!("Attempted to run a closure within an invalid scope");
        }
        let old = self.current_scope.replace(scope);
        let r = f();
        self.current_scope.replace(old);
        r
    }

    pub(crate) fn new() -> Rc<Self> {
        const DEFAULT_MAX_HEIGHT_ALLOWED: usize = 128;
        Self::new_with_height(DEFAULT_MAX_HEIGHT_ALLOWED)
    }
    pub(crate) fn new_with_height(max_height: usize) -> Rc<Self> {
        Rc::new_cyclic(|weak| State {
            weak_self: weak.clone(),
            recompute_heap: RecomputeHeap::new(max_height),
            adjust_heights_heap: RefCell::new(AdjustHeightsHeap::new(max_height)),
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
            dead_vars_alt: RefCell::new(vec![]),
            handle_after_stabilisation: RefCell::new(vec![]),
            run_on_update_handlers: RefCell::new(vec![]),
            weak_maps: RefCell::new(vec![]),
            #[cfg(debug_assertions)]
            only_in_debug: OnlyInDebug::default(),
        })
    }

    pub(crate) fn add_weak_map(&self, weak_map: Rc<RefCell<dyn WeakMap>>) {
        let mut wm = self.weak_maps.borrow_mut();
        wm.push(weak_map);
    }

    pub(crate) fn constant<T: Value>(self: &Rc<Self>, value: T) -> Incr<T> {
        // TODO: - store the constant value directly in the Node.value_opt
        //       - make recompute a noop instead of a clone
        //       - set recomputed_at/changed_at to avoid queueing for recompute at all?
        //       to save the single clone that constant currently does.
        let node = Node::<Constant<T>>::create_rc(
            self.weak(),
            self.current_scope(),
            Kind::Constant(value),
        );
        Incr { node }
    }

    pub(crate) fn fold<F, T: Value, R: Value>(
        self: &Rc<Self>,
        vec: Vec<Incr<T>>,
        init: R,
        f: F,
    ) -> Incr<R>
    where
        F: FnMut(R, &T) -> R + 'static + NotObserver,
    {
        if vec.is_empty() {
            return self.constant(init);
        }
        let node = Node::<kind::ArrayFold<F, T, R>>::create_rc(
            self.weak(),
            self.current_scope(),
            Kind::ArrayFold(Box::new(kind::ArrayFold {
                init,
                fold: RefCell::new(Box::new(f)),
                children: vec,
            })),
        );
        Incr { node }
    }

    pub(crate) fn var_in_scope<T: Value>(
        self: &Rc<Self>,
        value: T,
        scope: Scope,
    ) -> public::Var<T> {
        let var = Rc::new(Var {
            state: self.weak(),
            set_at: Cell::new(self.stabilisation_num.get()),
            value: RefCell::new(value),
            node_id: NodeId(0).into(),
            node: RefCell::new(None),
            value_set_during_stabilisation: RefCell::new(None),
        });
        let node = Node::<super::var::VarGenerics<T>>::create_rc(
            self.weak(),
            scope,
            Kind::Var(var.clone()),
        );
        {
            let mut slot = var.node.borrow_mut();
            var.node_id.set(node.id);
            slot.replace(node);
        }
        public::Var::new(var)
    }

    pub(crate) fn observe<T: Value>(&self, incr: Incr<T>) -> Rc<InternalObserver<T>> {
        let internal_observer = InternalObserver::new(incr);
        self.num_active_observers.increment();
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
                    let node = obs.observing_erased();
                    let was_necessary = node.is_necessary();
                    {
                        let mut ao = self.all_observers.borrow_mut();
                        ao.insert(obs.id(), obs.clone());
                    }
                    obs.add_to_observed_node();
                    /* By adding [internal_observer] to [observing.observers], we may have added
                    on-update handlers to [observing].  We need to handle [observing] after this
                    stabilization to give those handlers a chance to run. */
                    node.handle_after_stabilisation(self);
                    debug_assert!(node.is_necessary());
                    if !was_necessary {
                        node.became_necessary_propagate(self);
                    }
                }
            }
        }
    }

    // not required. We don't have a GC with dead-but-still-participating objects to account for.
    // fn disallow_finalized_observers(&self) {}

    #[tracing::instrument]
    fn unlink_disallowed_observers(&self) {
        let mut disallowed = self.disallowed_observers.borrow_mut();
        for obs_weak in disallowed.drain(..) {
            let Some(obs) = obs_weak.upgrade() else {
                continue;
            };
            debug_assert_eq!(obs.state().get(), ObserverState::Disallowed);
            obs.state().set(ObserverState::Unlinked);
            // get a strong ref to the node, before we drop its owning InternalObserver
            let observing = obs.observing_packed();
            {
                obs.remove_from_observed_node();
                // remove from all_observers (this finally drops the InternalObserver)
                let mut ao = self.all_observers.borrow_mut();
                ao.remove(&obs.id());
                drop(obs);
            }
            observing.check_if_unnecessary(self);
        }
    }

    #[tracing::instrument]
    fn stabilise_start(&self) {
        self.status.set(IncrStatus::Stabilising);
        // self.disallow_finalized_observers();
        self.add_new_observers();
        self.unlink_disallowed_observers();
    }
    #[tracing::instrument]
    fn stabilise_end(&self) {
        self.stabilisation_num
            .set(self.stabilisation_num.get().add1());
        #[cfg(debug_assertions)]
        {
            self.only_in_debug.currently_running_node.take();
            // t.only_in_debug.expert_nodes_created_by_current_node <- []);
        }
        tracing::info_span!("set_during_stabilisation").in_scope(|| {
            let mut stack = self.set_during_stabilisation.borrow_mut();
            while let Some(var) = stack.pop() {
                let Some(var) = var.upgrade() else { continue };
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
            // This code handles Var<Var> by double buffering
            let mut alt = self.dead_vars_alt.borrow_mut();
            loop {
                let mut stack = self.dead_vars.borrow_mut();
                if stack.is_empty() {
                    break;
                }
                // Subtle: don't just swap the RefMuts! Swap the vecs.
                std::mem::swap(&mut *stack, &mut *alt);
                drop(stack);
                for var in alt.drain(..) {
                    let Some(var) = var.upgrade() else { continue };
                    tracing::debug!("dead_vars: found var with {:?}", var.id());
                    var.break_rc_cycle();
                }
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
        for wm in self.weak_maps.borrow().iter() {
            let mut w = wm.borrow_mut();
            w.garbage_collect();
        }
        self.status.set(IncrStatus::NotStabilising);
    }

    pub(crate) fn is_stable(&self) -> bool {
        self.recompute_heap.is_empty()
            && self.dead_vars.borrow().is_empty()
            && self.new_observers.borrow().is_empty()
    }

    pub(crate) fn stabilise(&self) {
        self.stabilise_debug(None)
    }

    pub(crate) fn stabilise_debug(&self, _prefix: Option<&str>) {
        let span = tracing::info_span!("stabilise");
        span.in_scope(|| {
            #[cfg(debug_assertions)]
            let mut do_debug = {
                let mut iterations = 0;
                let mut buf = String::new();
                move || {
                    if tracing::enabled!(tracing::Level::INFO) {
                        if let Some(prefix) = _prefix {
                            iterations += 1;
                            use std::fmt::Write;
                            buf.clear();
                            write!(&mut buf, "{prefix}-stabilise-{iterations}.dot").unwrap();
                            self.save_dot_to_file(&buf);
                        }
                    }
                }
            };

            #[cfg(not(debug_assertions))]
            fn do_debug() {}

            assert_eq!(self.status.get(), IncrStatus::NotStabilising);

            self.stabilise_start();

            while let Some(node) = self.recompute_heap.remove_min() {
                do_debug();
                node.recompute(self);
            }

            self.stabilise_end();
            do_debug();
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
                    node.invalidate_node(self);
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

    pub(crate) fn is_stabilising(&self) -> bool {
        self.status.get() != IncrStatus::NotStabilising
    }

    pub(crate) fn set_max_height_allowed(&self, new_max_height: usize) {
        if self.status.get() == IncrStatus::Stabilising {
            panic!("tried to set_max_height_allowed during stabilisation");
        }
        let mut ah_heap = self.adjust_heights_heap.borrow_mut();
        ah_heap.set_max_height_allowed(new_max_height);
        drop(ah_heap);
        self.recompute_heap.set_max_height_allowed(new_max_height);
    }

    pub(crate) fn set_height(&self, node: NodeRef, height: i32) {
        let mut ah_heap = self.adjust_heights_heap.borrow_mut();
        ah_heap.set_height(&node, height);
    }

    pub(crate) fn save_dot_to_file(&self, named: &str) {
        let observers = self.all_observers.borrow();
        let mut all_observed = observers.iter().map(|(_id, o)| o.observing_erased());
        super::node::save_dot_to_file(&mut all_observed, named).unwrap();
    }
    pub(crate) fn save_dot_to_string(&self) -> String {
        let observers = self.all_observers.borrow();
        let mut all_observed = observers.iter().map(|(_id, o)| o.observing_erased());
        let mut buf = String::new();
        super::node::save_dot(&mut buf, &mut all_observed).unwrap();
        buf
    }

    #[tracing::instrument]
    pub(crate) fn destroy(&self) {
        let mut dead_vars = self.dead_vars.take();
        for var in dead_vars.drain(..).filter_map(|x| x.upgrade()) {
            var.break_rc_cycle();
        }
        for (_id, obs) in self.all_observers.borrow().iter() {
            let state = obs.state().get();
            if state == ObserverState::InUse || state == ObserverState::Created {
                obs.disallow_future_use(self);
            }
        }
        self.unlink_disallowed_observers();
        self.all_observers.take().clear();
        self.disallowed_observers.take().clear();
        self.weak_maps.take().clear();
        self.recompute_heap.clear();
        self.adjust_heights_heap.borrow_mut().clear();
    }
}

impl Drop for State {
    fn drop(&mut self) {
        tracing::debug!("destroying internal State object");
        self.destroy();
    }
}

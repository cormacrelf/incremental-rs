use std::any::Any;
use std::cell::{Cell, RefCell};
use std::fmt;
use std::hash::Hash;
use std::rc::{Rc, Weak};

use super::kind::{self, Kind};
use super::node::{ErasedNode, Incremental, Input, Node, NodeId};
use super::scope::{BindScope, Scope};
use crate::incrsan::NotObserver;
use crate::kind::BindLhsChangeGen;
use crate::node_update::OnUpdateHandler;
use crate::{Cutoff, NodeRef, NodeUpdate, Observer, Value, WeakState};

#[derive(Debug)]
#[must_use = "Incr<T> must be observed (.observe()) to be part of a computation."]
pub struct Incr<T> {
    pub(crate) node: Input<T>,
}

impl<T> Clone for Incr<T> {
    fn clone(&self) -> Self {
        Self {
            node: self.node.clone(),
        }
    }
}

impl<T> From<Input<T>> for Incr<T> {
    fn from(node: Input<T>) -> Self {
        Self { node }
    }
}

impl<T> PartialEq for Incr<T> {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
    }
}

impl<T> Eq for Incr<T> {}

impl<T> Hash for Incr<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.node.id().hash(state)
    }
}

impl<T> Incr<T> {
    /// Allows an extra T2, because you might be comparing two incrs inside a map2 constructor.
    pub(crate) fn ptr_eq<T2>(&self, other: &Incr<T2>) -> bool {
        crate::rc_thin_ptr_eq_t2(&self.node, &other.node)
    }
    pub fn weak(&self) -> WeakIncr<T> {
        WeakIncr(Rc::downgrade(&self.node))
    }
    pub fn set_graphviz_user_data(&self, data: impl fmt::Debug + NotObserver + 'static) {
        self.node.set_graphviz_user_data(Box::new(data))
    }
    pub fn with_graphviz_user_data(self, data: impl fmt::Debug + NotObserver + 'static) -> Self {
        self.node.set_graphviz_user_data(Box::new(data));
        self
    }
    pub fn state(&self) -> WeakState {
        self.node.state().public_weak()
    }
}

/// Type for use in weak hash maps of incrementals
#[derive(Debug)]
pub struct WeakIncr<T>(pub(crate) Weak<dyn Incremental<T>>);
impl<T> WeakIncr<T> {
    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }
    pub fn upgrade(&self) -> Option<Incr<T>> {
        self.0.upgrade().map(Incr::from)
    }
    pub fn strong_count(&self) -> usize {
        self.0.strong_count()
    }
    pub fn weak_count(&self) -> usize {
        self.0.weak_count()
    }
}

impl<T> Clone for WeakIncr<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Value> Incr<T> {
    /// A convenience function for taking a function of type `fn(Incr<T>) -> Incr<R>` and
    /// applying it to self. This enables you to put your own functions
    /// into the middle of a chain of method calls on Incr.
    #[inline]
    pub fn pipe<R>(&self, f: impl FnOnce(Incr<T>) -> Incr<R>) -> Incr<R> {
        // clones are cheap.
        f(self.clone())
    }

    pub fn pipe1<R, A1>(&self, f: impl FnOnce(Incr<T>, A1) -> Incr<R>, arg1: A1) -> Incr<R> {
        f(self.clone(), arg1)
    }

    pub fn pipe2<R, A1, A2>(
        &self,
        f: impl FnOnce(Incr<T>, A1, A2) -> Incr<R>,
        arg1: A1,
        arg2: A2,
    ) -> Incr<R> {
        f(self.clone(), arg1, arg2)
    }

    /// A simple variation on [Incr::map] that tells you how many
    /// times the incremental has recomputed before this time.
    pub fn enumerate<R, F>(&self, mut f: F) -> Incr<R>
    where
        R: Value,
        F: FnMut(usize, &T) -> R + 'static + NotObserver,
    {
        let mut counter = 0;
        self.map(move |x| {
            let v = f(counter, x);
            counter += 1;
            v
        })
    }

    /// Turn two incrementals into a tuple incremental.
    /// Aka `both` in OCaml. This is named `zip` to match [Option::zip] and [Iterator::zip].
    pub fn zip<T2: Value>(&self, other: &Incr<T2>) -> Incr<(T, T2)> {
        if let Some(a) = self.node.constant() {
            if let Some(b) = other.node.constant() {
                return self.state().constant((a.clone(), b.clone()));
            }
        }
        self.map2(other, |a, b| (a.clone(), b.clone()))
    }

    pub fn map_ref<F, R: Value>(&self, f: F) -> Incr<R>
    where
        F: for<'a> Fn(&'a T) -> &'a R + 'static + NotObserver,
    {
        let state = self.node.state();
        let node = Node::<kind::MapRefNode<R>>::create_rc(
            state.weak(),
            state.current_scope(),
            Kind::MapRef(kind::MapRefNode {
                input: self.node.packed(),
                did_change: true.into(),
                mapper: Box::new(move |x: &dyn Any| {
                    let x = x.downcast_ref::<T>().unwrap();
                    f(x)
                }),
            }),
        );
        Incr { node }
    }

    /// A version of map that gives you a (weak) reference to the map node you're making, in the
    /// closure.
    ///
    /// Useful for advanced usage where you want to add manual dependencies with the
    /// [crate::expert] constructs.
    pub fn map_cyclic<R: Value>(
        &self,
        mut cyclic: impl FnMut(WeakIncr<R>, &T) -> R + 'static + NotObserver,
    ) -> Incr<R> {
        let node = Rc::<Node<_>>::new_cyclic(move |node_weak| {
            let f = {
                let weak = WeakIncr(node_weak.clone());
                move |t: &dyn Any| {
                    let t = t.downcast_ref().unwrap();
                    miny::Miny::new_unsized(cyclic(weak.clone(), t))
                }
            };
            let mapper = kind::MapNode {
                input: self.node.packed(),
                mapper: Box::new(RefCell::new(f)),
                phantom: Default::default(),
            };
            let state = self.node.state();
            let mut node = Node::<kind::MapNode<R>>::create(
                state.weak(),
                state.current_scope.borrow().clone(),
                Kind::Map(mapper),
            );
            node.weak_self = node_weak.clone();
            node
        });
        node.created_in.add_node(node.clone());

        Incr { node }
    }

    /// A version of [Incr::map] that allows reuse of the old
    /// value. You can use it to produce a new value. The main
    /// use case is avoiding allocation.
    ///
    /// The return type of the closure is `(R, bool)`. The boolean
    /// value is a replacement for the [Cutoff] system, because
    /// the [Cutoff] functions require access to an old value and
    /// a new value. With [Incr::map_with_old], you must figure out yourself
    /// (without relying on PartialEq, for example) whether the
    /// incremental node should propagate its changes.
    ///
    pub fn map_with_old<R, F>(&self, mut f: F) -> Incr<R>
    where
        R: Value,
        F: FnMut(Option<R>, &T) -> (R, bool) + 'static + NotObserver,
    {
        let state = self.node.state();
        let node = Node::<kind::MapWithOld<R>>::create_rc(
            state.weak(),
            state.current_scope(),
            Kind::MapWithOld(kind::MapWithOld {
                input: self.node.packed(),
                mapper: RefCell::new(Box::new(move |opt_r, x: &dyn Any| {
                    let x = x.downcast_ref::<T>().unwrap();
                    f(opt_r, x)
                })),
                _p: std::marker::PhantomData,
            }),
        );
        Incr { node }
    }

    /// A version of bind that includes a copy of the [crate::IncrState] (as [crate::WeakState])
    /// to help you construct new incrementals within the bind.
    pub fn binds<F, R>(&self, mut f: F) -> Incr<R>
    where
        R: Value,
        F: FnMut(&WeakState, &T) -> Incr<R> + 'static + NotObserver,
    {
        let cloned = self.node.state().public_weak();
        self.bind(move |value: &T| f(&cloned, value))
    }

    pub fn bind<F, R>(&self, mut f: F) -> Incr<R>
    where
        R: Value,
        F: FnMut(&T) -> Incr<R> + 'static + NotObserver,
    {
        let state = self.node.state();
        let bind = Rc::new_cyclic(|weak| kind::BindNode {
            lhs: self.node.packed(),
            mapper: RefCell::new(Box::new(move |any_ref: &dyn Any| -> NodeRef {
                let downcast = any_ref
                    .downcast_ref::<T>()
                    .expect("Type mismatch in bind function");
                f(downcast).node.packed()
            })),
            rhs: RefCell::new(None),
            rhs_scope: Scope::Bind(weak.clone() as Weak<dyn BindScope>).into(),
            all_nodes_created_on_rhs: RefCell::new(vec![]),
            lhs_change: RefCell::new(Weak::<Node<BindLhsChangeGen<R>>>::new()),
            id_lhs_change: Cell::new(NodeId(0)),
            main: RefCell::new(Weak::<Node<kind::BindNodeMainGen<R>>>::new()),
        });
        let lhs_change = Node::<kind::BindLhsChangeGen<R>>::create_rc(
            state.weak(),
            state.current_scope(),
            Kind::BindLhsChange {
                casts: kind::BindLhsId {
                    r_unit: refl::refl(),
                },
                bind: bind.clone(),
            },
        );
        let main = Node::<kind::BindNodeMainGen<R>>::create_rc(
            state.weak(),
            state.current_scope(),
            Kind::BindMain {
                bind: bind.clone(),
                lhs_change: lhs_change.clone(),
            },
        );
        {
            let mut bind_lhs_change = bind.lhs_change.borrow_mut();
            let mut bind_main = bind.main.borrow_mut();
            *bind_lhs_change = lhs_change.weak();
            *bind_main = main.weak();
            bind.id_lhs_change.set(lhs_change.id);
        }

        /* We set [lhs_change] to never cutoff so that whenever [lhs] changes, [main] is
        recomputed.  This is necessary to handle cases where [f] returns an existing stable
        node, in which case the [lhs_change] would be the only thing causing [main] to be
        stale. */
        lhs_change.set_cutoff(Cutoff::Never);
        Incr { node: main }
    }

    /// Creates an observer for this incremental.
    ///
    /// Observers are the primary way to get data out of the computation.
    /// Their creation and lifetime inform Incremental which parts of the
    /// computation graph are necessary, such that if you create many
    /// variables and computations based on them, but only hook up some of
    /// that to an observer, only the parts transitively necessary to
    /// supply the observer with values are queued to be recomputed.
    ///
    /// That means, without an observer, `var.set(new_value)` does essentially
    /// nothing, even if you have created incrementals like
    /// `var.map(...).bind(...).map(...)`. In this fashion, you can safely
    /// set up computation graphs before you need them, or refuse to dismantle
    /// them, knowing the expensive computations they contain will not
    /// grace the CPU until they're explicitly put back under the purview
    /// of an Observer.
    ///
    /// Calling this multiple times on the same node produces multiple
    /// observers. Only one is necessary to keep a part of a computation
    /// graph alive and ticking.
    pub fn observe(&self) -> Observer<T> {
        let incr = self.clone();
        let internal = incr.node.state().observe(incr);
        Observer::new(internal)
    }

    /// Sets the cutoff function that determines (if it returns true)
    /// whether to stop (cut off) propagating changes through the graph.
    /// Note that this method can be called on `Var` as well as any
    /// other [Incr].
    ///
    /// The default is [Cutoff::PartialEq]. So if your values do not change,
    /// they will cut off propagation. There is a bound on all T in
    /// `Incr<T>` used in incremental-rs, all values you pass around
    /// must be PartialEq.
    ///
    /// You can also supply your own comparison function. This will chiefly
    /// be useful for types like `Rc<T>`, not to avoid T: PartialEq (you
    /// can't avoid that) but rather to avoid comparing a large structure
    /// and simply compare the allocation's pointer value instead.
    /// In that case, you can:
    ///
    /// ```rust
    /// use std::rc::Rc;
    /// use incremental::{IncrState, Cutoff};
    /// let incr = IncrState::new();
    /// let var = incr.var(Rc::new(5));
    /// var.set_cutoff(Cutoff::Fn(Rc::ptr_eq));
    /// // but note that doing this will now cause the change below
    /// // to propagate, whereas before it would not as the two
    /// // numbers are == equal:
    /// var.set(Rc::new(5));
    /// ```
    ///
    pub fn set_cutoff(&self, cutoff: Cutoff<T>) {
        self.node.set_cutoff(cutoff);
    }

    /// A shorthand for using [Incr::set_cutoff] with a function pointer,
    /// i.e. with [Cutoff::Fn]. Most comparison functions, including
    /// closures, can be cast to a function pointer because they won't
    /// capture any values.
    ///
    /// ```rust
    /// use incremental::IncrState;
    /// let incr = IncrState::new();
    /// let var = incr.var(5);
    /// var.set_cutoff_fn(|a, b| a == b);
    /// var.set_cutoff_fn(i32::eq);
    /// var.set_cutoff_fn(PartialEq::eq);
    /// ```
    pub fn set_cutoff_fn(&self, cutoff_fn: fn(&T, &T) -> bool) {
        self.node.set_cutoff(Cutoff::Fn(cutoff_fn));
    }

    /// A shorthand for using [Incr::set_cutoff] with [Cutoff::FnBoxed] and
    /// a closure that may capture its environment and mutate its captures.
    ///
    /// ```rust
    /// use std::{cell::Cell, rc::Rc};
    /// use incremental::IncrState;
    /// let incr = IncrState::new();
    /// let var = incr.var(5);
    /// let capture = Rc::new(Cell::new(false));
    /// var.set_cutoff_fn_boxed(move |_, _| capture.get());
    /// ```
    pub fn set_cutoff_fn_boxed<F>(&self, cutoff_fn: F)
    where
        F: FnMut(&T, &T) -> bool + Clone + 'static + NotObserver,
    {
        self.node.set_cutoff(Cutoff::FnBoxed(Box::new(cutoff_fn)));
    }

    pub fn save_dot_to_file(&self, named: &str) {
        super::node::save_dot_to_file(&mut core::iter::once(self.node.erased()), named).unwrap()
    }

    pub fn depend_on<T2: Value>(&self, on: &Incr<T2>) -> Incr<T> {
        let output = self.map2(on, |a, _| a.clone());
        preserve_cutoff(self, &output);
        output
    }

    pub fn on_update(&self, f: impl FnMut(NodeUpdate<&T>) + 'static + NotObserver) {
        let state = self.node.state();
        let now = state.stabilisation_num.get();
        let handler = OnUpdateHandler::new(now, Box::new(f));
        self.node.add_on_update_handler(handler);
    }
}

fn preserve_cutoff<T: Value, O: Value>(input: &Incr<T>, output: &Incr<O>) {
    let input_ = input.weak();
    let output_ = output.weak();
    output.set_cutoff_fn_boxed(move |_, _| {
        if let Some((i, o)) = input_.upgrade().zip(output_.upgrade()) {
            i.node.changed_at() == o.node.changed_at()
        } else {
            panic!("preserve_cutoff input or output was deallocated")
        }
    })
}

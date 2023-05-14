use std::{cell::RefCell, rc::Rc};
// RUST_LOG_SPAN_EVENTS=enter,exit
use test_log::test;

use incremental::{expert::*, Incr, IncrState, Value};

fn join<T: Value>(incr: &Incr<Incr<T>>) -> Incr<T> {
    let prev_rhs: Rc<RefCell<Option<Dependency<T>>>> = Rc::new(None.into());
    let state = incr.state();
    let join = Node::<T, T>::new(&state, {
        let prev_rhs_ = prev_rhs.clone();
        move || prev_rhs_.borrow().clone().unwrap().value_cloned()
    });
    let join_ = join.weak();
    // TODO: should not need to create a map node for this.
    let lhs_change = incr.map(move |rhs| {
        let dep = join_.add_dependency(rhs);
        let mut prev_rhs_ = prev_rhs.borrow_mut();
        if let Some(prev) = prev_rhs_.take() {
            join_.remove_dependency(prev);
        }
        prev_rhs_.replace(dep);
    });
    join.add_dependency_unit(&lhs_change);
    join.watch()
}

#[test]
fn test_join() {
    let incr = IncrState::new();
    let inner = incr.var(10i32);
    let outer = incr.var(inner.watch());
    let joined = join(&outer);
    let o = joined.observe();
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(10));
    inner.set(20);
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(20));
}

#[allow(dead_code)]
fn bind<T: Value, R: Value>(incr: Incr<T>, mut f: impl FnMut(&T) -> Incr<R> + 'static) -> Incr<R> {
    let prev_rhs: Rc<RefCell<Option<Dependency<R>>>> = Rc::new(None.into());
    let state = incr.state();
    let join = Node::<R, R>::new(&state, {
        let prev_rhs_ = prev_rhs.clone();
        move || prev_rhs_.borrow().clone().unwrap().value_cloned()
    });
    let join_ = join.weak();
    let lhs_change = incr.map(move |input| {
        let rhs = f(input);
        let mut prev_rhs_ = prev_rhs.borrow_mut();
        if prev_rhs_.as_ref().map_or(false, |prev| prev.node() == rhs) {
            return;
        }
        let dep = join_.add_dependency(&rhs);
        if let Some(prev) = prev_rhs_.take() {
            join_.remove_dependency(prev);
        }
        prev_rhs_.replace(dep);
    });
    join.add_dependency_unit(&lhs_change);
    join.watch()
}

#[test]
fn map345_expert() {
    let incr = IncrState::new();
    let i1 = incr.var(3);
    let i2 = incr.var(5);
    let i3 = incr.var(7);
    let triple = i1.map3(&i2, &i3, |a, b, c| (a * b, *c));

    // now have an expert node add a dependency on triple
    let outer = incr.var(triple.clone());
    let joined = join(&outer);

    let j = joined.observe();
    incr.stabilise();
    assert_eq!(j.value(), (15, 7));
}

fn manual_zip2<T1: Value, T2: Value>(one: &Incr<T1>, two: &Incr<T2>) -> Incr<(T1, T2)> {
    let state = one.state();
    enum Storage<A, B> {
        None,
        OneOnly(A),
        TwoOnly(B),
        Both(A, B),
    }
    impl<A, B> Storage<A, B> {
        fn take(&mut self) -> Self {
            std::mem::replace(self, Storage::None)
        }
        fn both_cloned(&self) -> (A, B)
        where
            A: Clone,
            B: Clone,
        {
            match self {
                Self::Both(a, b) => (a.clone(), b.clone()),
                _ => panic!("zip2 node has not yet read both inputs"),
            }
        }
    }
    let current = Rc::new(RefCell::new(Storage::None));
    let zip2 = Node::<(T1, T2), ()>::new(&state, {
        let current_ = current.clone();
        move || current_.borrow().both_cloned()
    });
    let current_1 = current.clone();
    zip2.add_dependency_with_(one, move |new_one| {
        let mut tuple = current_1.borrow_mut();
        let storage = tuple.take();
        let new_one = new_one.clone();
        let new = match storage {
            Storage::Both(_, b) | Storage::TwoOnly(b) => Storage::Both(new_one, b),
            Storage::None | Storage::OneOnly(_) => Storage::OneOnly(new_one),
        };
        *tuple = new;
    });
    let current_2 = current;
    zip2.add_dependency_with_(two, move |new_two| {
        let mut tuple = current_2.borrow_mut();
        let storage = tuple.take();
        let new_b = new_two.clone();
        let new = match storage {
            Storage::Both(a, _) | Storage::OneOnly(a) => Storage::Both(a, new_b),
            Storage::None | Storage::TwoOnly(_) => Storage::TwoOnly(new_b),
        };
        *tuple = new;
    });
    zip2.watch()
}

#[test]
fn test_zip2() {
    let incr = IncrState::new();
    let i1 = incr.var(3);
    let i2 = incr.var(5);
    let z = manual_zip2(&i1, &i2);
    let o = z.observe();
    incr.stabilise();
    assert_eq!(o.value(), (3, 5));
}

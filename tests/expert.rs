use std::{cell::RefCell, rc::Rc};

use incremental::{expert::*, Incr, IncrState, Value};

fn join<T: Value>(incr: Incr<Incr<T>>) -> Incr<T> {
    let prev_rhs: Rc<RefCell<Option<Dependency<T>>>> = Rc::new(None.into());
    let state = incr.state();
    let join = Node::<T, T>::new(&state, {
        let prev_rhs_ = prev_rhs.clone();
        move || prev_rhs_.borrow().clone().unwrap().value_cloned()
    });
    let join_ = join.weak();
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
    let joined = join(outer.watch());
    let o = joined.observe();
    incr.stabilise();
    assert_eq!(o.value(), Ok(10));
    inner.set(20);
    incr.stabilise();
    assert_eq!(o.value(), Ok(20));
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

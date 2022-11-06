use std::{cell::RefCell, rc::Rc};

use incremental::{IncrState, Incr, expert::*, Value};

fn join<T: Value>(incr: Incr<Incr<T>>) -> Incr<T> {
    let prev_rhs: Rc<RefCell<Option<Dependency<T>>>> = Rc::new(None.into());
    let state = incr.state();
    let join = Node::<T, T>::new(&state, {
        let prev_rhs_ = prev_rhs.clone();
        move || {
            prev_rhs_.borrow().clone().unwrap().value_cloned()
        }
    });
    let join_ = join.clone();
    let lhs_change = incr.map(move |rhs| {
        let rhs_dep = Dependency::new(rhs);
        join_.add_dependency(rhs_dep.clone());
        let mut prev_rhs_ = prev_rhs.borrow_mut();
        if let Some(prev) = prev_rhs_.take() {
            join_.remove_dependency(prev);
        }
        prev_rhs_.replace(rhs_dep);
    });
    join.add_dependency_unit(Dependency::new(&lhs_change));
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

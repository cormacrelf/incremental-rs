use std::{cell::Cell, rc::Rc};

use incremental::{Incr, IncrState, Observer, SubscriptionToken, Value, WeakState};
use test_log::test;

// fn fixed_point(f: impl FnMut (Vec<Incr<T>>) -> Vec<Incr<T>>)

fn fixed_point<T: Value>(
    state: &WeakState,
    init: T,
    mut f: impl FnMut(&mut T) -> T + 'static,
) -> Incr<T> {
    let var = state.var(init);
    // TODO: Cloning var and using it in a node may make  a ref cycle.
    // prefer var.weak() (but we need to override this for Var so it
    // is not a bare Incr<T>).
    let v = var.clone();

    // now, mapping var. if var is set during stabilisation, then
    // it queues var to be recomputed next round.
    // however this does not enqueue this map node. that only happens
    // during stabilisation IF var changes.
    var.map(move |_input| {
        // returns the old value
        // this means the first output of this incremental
        // is just init.
        v.replace_with(|x| f(x))
    })
}

#[test]
fn one_node() {
    let incr = IncrState::new();
    let observer = fixed_point(&incr.weak(), 10_u32, |x| x.saturating_sub(1)).observe();
    observer
        .subscribe(|t| {
            println!("observed {:?}", t);
        })
        .unwrap();
    while !incr.is_stable() {
        incr.stabilise();
    }
    assert_eq!(observer.expect_value(), 0);
}

struct FixedPointIter<'a, T: Value> {
    cycle_count: Rc<Cell<i32>>,
    token: SubscriptionToken,
    observer: Observer<T>,
    state: &'a IncrState,
}

impl<'a, T: Value> FixedPointIter<'a, T> {
    fn new(state: &'a IncrState, observer: Observer<T>) -> Self {
        let cycle_count = Rc::new(Cell::new(0i32));
        let weak = Rc::downgrade(&cycle_count);
        let token = observer
            .subscribe(move |_val| {
                let count = weak.upgrade().unwrap();
                count.set(count.get() + 1);
            })
            .unwrap();
        Self {
            cycle_count,
            token,
            observer,
            state,
        }
    }

    fn iterate(&self) -> T {
        let mut last_cycle_count = -1;
        while self.cycle_count.get() != last_cycle_count {
            last_cycle_count = self.cycle_count.get();
            // this will possibly increment cycle_count
            // if it doesn't, it's because the observed node did not emit a change event.
            self.state.stabilise();
        }
        self.observer.expect_value()
    }
}

impl<'a, T: Value> Drop for FixedPointIter<'a, T> {
    fn drop(&mut self) {
        self.state.unsubscribe(self.token);
    }
}

#[test]
fn iterated() {
    // let cell = Rc::<Cell<i32>>::new();
    let incr = IncrState::new();
    let tillzero = fixed_point(&incr.weak(), 10_u32, |x| x.saturating_sub(1));
    let observer = tillzero.observe();
    let fixed_point = FixedPointIter::new(&incr, observer);
    let value = fixed_point.iterate();
    assert_eq!(value, 0);
    panic!();
}

#[test]
fn dependencies() {
    // let cell = Rc::<Cell<i32>>::new();
    let incr = IncrState::new();
    let tillzero = fixed_point(&incr.weak(), 10_u32, |x| x.saturating_sub(1));
    let mapped = tillzero.map(|x| x + 1);
    let observer = mapped.observe();
    observer
        .subscribe(|t| {
            println!("observed {:?}", t);
        })
        .unwrap();
    while !incr.is_stable() {
        incr.stabilise();
    }
    assert_eq!(observer.expect_value(), 1);
    panic!();
}

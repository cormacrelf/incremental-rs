use std::{cell::Cell, rc::Rc};

use incremental::{Incr, IncrState, NodeUpdate, Observer, SubscriptionToken, Value, WeakState};
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
        while self.cycle_count.get() != last_cycle_count && !self.state.is_stable() {
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
}

#[test]
fn dependencies() {
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
}

fn using_cutoff<T: Value>(
    state: &WeakState,
    init: T,
    mut f: impl FnMut(&mut T) -> T + 'static,
) -> (Incr<T>, UntilStableValue<T>) {
    let var = state.var(init);
    // TODO: Cloning var and using it in a node may make  a ref cycle.
    // prefer var.weak() (but we need to override this for Var so it
    // is not a bare Incr<T>).
    let v_mapped = var.clone();

    // now, mapping var. if var is set during stabilisation, then
    // it queues var to be recomputed next round.
    // however this does not enqueue this map node. that only happens
    // during stabilisation IF var changes.
    let output = var.map(move |_input| {
        // returns the old value
        // this means the first output of this incremental
        // is just init.
        v_mapped.replace_with(|x| f(x))
    });

    // Do not trigger a recompute until our value is stable.
    let v_cutoff = var.clone();
    output.set_cutoff_custom_boxed(move |_, _| {
        let was_changed = v_cutoff.was_changed_during_stabilisation();
        println!("cutoff function ran; was it changed during stab? {was_changed:?}");
        was_changed
    });
    let until = UntilStableValue::new(output.observe());
    (output, until)
}

#[test]
fn dependencies_using_cutoff() {
    let incr = IncrState::new();
    let cell = Rc::new(Cell::new(0i32));
    let (tillzero, _) = using_cutoff(&incr.weak(), 10_u32, |x| x.saturating_sub(1));
    // add a dependency and observe that
    let map_observer = tillzero.map(|x| x + 1).observe();
    let o_cell = cell.clone();
    map_observer
        .subscribe(move |t| {
            println!("observed {:?}", t);
            if let NodeUpdate::Changed(_) = t {
                o_cell.set(o_cell.get() + 1);
            }
        })
        .unwrap();
    while !incr.is_stable() {
        incr.stabilise();
    }
    assert_eq!(map_observer.expect_value(), 1);
    // this time we didn't fire until tillzero had settled.
    assert_eq!(cell.get(), 1);
}

struct UntilStableValue<T: Value> {
    change_count: Rc<Cell<i32>>,
    token: SubscriptionToken,
    observer: Observer<T>,
}

impl<T: Value> UntilStableValue<T> {
    fn new(observer: Observer<T>) -> Self {
        let change_count = Rc::new(Cell::new(0i32));
        let count = change_count.clone();
        let token = observer
            .subscribe(move |_val| {
                if let NodeUpdate::Changed(_) = _val {
                    count.set(count.get() + 1);
                }
            })
            .unwrap();
        Self {
            change_count,
            token,
            observer,
        }
    }

    fn iterate(&self, state: &IncrState) -> T {
        let next_change_count = self.change_count.get() + 1;
        while self.change_count.get() < next_change_count && !state.is_stable() {
            // this will possibly increment change_count
            // if it doesn't, it's because the observed node did not emit a change event.
            state.stabilise();
        }
        self.observer.expect_value()
    }
}

impl<T: Value> Drop for UntilStableValue<T> {
    fn drop(&mut self) {
        self.observer.unsubscribe(self.token).unwrap()
    }
}

#[test]
fn dependencies_using_cutoff_iterated() {
    let incr = IncrState::new();
    let cell = Rc::new(Cell::new(0i32));
    let (tillzero, until_stable) = using_cutoff(&incr.weak(), 10_u32, |x| x.saturating_sub(1));
    // add a dependency and observe that
    let map_observer = tillzero.map(|x| x + 1).observe();
    let o_cell = cell.clone();
    map_observer
        .subscribe(move |t| {
            println!("observed {:?}", t);
            if let NodeUpdate::Changed(_) = t {
                o_cell.set(o_cell.get() + 1);
            }
        })
        .unwrap();
    until_stable.iterate(&incr);
    assert_eq!(map_observer.expect_value(), 1);
    // this time we didn't fire until tillzero had settled.
    assert_eq!(cell.get(), 1);
}

#[test]
fn two_fixedpoints_iterated() {
    let incr = IncrState::new();
    let cell = Rc::new(Cell::new(0i32));

    let (from_10, until_stable_10) = using_cutoff(&incr.weak(), 10_u32, |x| x.saturating_sub(1));
    let (from_20, ____________) = using_cutoff(&incr.weak(), 20_u32, |x| x.saturating_sub(1));
    let still_20 = from_20.map(|&x| x);

    let o_from_20 = from_20.observe();
    let o_still_20 = still_20.observe();
    assert_eq!(until_stable_10.iterate(&incr), 0);
    assert_eq!(o_from_20.expect_value(), 10);

    // until_stable_10 only did 10 stabilise()s.
    // so from_20 hasn't gotten to a fixed point yet, and so any
    // downstream nodes have still not been queued for a recompute.
    assert_eq!(o_still_20.expect_value(), 20);
}

#[test]
fn two_fixedpoints_combined() {
    let incr = IncrState::new();
    let (from_10, until_stable_10) = using_cutoff(&incr.weak(), 10_u32, |x| x.saturating_sub(1));
    let (from_20, ____________) = using_cutoff(&incr.weak(), 20_u32, |x| x.saturating_sub(1));
    let o_from_20 = from_20.observe();

    let counter = Rc::new(Cell::new(0));
    let count = counter.clone();
    let combined = (from_10 % from_20)
        .map(move |&ten, &twenty| {
            println!("combined is recomputing");
            count.set(count.get() + 1);
            ten + twenty
        })
        .observe();

    assert_eq!(until_stable_10.iterate(&incr), 0);
    assert_eq!(o_from_20.expect_value(), 10);

    // combined was only recomputed once
    assert_eq!(counter.get(), 2);
    assert_eq!(combined.expect_value(), 10);

    // blast the rest of the way.
    while !incr.is_stable() {
        incr.stabilise();
    }
    assert_eq!(counter.get(), 3);
    assert_eq!(combined.expect_value(), 0);
}

#[cfg(feature = "im-rc")]
#[test]
fn transitive_closure() {
    use im_rc::{hashmap, hashset, HashMap, HashSet};
    let incr = IncrState::new();

    // (1, 2), (2, 3), (3, 4)         gets (1, 3), (2, 4) added
    // (1, 2), (2, 3), (3, 4), (1, 3) gets (1, 4)         added
    //
    let map: HashMap<i32, HashSet<i32>> = hashmap! {
        1 => hashset!{2},
        2 => hashset!{3},
        3 => hashset!{4},
    };

    let (_node, until_stable) = using_cutoff(&incr.weak(), map, |map| {
        let mut new = map.clone();
        for (&a, a_trans) in map.iter() {
            for &b in a_trans.iter() {
                let b_trans = map.get(&b);
                for &c in b_trans.into_iter().flatten() {
                    let new_a_trans = new.entry(a).or_default();
                    new_a_trans.insert(c);
                }
            }
        }
        println!("transitive closure round produced: {new:?}");
        new
    });

    let output = until_stable.iterate(&incr);
    assert_eq!(
        output,
        hashmap! {
            1 => hashset!{2, 3, 4},
            2 => hashset!{3, 4},
            3 => hashset!{4},
        }
    )
}

fn using_cutoff_bind<T: Value, F>(init: Incr<T>, f: F) -> (Incr<T>, UntilStableValue<T>)
where
    T: Default,
    F: FnMut(&mut T) -> T + 'static + Clone,
{
    let state = init.state();
    let var = state.var(T::default());
    // TODO: Cloning var and using it in a node may make  a ref cycle.
    // prefer var.weak() (but we need to override this for Var so it
    // is not a bare Incr<T>).
    let v_mapped = var.clone();

    // now, mapping var. if var is set during stabilisation, then
    // it queues var to be recomputed next round.
    // however this does not enqueue this map node. that only happens
    // during stabilisation IF var changes.
    let output = init.bind(move |init_val| {
        println!("setting to new init val: {:?}", init_val);
        v_mapped.set(init_val.clone());
        // returns the old value
        // this means the first output of this incremental
        // is just init.
        let v = v_mapped.clone();
        let mut f_ = f.clone();
        let output = v_mapped.map(move |_x| v.replace_with(|x| f_(x)));
        // Do not trigger a recompute until our value is stable.
        output
    });
    let v_cutoff = var.clone();
    output.set_cutoff_custom_boxed(move |_, _| {
        let was_changed = v_cutoff.was_changed_during_stabilisation();
        println!("cutoff function ran; was it changed during stab? {was_changed:?}");
        was_changed
    });
    let until = UntilStableValue::new(output.observe());
    (output, until)
}

#[cfg(feature = "im-rc")]
mod transitive_closure {
    use super::*;
    use im_rc::{hashmap, hashset, HashMap, HashSet};
    use incremental::IntoIncr;
    use test_log::test;

    type EfficientSet = HashMap<i32, HashSet<i32>>;

    fn transitive_closure(
        input_set: impl IntoIncr<EfficientSet>,
    ) -> (Incr<EfficientSet>, UntilStableValue<EfficientSet>) {
        using_cutoff_bind(input_set.into_incr(), |map| {
            let mut new = map.clone();
            for (&a, a_trans) in map.iter() {
                for &b in a_trans.iter() {
                    let b_trans = map.get(&b);
                    for &c in b_trans.into_iter().flatten() {
                        let new_a_trans = new.entry(a).or_default();
                        new_a_trans.insert(c);
                    }
                }
            }
            println!("transitive closure round produced: {new:?}");
            new
        })
    }

    #[test]
    fn with_query() {
        let incr = IncrState::new();
        let query = incr.var((1, 4));
        let map = incr.var(hashmap! {
            1 => hashset!{2},
            2 => hashset!{3},
            3 => hashset!{4},
        });

        let (closure, until_stable) = transitive_closure(map.watch());

        let is_in_set = (query.watch() % closure)
            .map(|(from, to), map| map.get(from).map_or(false, |trans| trans.contains(to)))
            .observe();

        until_stable.iterate(&incr);

        query.set((2, 1));
        incr.stabilise();
        assert_eq!(is_in_set.expect_value(), false);
        query.set((1, 3));
        incr.stabilise();
        assert_eq!(is_in_set.expect_value(), true);
        map.modify(|m| {
            m.entry(3).or_default().insert(1);
        });
        until_stable.iterate(&incr);
        assert_eq!(is_in_set.expect_value(), true);
    }

    #[test]
    fn bound() {
        let incr = IncrState::new();

        // (1, 2), (2, 3), (3, 4)         gets (1, 3), (2, 4) added
        // (1, 2), (2, 3), (3, 4), (1, 3) gets (1, 4)         added
        //
        let initial: HashMap<i32, HashSet<i32>> = hashmap! {
            1 => hashset!{2},
            2 => hashset!{3},
            3 => hashset!{4},
        };

        let map_var = incr.var(initial);

        let (_node, until_stable) = transitive_closure(map_var.watch());

        let output = until_stable.iterate(&incr);
        assert_eq!(
            output,
            hashmap! {
                1 => hashset!{2, 3, 4},
                2 => hashset!{3, 4},
                3 => hashset!{4},
            }
        );
        map_var.update(|mut v| {
            v.entry(3).or_default().insert(1);
            v
        });
        let output = until_stable.iterate(&incr);
        assert_eq!(
            output,
            hashmap! {
                1 => hashset!{1, 2, 3, 4},
                2 => hashset!{1, 2, 3, 4},
                3 => hashset!{1, 2, 3, 4},
            }
        );
    }
}

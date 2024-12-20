use test_log::test;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use incremental::{Cutoff, Incr, IncrState, NodeUpdate, StatsDiff, Update, Var};

#[test]
fn testit() {
    let incr = IncrState::new();
    let var = incr.var(5);
    var.set(10);
    let observer = var.observe();
    incr.stabilise();
    assert_eq!(observer.try_get_value(), Ok(10));
}

#[test]
fn test_map() {
    let incr = IncrState::new();
    let var = incr.var(5);
    let mapped = var.map(|x| dbg!(dbg!(x) * 10));
    let observer = mapped.observe();
    incr.stabilise();
    assert_eq!(observer.try_get_value(), Ok(50));
    var.set(3);
    incr.stabilise();
    assert_eq!(observer.try_get_value(), Ok(30));
}

#[test]
fn test_map2() {
    let incr = IncrState::new();
    let a = incr.var(5);
    let b = incr.var(8);
    let mapped = a.map2(&b, |a, b| dbg!(dbg!(a) + dbg!(b)));
    let observer = mapped.observe();
    incr.stabilise();
    assert_eq!(observer.try_get_value(), Ok(13));
    // each of these queues up the watch node into the recompute heap.
    a.set(3);
    b.set(9);
    // during stabilisation the map node gets inserted into the recompute heap
    incr.stabilise();
    assert_eq!(observer.try_get_value(), Ok(12));
}
#[test]
fn test_map_map2() {
    let incr = IncrState::new();
    let a = incr.var(5);
    let b = incr.var(8);
    let map_left = a.map(|a| dbg!(a * 10));
    let mapped = map_left.map2(&b, |left, b| dbg!(dbg!(left) + dbg!(b)));
    let observer = mapped.observe();
    incr.stabilise();
    assert_eq!(observer.try_get_value(), Ok(58));
    // each of these queues up the watch node into the recompute heap.
    a.set(3);
    b.set(9);
    // during stabilisation the map node gets inserted into the recompute heap
    incr.stabilise();
    assert_eq!(observer.try_get_value(), Ok(39));
}

#[test]
fn test_bind_existing() {
    #[derive(Debug, Clone, PartialEq)]
    enum Choose {
        B,
        C,
    }

    println!("");
    println!("------------------------------------------------------------------");
    println!("------------------------------------------------------------------");
    let incr = IncrState::new();
    let choose = incr.var(Choose::B);
    let b = incr.var(5);
    let c = incr.var(10);
    // we move b_ & c_ into the closure.
    // and we clone one of them each time a changes
    let b_ = b.watch();
    let c_ = c.watch();
    let bound = choose.bind(move |choose| match dbg!(choose) {
        Choose::B => b_.clone(),
        Choose::C => c_.clone(),
    });
    let obs = bound.observe();
    incr.stabilise();
    assert_eq!(dbg!(obs.try_get_value()), Ok(5));

    println!("");
    println!("------------------------------------------------------------------");
    println!("Modifying b, which is currently bound and should cause observable changes");
    println!("------------------------------------------------------------------");
    b.set(50);
    incr.stabilise();
    assert_eq!(dbg!(obs.try_get_value()), Ok(50));

    println!("");
    println!("------------------------------------------------------------------");
    println!("Modifying c, which is NOT currently bound, so no change");
    println!("------------------------------------------------------------------");
    c.set(99);
    incr.stabilise();
    assert_eq!(dbg!(obs.try_get_value()), Ok(50));

    println!("");
    println!("------------------------------------------------------------------");
    println!("Changing bind's LHS will swap out b for c in the bind");
    println!("So the bind function will be re-evaluated");
    println!("------------------------------------------------------------------");
    choose.set(Choose::C);
    incr.stabilise();
    assert_eq!(dbg!(obs.try_get_value()), Ok(99));
    tracing::warn!("{:?}", incr.stats());
    assert_eq!(incr.stats().became_unnecessary, 1);
}

/// Exercisees Adjust_heights_heap for a variable that was created from scratch inside a bind
#[test]
fn create_var_in_bind() {
    let incr = IncrState::new();
    let lhs = incr.var(true);
    let o = lhs
        .binds(move |state, _| {
            // the difference between creating the variable beforehand
            // and doing it inside the bind is that during the bind, new nodes
            // have a scope. so the new variable is created with a height of -1,
            // but when became_necessary runs on it, it inherits the bind scope's
            // height + 1.
            //
            // But the bind scope has the BindLhsChange node's height.
            //
            // So BindMain also inherits it at h+1. That means that v here, which is now
            // the bind RHS, and BindMain, both have a height of 3. Except BindMain is
            // meant to be recomputed after the RHS, so it can copy the value that's
            // already been computed and stored in the RHS node.
            //
            // If they both have a height of 3, it is not determined that RHS is computed
            // first. So we need to adjust heights to make sure. You might ask -- why isn't
            // BindMain already picking up the height of its children and adding 1 to it?
            // That's because BindMain is an existing node, it is already necessary, and
            // that procedure is done in became_necessary.
            //
            // The key lines in adjust_heights_heap.ml are
            //
            //     set_height t parent (child.height + 1))
            //     ...
            //     if debug then assert (original_child.height < original_parent.height)
            //
            // That's because in its roundabout way, it's doing the same thing as
            // became_necessary, but for the dynamic nodes created by a bind.
            //
            let v = state.var(9);
            tracing::debug!("--------------------------------");
            tracing::debug!("created var in bind with id {:?}", v.id());
            tracing::debug!("--------------------------------");
            v.watch()
        })
        .observe();
    incr.stabilise();
    incr.stabilise();
    incr.stabilise();
    lhs.set(false);
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(9));
    o.save_dot_to_file("create_var_in_bind.dot");
}

#[test]
fn bind_changing_heights_outside() {
    let incr = IncrState::new();
    let is_short = incr.var(true);
    // this is a very short graph, height 1.
    let short_graph = incr.var(5).watch();
    // this is a very short graph, height 4.
    let long_graph = incr.var(10).map(|&x| x).map(|&x| x).map(|&x| x);
    let obs = is_short
        // BindMain's height is initially bumped to 3 (short=1, lhs=2, main=3)
        .bind(move |&s| {
            if s {
                short_graph.clone()
            } else {
                long_graph.clone()
            }
        })
        .observe();
    println!("------------------------------------------------------------------");
    incr.stabilise();
    assert_eq!(obs.try_get_value(), Ok(5));
    println!("------------------------------------------------------------------");
    println!("set long");
    println!("------------------------------------------------------------------");
    // BindMain's height is bumped up to 5.
    is_short.set(false);
    incr.stabilise();
    assert_eq!(obs.try_get_value(), Ok(10));
    println!("------------------------------------------------------------------");
    println!("return to short");
    println!("------------------------------------------------------------------");
    // here, BindMain still has height 5.
    // But it should not decrease its height, just because its child is now back to height 1.
    // We have child.height() < parent.height(), so that's fine. And, AdjustHeightsHeap is not
    // capable of setting a height lower than the previous one.
    is_short.set(true);
    incr.stabilise();
    assert_eq!(obs.try_get_value(), Ok(5));
}

#[test]
fn bind_changing_heights_inside() {
    let incr = IncrState::new();
    let is_short = incr.var(true);
    let i2 = incr.weak();
    let obs = is_short
        .bind(move |&s| {
            if s {
                i2.var(5).watch()
            } else {
                i2.var(10).map(|&x| x).map(|&x| x).map(|&x| x)
            }
        })
        .observe();
    println!("------------------------------------------------------------------");
    incr.stabilise();
    assert_eq!(obs.try_get_value(), Ok(5));
    println!("------------------------------------------------------------------");
    println!("set long");
    println!("------------------------------------------------------------------");
    is_short.set(false);
    incr.stabilise();
    assert_eq!(obs.try_get_value(), Ok(10));
    println!("------------------------------------------------------------------");
    println!("return to short");
    println!("------------------------------------------------------------------");
    is_short.set(true);
    incr.stabilise();
    assert_eq!(obs.try_get_value(), Ok(5));
}

#[test]
fn enumerate() {
    let g = Rc::new(Cell::new(0));
    let g_ = g.clone();
    let incr = IncrState::new();
    let v = incr.var("first");
    let m = v.enumerate(move |i, &x| {
        g_.set(i + 1);
        x
    });
    let o = m.observe();
    assert_eq!(g.get(), 0);
    incr.stabilise();
    assert_eq!(g.get(), 1);

    v.set("second");
    v.set("second again");
    assert_eq!(g.get(), 1);
    incr.stabilise();
    assert_eq!(g.get(), 2);

    v.set("third");
    assert_eq!(g.get(), 2);
    incr.stabilise();
    assert_eq!(g.get(), 3);
    // ensure observer is alive to keep the map function running
    drop(o);
}

#[test]
fn mutable_string() {
    let incr = IncrState::new();
    let v = incr.var("hello");
    let mut buf = String::new();
    let o = v
        // We move buf so it's owned by the closure.
        // We clone buf so we don't have a reference
        // escaping the closure.
        .map(move |s| {
            buf.push_str(s);
            buf.clone()
        })
        .observe();
    incr.stabilise();
    incr.stabilise();
    assert_eq!(o.value(), "hello");
    v.set(", world");
    incr.stabilise();
    assert_eq!(o.value(), "hello, world");
}

#[test]
fn fold() {
    let incr = IncrState::new();
    let vars = [incr.var(10), incr.var(20), incr.var(30)];
    let watches = vars.iter().map(Var::watch).collect();
    let sum = incr.fold(watches, 0, |acc, x| acc + x);
    let obs = sum.observe();
    incr.stabilise();
    assert_eq!(obs.value(), 60);
    vars[0].set(30);
    vars[0].set(40);
    incr.stabilise();
    assert_eq!(obs.value(), 90);
}

#[test]
fn bind_fold() {
    let incr = IncrState::new();

    let v1 = incr.var(10i32);
    let v2 = incr.var(20);
    let v3 = incr.var(30);

    let vars = incr.var(vec![v1.clone(), v2, v3]);

    let sum = vars.binds(|incr, vars| {
        let watches = vars.iter().map(Var::watch).collect();
        incr.fold(watches, 0, |acc, x| acc + x)
    });

    let obs = sum.observe();
    incr.stabilise();
    assert_eq!(obs.try_get_value(), Ok(60));

    v1.set(40);
    incr.stabilise_debug("bind_fold");

    assert_eq!(obs.try_get_value(), Ok(90));
    vars.set(vec![v1.clone()]);
    incr.stabilise();
    assert_eq!(obs.try_get_value(), Ok(40));
}

// #[test]
// fn unordered_fold() {
//     let incr = IncrState::new();
//
//     let v1 = incr.var(10i32);
//     let v2 = incr.var(20);
//     let v3 = incr.var(30);
//
//     let vars = incr.var(vec![v1.clone(), v2, v3]);
//
//     let sum = vars.binds(|incr, vars| {
//         let watches = vars.iter().map(Var::watch).collect();
//         incr.unordered_fold(
//             watches,
//             0,
//             |acc, x| acc + x,
//             |acc, old, new| acc + new - old,
//             None,
//         )
//     });
//
//     let obs = sum.observe();
//     incr.stabilise();
//     obs.save_dot_to_file("unordered_fold-1.dot");
//     assert_eq!(obs.try_get_value(), Ok(60));
//
//     v1.set(40);
//     incr.stabilise();
//     obs.save_dot_to_file("unordered_fold-2.dot");
//
//     assert_eq!(obs.try_get_value(), Ok(90));
//     vars.set(vec![v1.clone()]);
//     incr.stabilise();
//     obs.save_dot_to_file("unordered_fold-3.dot");
//     assert_eq!(obs.try_get_value(), Ok(40));
//
//     vars.set(vec![]);
//     incr.stabilise();
//     obs.save_dot_to_file("unordered_fold-4.dot");
// }

#[derive(Debug)]
struct CallCounter(#[allow(unused)] &'static str, Cell<u32>);

#[allow(dead_code)]
impl CallCounter {
    fn new(name: &'static str) -> Rc<Self> {
        Self(name, Cell::new(0)).into()
    }
    fn count(&self) -> u32 {
        self.1.get()
    }
    fn increment(&self) {
        self.1.set(self.1.get() + 1);
    }
    fn wrap1<A, R>(
        self: Rc<Self>,
        mut f: impl (FnMut(&A) -> R) + Clone,
    ) -> impl FnMut(&A) -> R + Clone {
        move |a| {
            self.increment();
            f(a)
        }
    }
    fn wrap_folder<'a, B, R>(
        &'a self,
        mut f: impl (FnMut(R, &B) -> R) + Clone + 'a,
    ) -> impl FnMut(R, &B) -> R + Clone + 'a {
        move |a, b| {
            self.increment();
            f(a, b)
        }
    }
}

impl PartialEq<u32> for CallCounter {
    fn eq(&self, other: &u32) -> bool {
        self.1.get().eq(other)
    }
}

// #[test]
// fn unordered_fold_inverse() {
//     let f = CallCounter::new("f");
//     let finv = CallCounter::new("finv");
//     let incr = IncrState::new();
//     let v1 = incr.var(10i32);
//     let v2 = incr.var(20);
//     let v3 = incr.var(30);
//     let vars = incr.var(vec![v1.clone(), v2.clone(), v3.clone()]);
//     let f_ = f.clone();
//     let finv_ = finv.clone();
//     let sum = vars.binds(move |incr, vars| {
//         let f_ = f_.clone();
//         let finv_ = finv_.clone();
//         let watches: Vec<_> = vars.iter().map(Var::watch).collect();
//         incr.unordered_fold_inverse(
//             watches,
//             0,
//             move |acc, x| {
//                 f_.increment();
//                 acc + x
//             },
//             move |acc, x| {
//                 finv_.increment();
//                 acc - x
//             }, // this time our update function is
//             // constructed for us.
//             None,
//         )
//     });
//     let obs = sum.observe();
//     incr.stabilise();
//     assert_eq!(obs.try_get_value(), Ok(60));
//     assert_eq!(*f, 3);
//     assert_eq!(*finv, 0);
//
//     v1.set(40);
//     incr.stabilise();
//     assert_eq!(obs.try_get_value(), Ok(90));
//     assert_eq!(*f, 4);
//     assert_eq!(*finv, 1);
//
//     vars.set(vec![v1.clone()]);
//     incr.stabilise();
//     assert_eq!(obs.try_get_value(), Ok(40));
//     assert_eq!(*f, 5);
//     // we create a new UnorderedArrayFold in the bind.
//     // Hence finv was not needed, fold_value was zero
//     // and we just folded the array non-incrementally
//     assert_eq!(*finv, 1);
// }

#[test]
fn var_set_during_stabilisation() {
    let incr = IncrState::new();
    let v = incr.var(10_i32);
    let v_ = v.clone();
    let o = v
        .map(move |&x| {
            v_.set(x + 10);
            x
        })
        .observe();

    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(10));
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(20));
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(30));
}

#[test]
fn var_update_simple() {
    let incr = IncrState::new();
    let var = incr.var(10);
    let o = var.observe();
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(10));
    var.update(|x| x + 10);
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(20));
    var.modify(|x| *x += 10);
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(30));
    let old = var.replace(40);
    incr.stabilise();
    assert_eq!(old, 30);
    assert_eq!(o.try_get_value(), Ok(40));
    let old = var.replace_with(|old| {
        *old += 5;
        *old + 5
    });
    incr.stabilise();
    assert_eq!(old, 45);
    assert_eq!(o.try_get_value(), Ok(50));
}

#[test]
fn var_update_during_stabilisation() {
    let incr = IncrState::new();
    let var = incr.var(10);
    let var_ = var.clone();
    let o = var
        .map(move |&x| {
            var_.modify(|y| *y += 1);
            x
        })
        .observe();
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(10));
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(11));
    var.set(5);
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(5));
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(6));
    o.save_dot_to_file("var_update_during_stabilisation.dot");
}

#[test]
fn var_update_before_set_during_stabilisation() {
    let incr = IncrState::new();
    let var = incr.var(10);
    let var_ = var.clone();
    let o = var
        .map(move |&x| {
            var_.set(99);
            x
        })
        .observe();
    var.modify(|x| *x += 1);
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(11));
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(99));
}

#[test]
fn test_constant() {
    let incr = IncrState::new();
    let c = incr.constant(5);
    let m = c.map(|x| x + 10);
    let o = m.observe();
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(15));
    let flip = incr.var(true);
    let o2 = flip
        .binds(|incr, &t| {
            if t {
                incr.constant(5)
            } else {
                incr.constant(10)
            }
        })
        .observe();
    incr.stabilise();
    assert_eq!(o2.try_get_value(), Ok(5));
    flip.set(!flip.get());
    incr.stabilise();
    assert_eq!(o2.try_get_value(), Ok(10));
    o2.save_dot_to_file("test_constant.dot");
}

#[test]
#[should_panic = "ptr_eq"]
fn two_worlds() {
    let one = IncrState::new();
    let two = IncrState::new();
    let v2 = two.var(9);
    let v2_ = v2.watch();
    let _o = one.constant(5).bind(move |_| v2_.clone()).observe();
    let _o2 = v2.observe();
    two.stabilise();
    one.stabilise();
}

#[test]
fn cutoff_rc_ptr_eq() {
    let sum_counter = CallCounter::new("map on rc");
    let sum_counter_ = sum_counter.clone();
    let incr = IncrState::new();
    let rc: Rc<Vec<i32>> = Rc::new(vec![1i32, 2, 3]);
    let var = incr.var(rc.clone());
    let v = var.clone();
    // Rc::ptr_eq is a fn(&T, &T) -> bool for any T.
    // And Var derefs to Incr, so you can use the method directly.
    v.watch().set_cutoff(Cutoff::Never);
    v.set_cutoff(Cutoff::PartialEq);

    // And we're using Rc::ptr_eq! Without any fancy specialization tricks!
    // Because v already knows what its T is!
    v.set_cutoff(Cutoff::Fn(Rc::ptr_eq));

    // The default is of course PartialEq.

    let o = v
        .map(move |list| {
            sum_counter_.increment();
            list.iter().sum::<i32>()
        })
        .observe();

    incr.stabilise();
    assert_eq!(*sum_counter, 1);

    // We didn't change the Rc that was returned. So map should not have to call its function again.
    var.set(rc.clone());
    incr.stabilise();
    assert_eq!(*sum_counter, 1);
    assert_eq!(o.try_get_value(), Ok(6));

    var.set(Rc::new(vec![1, 2, 3, 4]));
    incr.stabilise();
    assert_eq!(*sum_counter, 2);
    assert_eq!(o.try_get_value(), Ok(10));
}

#[test]
fn cutoff_sum() {
    let add10_counter = CallCounter::new("sum + 10");
    let add10_counter_ = add10_counter.clone();
    let incr = IncrState::new();
    let vec = vec![1i32, 2, 3];
    let v = incr.var(vec);
    let o = v
        .map(|xs| xs.into_iter().fold(0i32, |acc, &x| acc + x))
        // because our first map node checks PartialEq before changing
        // its output value, the second map node will not have to run.
        .map(add10_counter_.wrap1(|&sum| sum + 10))
        .observe();

    incr.stabilise();
    assert_eq!(*add10_counter, 1);
    assert_eq!(o.try_get_value(), Ok(16));

    v.set(vec![6]);
    incr.stabilise();
    assert_eq!(*add10_counter, 1); // the crux
    assert_eq!(o.try_get_value(), Ok(16));

    v.set(vec![9, 1]);
    incr.stabilise();
    assert_eq!(*add10_counter, 2); // the crux again
    assert_eq!(o.try_get_value(), Ok(20));
}

#[test]
fn map_with_old_reuse() {
    let incr = IncrState::new();
    let v = incr.var(100);
    let o = v
        .map_with_old(|old: Option<Rc<String>>, input: &i32| {
            let mut rc = old.unwrap_or_default();
            if Rc::strong_count(&rc) != 1 {
                panic!("Rc::make_mut was going to clone");
            }
            let string = Rc::make_mut(&mut rc);
            tracing::info!("old: {:?}", &string);
            string.clear();
            use std::fmt::Write;
            write!(string, "{}", input).unwrap();
            (rc, true)
        })
        // we can ruin everything by doing this
        // .map(|x| x.clone())
        .observe();

    incr.stabilise();
    assert_eq!(o.value().to_string(), "100".to_string());
    v.set(200);
    incr.stabilise();
    assert_eq!(o.value().to_string(), "200".to_string());
}

#[test]
fn observer_subscribe_drop() {
    let call_count = CallCounter::new("subscriber");
    let call_count_ = call_count.clone();
    let incr = IncrState::new();
    let var = incr.var(10);
    let observer = var.observe();
    observer.subscribe(move |value| {
        tracing::info!("received update: {:?}", value);
        call_count_.increment();
    });
    incr.stabilise();
    assert_eq!(*call_count, 1);
    incr.stabilise();
    assert_eq!(*call_count, 1);
    var.set(11);
    incr.stabilise();
    assert_eq!(*call_count, 2);

    drop(observer);
    var.set(12);
    incr.stabilise();
    assert_eq!(*call_count, 2);
}

#[test]
fn observer_unsubscribe() {
    let call_count = CallCounter::new("subscriber");
    let call_count_ = call_count.clone();
    let incr = IncrState::new();
    let var = incr.var(10);
    let observer = var.observe();
    let token = observer.subscribe(move |value| {
        tracing::debug!("received update: {:?}", value);
        call_count_.increment();
    });
    incr.stabilise();
    assert_eq!(*call_count, 1);
    var.set(11);
    incr.stabilise();
    assert_eq!(*call_count, 2);

    observer.unsubscribe(token).unwrap();
    var.set(12);
    incr.stabilise();
    assert_eq!(*call_count, 2); // crux: still 2.
}

#[test]
fn state_unsubscribe_after_observer_dropped() {
    let incr = IncrState::new();
    let var = incr.var(10);
    let observer = var.observe();
    let token = observer.subscribe(|value| {
        tracing::debug!("received update: {:?}", value);
    });
    let second = observer.subscribe(|_| ());
    incr.stabilise();
    var.set(11);
    incr.stabilise();

    // crux
    drop(observer);
    tracing::debug!("unsubscribing {token:?}");
    // this should not panic
    incr.unsubscribe(token);

    var.set(12);
    incr.stabilise();

    tracing::debug!("unsubscribing {token:?}");
    incr.unsubscribe(second);
}

fn stabilise_diff(incr: &incremental::IncrState, msg: &str) -> incremental::StatsDiff {
    let before = incr.stats();
    incr.stabilise();
    let delta = incr.stats() - before;
    println!("{msg} : {delta:#?}");
    delta
}

#[test]
fn becomes_unnecessary() {
    let incr = IncrState::new();

    let zero = incr.stats();
    let v = incr.var(10);
    let maps = v.map(|&x| x).map(|&x| x);
    assert!(matches!(
        incr.stats() - zero,
        StatsDiff {
            created: 3,
            became_necessary: 0,
            became_unnecessary: 0,
            necessary: 0,
            ..
        }
    ));

    let o = maps.observe();
    let diff = stabilise_diff(&incr, "after observe maps & stabilise");
    assert!(matches!(
        diff,
        StatsDiff {
            created: 0,
            became_necessary: 3,
            became_unnecessary: 0,
            necessary: 3,
            ..
        }
    ));

    drop(o);
    let diff = stabilise_diff(&incr, "dropping the maps observer");
    assert!(matches!(
        diff,
        StatsDiff {
            created: 0,
            became_necessary: 0,
            became_unnecessary: 3,
            necessary: -3,
            ..
        }
    ));
    assert_eq!(incr.stats().became_unnecessary, 3);
    assert_eq!(incr.stats().necessary, 0);

    let useit = incr.var(false);
    let bind = useit.binds(move |incr, &in_use| {
        if in_use {
            maps.clone()
        } else {
            incr.constant(5)
        }
    });

    let diff = stabilise_diff(&incr, "after creating bind (no diff)");
    assert_eq!(diff, StatsDiff::default());

    // This creates an extra node incr.constant(5).
    let obs = bind.observe();
    let diff = stabilise_diff(&incr, "after observing bind");
    assert!(matches!(
        diff,
        StatsDiff {
            // i.e. nodes created during stabilise
            created: 1,
            became_necessary: 4,
            became_unnecessary: 0,
            necessary: 4,
            ..
        }
    ));
    assert_eq!(incr.stats().necessary, 4);
    assert_eq!(obs.try_get_value(), Ok(5));

    // Now we swap the bind's output from a constant to the three-node map
    useit.set(true);
    let diff = stabilise_diff(&incr, "after setting bind input to true");
    assert!(matches!(
        diff,
        StatsDiff {
            created: 0,
            became_necessary: 3,
            became_unnecessary: 1,
            ..
        }
    ));
    assert_eq!(incr.stats().necessary, 6);
    assert_eq!(obs.try_get_value(), Ok(10));

    drop(obs);
    let diff = stabilise_diff(&incr, "after dropping bind observer");
    assert!(matches!(
        diff,
        StatsDiff {
            created: 0,
            became_necessary: 0,
            became_unnecessary: 6,
            necessary: -6,
            ..
        }
    ));
    assert_eq!(incr.stats().necessary, 0);
}

#[derive(PartialEq, Clone, Debug)]
struct MapRefTest {
    other: i32,
    string: String,
}

#[test]
fn test_map_ref() {
    let incr = IncrState::new();
    let var = incr.var(MapRefTest {
        other: 5,
        string: "hello".into(),
    });
    let string = var.map_ref(|thing| &thing.string).observe();

    string.subscribe(|change| {
        println!("changed: {change:?}");
    });

    incr.stabilise();

    var.modify(|v| v.other = 10);
    incr.stabilise();
    var.modify(|v| v.other = 20);
    incr.stabilise();
    var.modify(|v| v.string += ", world");
    incr.stabilise();
}

#[test]
fn map_with_old() {
    let incr = IncrState::new();
    let v = incr.var(10);
    let _ = v
        .map_with_old(|old, input| {
            tracing::warn!("old {old:?} <- input {input:?}");
            (input + 1, false)
        })
        .observe();
    incr.stabilise();
    v.set(20);
    incr.stabilise();
    v.set(30);
    incr.stabilise();
}

#[test]
#[ignore = "we're ok with map_ref behaving like this, given it saves a clone"]
fn map_with_old_map_ref() {
    let counter = CallCounter::new("map_ref");
    let incr = IncrState::new();
    let v = incr.var((10, 20));
    let _m = v
        .map_with_old(|old, input| {
            // Does nothing.
            let v = *input;
            (v, old.map_or(true, |o| o != v))
        })
        // problem: map_with_old doesn't keep its old value.
        // so how are we supposed to compare them in
        // child_changed on map_ref?
        //
        // this node is meant to Cutoff::PartialEq. But it can't. So it's equivalent
        // to using Cutoff::Never.
        .map_ref(|x| &x.1)
        .map({
            let counter_ = counter.clone();
            move |x| {
                counter_.increment();
                tracing::warn!("map_ref said it changed, so map ran for {x}");
                5
            }
        })
        .observe();
    incr.stabilise();
    assert_eq!(*counter, 1);
    v.set((10, 30));
    incr.stabilise();
    assert_eq!(*counter, 2);
    v.modify(|_| {});
    incr.stabilise();
    assert_eq!(
        *counter, 2,
        "as long as map_with_old doesn't actually change, it cuts off right"
    );
    v.set((20, 30));
    incr.stabilise();
    assert_eq!(
        *counter, 2,
        "should not execute map_ref, since the original map node was cut off."
    );
}

#[test]
fn weak_memoize_fn() {
    let incr = IncrState::new();
    let counter = CallCounter::new("memoized function");
    let function = {
        let counter = counter.clone();
        let incr = incr.weak();
        move |x: i32| {
            counter.increment();
            incr.constant(x)
        }
    };
    let mut memoized = incr.weak_memoize_fn(function);
    let five = memoized(5);
    let another_five = memoized(5);
    assert_eq!(counter.count(), 1);
    assert_eq!(five, another_five);

    drop(five);
    drop(another_five);
    // garbage collets any registered weak hashmaps
    incr.stabilise();
    assert_eq!(counter.count(), 1);
    let _new_five = memoized(5);
    assert_eq!(counter.count(), 2);
    let _six = memoized(6);
    assert_eq!(counter.count(), 3);
}

#[test]
fn test_depend_on() {
    let state = IncrState::new();
    let x = state.var(13);
    let y = state.var(14);
    let d = x.depend_on(&y.watch());
    let ox = d.observe();
    let nx: Rc<Cell<i32>> = Default::default();
    let nx_ = nx.clone();
    let incr_nx = move |upd: Update<&_>| match upd {
        Update::Invalidated => panic!(),
        Update::Changed(_) | Update::Initialised(_) => nx_.set(nx_.get() + 1),
    };
    let _tok = ox.subscribe(incr_nx.clone());

    let ny: Rc<Cell<i32>> = Default::default();
    let ny_ = ny.clone();
    y.on_update(move |upd| match upd {
        NodeUpdate::Invalidated => panic!(),
        NodeUpdate::Unnecessary => println!("y became unnecessary"),
        NodeUpdate::Changed(_) | NodeUpdate::Necessary(_) => ny_.set(ny_.get() + 1),
    });

    macro_rules! check(($state:ident, $o:ident => $eo:expr, $nx:ident => $enx:expr, $ny:ident => $eny:expr) => {
        $state.stabilise();
        assert_eq!($o.value(), $eo);
        assert_eq!($nx.get(), $enx, stringify!($nx));
        assert_eq!($ny.get(), $eny, stringify!($ny));
    });

    check!(state, ox => 13, nx => 1, ny => 1);
    x.set(15);
    check!(state, ox => 15, nx => 2, ny => 1);
    y.set(16);
    check!(state, ox => 15, nx => 2, ny => 2);
    x.set(17);
    y.set(18);
    check!(state, ox => 17, nx => 3, ny => 3);
    x.set(17);
    check!(state, ox => 17, nx => 3, ny => 3);
    y.set(18);
    check!(state, ox => 17, nx => 3, ny => 3);
    ox.disallow_future_use();

    macro_rules! check(($state:ident, $nx:ident => $enx:expr, $ny:ident => $eny:expr) => {
        $state.stabilise();
        assert_eq!($nx.get(), $enx, stringify!($nx));
        assert_eq!($ny.get(), $eny, stringify!($ny));
    });

    x.set(19);
    y.set(20);
    check!(state, nx => 3, ny => 3);
    let o_ = d.observe();
    let _ = o_.subscribe(incr_nx);
    check!(state, nx => 4, ny => 4);
    assert_eq!(o_.value(), 19);
}

#[test]
fn test_duplicate_incrstate_drop() {
    let incr = IncrState::new();
    let obs = incr.var(5).observe();

    incr.stabilise();
    assert_eq!(obs.value(), 5);

    // I previously left an accidental `inner.destroy()` in the
    // Drop impl of IncrState.
    let cloned = incr.clone();
    drop(cloned);

    assert_eq!(obs.value(), 5);

    drop(obs);
    drop(incr);
}

#[test]
fn map3456() {
    let incr = IncrState::new();
    let i1 = incr.var(3);
    let i2 = incr.var(5);
    let i3 = incr.var(7);
    let triple = i1.map3(&i2, &i3, |a, b, c| a * b + c);
    let trip = triple.observe();
    incr.stabilise();
    assert_eq!(trip.value(), 22);
    i3.set(9);
    incr.stabilise();
    assert_eq!(trip.value(), 24);
    let i4 = incr.var(100);
    let i5 = incr.var(200);
    let i6 = incr.var(300);
    let quadruple = i1.map4(&i2, &i3, &i4, |a, b, c, d| a * b + c + d);
    let quad = quadruple.observe();
    let quintuple = i1.map5(&i2, &i3, &i4, &i5, |a, b, c, d, e| a * b + c + d + e);
    let five = quintuple.observe();
    let sextuple = i1.map6(&i2, &i3, &i4, &i5, &i6, |a, b, c, d, e, f| {
        a * b + c + d + e + f
    });
    let six = sextuple.observe();
    incr.stabilise();
    assert_eq!(trip.value(), 24);
    assert_eq!(quad.value(), 124);
    assert_eq!(five.value(), 324);
    assert_eq!(six.value(), 624);
    i1.set(0);
    incr.stabilise();
    assert_eq!(trip.value(), 9);
    assert_eq!(quad.value(), 109);
    assert_eq!(five.value(), 309);
    assert_eq!(six.value(), 609);
}

#[test]
fn map345_ref() {
    let incr = IncrState::new();
    let i1 = incr.var(3);
    let i2 = incr.var(5);
    let i3 = incr.var(7);
    let triple = i1.map3(&i2, &i3, |a, b, c| (a * b, *c));
    let trip = triple.observe();
    incr.stabilise();
    assert_eq!(trip.value(), (15, 7));
    let by_ref = triple.map_ref(|(ab, _)| ab);
    let obs = by_ref.observe();
    incr.stabilise();
    assert_eq!(obs.value(), 15);
}

#[test]
fn drop_var_var() {
    let incr = IncrState::new();
    let var = incr.var(incr.var(5i32));
    incr.stabilise();
    drop(var);
    incr.stabilise();
    assert!(incr.is_stable());
    incr.stabilise();
}

#[test]
fn drop_var_var_obs() {
    let incr = IncrState::new();
    let var = incr.var(incr.var(5i32));
    let o = var.bind(|v| v.watch()).observe();
    incr.stabilise();
    drop(var);
    incr.stabilise();
    drop(o);
    incr.stabilise();
}

#[test]
fn drop_var_var_obs_var() {
    let incr = IncrState::new();
    let var = incr.var(incr.var(incr.var(5i32)));
    let o = var.bind(|v| v.watch()).observe();
    incr.stabilise();
    o.disallow_future_use();
    incr.stabilise();
    drop(o);
    drop(var);
    incr.stabilise();
}

#[test]
fn var_bind_to_itself() {
    let incr = IncrState::new();
    let var = incr.var(5i32);
    let v = var.clone();
    let o = var.bind(move |_| v.watch()).observe();
    incr.stabilise();
    drop(o);
    incr.stabilise();
    drop(var);
    incr.stabilise();
}

#[test]
fn map2_itself() {
    let incr = IncrState::new();
    let var = incr.var(5i32);
    let i = var.watch();
    let o = i.map2(&i, |a, b| a + b).observe();
    incr.stabilise();
    o.disallow_future_use();
    incr.stabilise();
    drop(o);
    drop(var);
    incr.stabilise();
}

#[test]
fn map2_itself_unobserved() {
    let incr = IncrState::new();
    let var = incr.var(5i32);
    let i = var.watch();
    let _ = i.map2(&i, |a, b| a + b);
    incr.stabilise();
    drop(var);
    incr.stabilise();
}

#[ignore = "This panics in the destructor while recovering from the BorrowMutError"]
#[should_panic = "BorrowMutError"]
#[test]
fn bind_bind() {
    let incr = IncrState::new();
    let var = incr.var(5i32);
    let i = var.watch();
    // This code is gacked. Don't do it.
    let cell = Rc::new(RefCell::new(None::<Incr<i32>>));
    let cell_ = cell.clone();
    let b = i.bind(move |_| cell_.borrow().as_ref().unwrap().clone());
    cell.borrow_mut().replace(b.clone());
    let o = b.observe();
    incr.stabilise();
    drop(o);
    incr.stabilise();
}

#[test]
fn bind_fold_2() {
    let incr = IncrState::new();
    let vector = incr.var(vec![
        incr.var(1).watch(),
        incr.var(2).watch(),
        incr.var(3).watch(),
    ]);
    let vec = vector.clone();
    let init = incr.var(0);
    let sum = init.binds(move |s, &init| {
        let s = s.clone();
        vec.watch()
            .map(move |vector| s.fold(vector.clone(), init, |acc, x| acc + x))
    });
    let _o = sum.observe();
    incr.stabilise();
    vector.modify(|v| v.push(incr.var(10).watch()));
    incr.stabilise();
    vector.modify(|v| v.clear());
    incr.stabilise();
}

#[ignore = "Not sure why this doesn't panic. Normally we would BorrowMut in the parent-twiddling code."]
#[should_panic]
#[test]
fn fold_many_same_incr() {
    let incr = IncrState::new();
    let five = incr.var(5i32);
    let five_ = five.clone();
    let bound = incr.constant(false).bind(move |_| five_.watch());
    // This basically shouldn't work
    let vec = incr.var(vec![five.watch(), five.watch(), bound]);
    let sum = vec.binds(|s, vec| s.fold(vec.clone(), 0i32, |acc, x| acc + x));
    let o = sum.observe();
    incr.stabilise();
    assert_eq!(o.value(), 15);
    vec.modify(|v| {
        v.pop();
    });
    incr.stabilise();
    assert_eq!(o.value(), 10);
}

#[test]
fn simultaneous_disallow_and_rebind() {
    let incr = IncrState::new();
    let c1 = incr.constant(1);
    let c2 = incr.constant(2);
    let v = incr.var(c1);
    let b1 = v.watch().binds(|_, v| v.clone());
    let b2 = b1.binds(|s, &v| s.constant(v));
    let o = b2.observe();
    incr.stabilise();
    v.set(c2);
    o.disallow_future_use();
    drop(o);
    incr.stabilise();
    let _o2 = b1.observe();
    incr.stabilise();
}

#[test]
fn fold_duplicate_inputs() {
    let incr = IncrState::new();
    let constant = incr.constant(1);

    let vec = vec![constant.clone(), constant];
    let fold = incr.fold(vec, 0, |i, _| i + 1);
    let obs = fold.observe();
    incr.stabilise();
    drop(obs);
    incr.stabilise();
}

#[test]
fn map_duplicate_inputs() {
    let incr = IncrState::new();
    let constant = incr.constant(1);

    let combine = (constant.clone() % constant).map(|_, _| 5);
    let obs = combine.observe();
    incr.stabilise();
    drop(obs);
    incr.stabilise();
}

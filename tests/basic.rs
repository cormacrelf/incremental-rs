use test_log::test;

use std::{cell::Cell, collections::BTreeMap, rc::Rc};

use incremental::{Cutoff, Incr, IncrState, Observer, ObserverError, StatsDiff, Var};

#[test]
fn testit() {
    let incr = IncrState::new();
    let var = incr.var(5);
    var.set(10);
    let observer = var.observe();
    incr.stabilise();
    assert_eq!(observer.value(), Ok(10));
}

#[test]
fn test_map() {
    let incr = IncrState::new();
    let var = incr.var(5);
    let mapped = var.map(|x| dbg!(dbg!(x) * 10));
    let observer = mapped.observe();
    incr.stabilise();
    assert_eq!(observer.value(), Ok(50));
    var.set(3);
    incr.stabilise();
    assert_eq!(observer.value(), Ok(30));
}

#[test]
fn test_map2() {
    let incr = IncrState::new();
    let a = incr.var(5);
    let b = incr.var(8);
    let mapped = a.map2(&b, |a, b| dbg!(dbg!(a) + dbg!(b)));
    let observer = mapped.observe();
    incr.stabilise();
    assert_eq!(observer.value(), Ok(13));
    // each of these queues up the watch node into the recompute heap.
    a.set(3);
    b.set(9);
    // during stabilisation the map node gets inserted into the recompute heap
    incr.stabilise();
    assert_eq!(observer.value(), Ok(12));
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
    assert_eq!(observer.value(), Ok(58));
    // each of these queues up the watch node into the recompute heap.
    a.set(3);
    b.set(9);
    // during stabilisation the map node gets inserted into the recompute heap
    incr.stabilise();
    assert_eq!(observer.value(), Ok(39));
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
    assert_eq!(dbg!(obs.value()), Ok(5));

    println!("");
    println!("------------------------------------------------------------------");
    println!("Modifying b, which is currently bound and should cause observable changes");
    println!("------------------------------------------------------------------");
    b.set(50);
    incr.stabilise();
    assert_eq!(dbg!(obs.value()), Ok(50));

    println!("");
    println!("------------------------------------------------------------------");
    println!("Modifying c, which is NOT currently bound, so no change");
    println!("------------------------------------------------------------------");
    c.set(99);
    incr.stabilise();
    assert_eq!(dbg!(obs.value()), Ok(50));

    println!("");
    println!("------------------------------------------------------------------");
    println!("Changing bind's LHS will swap out b for c in the bind");
    println!("So the bind function will be re-evaluated");
    println!("------------------------------------------------------------------");
    choose.set(Choose::C);
    incr.stabilise();
    assert_eq!(dbg!(obs.value()), Ok(99));
    tracing::warn!("{:?}", incr.stats());
    assert_eq!(incr.stats().became_unnecessary, 1);
}

/// Exercisees Adjust_heights_heap for a variable that was created from scratch inside a bind
#[test]
fn create_var_in_bind() {
    let incr = IncrState::new();
    let lhs = incr.var(true);
    let state = incr.clone();
    let o = lhs
        .bind(move |_| {
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
    assert_eq!(o.value(), Ok(9));
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
    assert_eq!(obs.value(), Ok(5));
    println!("------------------------------------------------------------------");
    println!("set long");
    println!("------------------------------------------------------------------");
    // BindMain's height is bumped up to 5.
    is_short.set(false);
    incr.stabilise();
    assert_eq!(obs.value(), Ok(10));
    println!("------------------------------------------------------------------");
    println!("return to short");
    println!("------------------------------------------------------------------");
    // here, BindMain still has height 5.
    // But it should not decrease its height, just because its child is now back to height 1.
    // We have child.height() < parent.height(), so that's fine. And, AdjustHeightsHeap is not
    // capable of setting a height lower than the previous one.
    is_short.set(true);
    incr.stabilise();
    assert_eq!(obs.value(), Ok(5));
}

#[test]
fn bind_changing_heights_inside() {
    let incr = IncrState::new();
    let is_short = incr.var(true);
    let i2 = incr.clone();
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
    assert_eq!(obs.value(), Ok(5));
    println!("------------------------------------------------------------------");
    println!("set long");
    println!("------------------------------------------------------------------");
    is_short.set(false);
    incr.stabilise();
    assert_eq!(obs.value(), Ok(10));
    println!("------------------------------------------------------------------");
    println!("return to short");
    println!("------------------------------------------------------------------");
    is_short.set(true);
    incr.stabilise();
    assert_eq!(obs.value(), Ok(5));
}

#[test]
fn observer_ugly() {
    let incr = IncrState::new();
    let lhs = incr.var(true);
    let unused = incr.var(2);
    let unused_w = unused.watch();
    let state = incr.clone();
    let v9 = state.var(9);
    let bound = lhs.bind(move |&l| if l { v9.watch() } else { unused_w.clone() });
    let o = bound.observe();

    incr.stabilise();
    assert_eq!(o.value(), Ok(9));

    let unrelated = incr.var(100);
    // make sure to add unrelated to recompute heap first
    let _o = unrelated.observe();

    let var = incr.var(10);
    let o1 = var.map(|a| a + 10).observe();
    assert_eq!(o1.value(), Err(ObserverError::NeverStabilised));

    let map = unrelated.map(move |_x| o.value());
    let o2 = map.observe();

    incr.stabilise();
    // ok none of this is guaranteed to work but i'm just trying to break it
    lhs.set(false);
    unused.set(700);
    assert_eq!(o2.value(), Ok(Err(ObserverError::CurrentlyStabilising)));
    incr.stabilise();
    assert_eq!(o2.value(), Ok(Err(ObserverError::CurrentlyStabilising)));
    unrelated.set(99);
    incr.stabilise();
    assert_eq!(o2.value(), Ok(Err(ObserverError::CurrentlyStabilising)));
}

#[test]
fn enumerate() {
    let g = CallCounter::new("g");
    let g_ = g.clone();
    let incr = IncrState::new();
    let v = incr.var("first");
    let m = v.enumerate(move |i, &x| {
        g_.increment();
        x
    });
    let o = m.observe();
    assert_eq!(*g, 0);
    incr.stabilise();
    assert_eq!(*g, 1);

    v.set("second");
    v.set("second again");
    assert_eq!(*g, 1);
    incr.stabilise();
    assert_eq!(*g, 2);

    v.set("third");
    assert_eq!(*g, 2);
    incr.stabilise();
    assert_eq!(*g, 3);
    // ensure observer is alive to keep the map function running
    drop(o);
}

// #[test]
// fn map_mutable() {
//     let mut global = String::new();
//     map_mutable_inner(&mut global);
//     assert_eq!(global, "abcde");
// }

// fn map_mutable_inner(global: &mut String) {
//     // this is just for fun
//     struct Computation {
//         incr: IncrState,
//         setter: Var<&'static str>,
//         #[allow(dead_code)]
//         total_len: Observer<usize>,
//     }
//     let incr = IncrState::new();
//     let setter = incr.var("a");
//     let total_len = setter
//         .map(|s| {
//             global.push_str(s);
//             global.len()
//         })
//         .observe();
//     let c = Computation {
//         incr: incr.clone(),
//         setter,
//         total_len,
//     };
//     c.incr.stabilise();
//     c.setter.set("ignored");
//     c.setter.set("bcde");
//     c.incr.stabilise();
//     assert_eq!(c.total_len.expect_value(), 5);
// }

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
    assert_eq!(o.expect_value(), "hello");
    v.set(", world");
    incr.stabilise();
    assert_eq!(o.expect_value(), "hello, world");
}

#[test]
fn fold() {
    let incr = IncrState::new();
    let vars = [incr.var(10), incr.var(20), incr.var(30)];
    let watches = vars.iter().map(Var::watch).collect();
    let sum = incr.fold(watches, 0, |acc, x| acc + x);
    let obs = sum.observe();
    incr.stabilise();
    assert_eq!(obs.expect_value(), 60);
    vars[0].set(30);
    vars[0].set(40);
    incr.stabilise();
    assert_eq!(obs.expect_value(), 90);
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
    assert_eq!(obs.value(), Ok(60));

    v1.set(40);
    incr.stabilise();
    obs.save_dot_to_file("bind_fold.dot");

    assert_eq!(obs.value(), Ok(90));
    vars.set(vec![v1.clone()]);
    incr.stabilise();
    assert_eq!(obs.value(), Ok(40));
}

#[test]
fn unordered_fold() {
    let incr = IncrState::new();

    let v1 = incr.var(10i32);
    let v2 = incr.var(20);
    let v3 = incr.var(30);

    let vars = incr.var(vec![v1.clone(), v2, v3]);

    let sum = vars.binds(|incr, vars| {
        let watches = vars.iter().map(Var::watch).collect();
        incr.unordered_fold(
            watches,
            0,
            |acc, x| acc + x,
            |acc, old, new| acc + new - old,
            None,
        )
    });

    let obs = sum.observe();
    incr.stabilise();
    obs.save_dot_to_file("unordered_fold-1.dot");
    assert_eq!(obs.value(), Ok(60));

    v1.set(40);
    incr.stabilise();
    obs.save_dot_to_file("unordered_fold-2.dot");

    assert_eq!(obs.value(), Ok(90));
    vars.set(vec![v1.clone()]);
    incr.stabilise();
    obs.save_dot_to_file("unordered_fold-3.dot");
    assert_eq!(obs.value(), Ok(40));

    vars.set(vec![]);
    incr.stabilise();
    obs.save_dot_to_file("unordered_fold-4.dot");
}

#[derive(Debug)]
struct CallCounter(&'static str, Cell<u32>);

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
    fn wrap1<A, R>(self: Rc<Self>, mut f: impl (FnMut(&A) -> R) + Clone) -> impl FnMut(&A) -> R + Clone {
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

#[test]
fn unordered_fold_inverse() {
    let f = CallCounter::new("f");
    let finv = CallCounter::new("finv");
    let incr = IncrState::new();
    let v1 = incr.var(10i32);
    let v2 = incr.var(20);
    let v3 = incr.var(30);
    let vars = incr.var(vec![v1.clone(), v2.clone(), v3.clone()]);
    let f_ = f.clone();
    let finv_ = finv.clone();
    let sum = vars.binds(move |incr, vars| {
        let f_ = f_.clone();
        let finv_ = finv_.clone();
        let watches: Vec<_> = vars.iter().map(Var::watch).collect();
        incr.unordered_fold_inverse(
            watches,
            0,
            move |acc, x| {
                f_.increment();
                acc + x
            },
            move |acc, x| {
                finv_.increment();
                acc - x
            }, // this time our update function is
            // constructed for us.
            None,
        )
    });
    let obs = sum.observe();
    incr.stabilise();
    assert_eq!(obs.value(), Ok(60));
    assert_eq!(*f, 3);
    assert_eq!(*finv, 0);

    v1.set(40);
    incr.stabilise();
    assert_eq!(obs.value(), Ok(90));
    assert_eq!(*f, 4);
    assert_eq!(*finv, 1);

    vars.set(vec![v1.clone()]);
    incr.stabilise();
    assert_eq!(obs.value(), Ok(40));
    assert_eq!(*f, 5);
    // we create a new UnorderedArrayFold in the bind.
    // Hence finv was not needed, fold_value was zero
    // and we just folded the array non-incrementally
    assert_eq!(*finv, 1);
}

#[test]
fn incr_map_uf() {
    let incr = IncrState::new();
    let mut b = BTreeMap::new();
    b.insert("five", 5);
    b.insert("eight", 8);

    let setter = incr.var(b.clone());

    // let list = vec![ 1, 2, 3 ];
    // let sum = list.into_iter().fold(0, |acc, x| acc + x);

    let sum =
        setter.incr_unordered_fold(0i32, |acc, _, new| acc + new, |acc, _, old| acc - old, true);

    let o = sum.observe();

    incr.stabilise();
    assert_eq!(o.value(), Ok(13));

    b.remove("five");
    setter.set(b.clone());
    incr.stabilise();
    assert_eq!(o.value(), Ok(8));

    b.insert("five", 100);
    setter.set(b.clone());
    incr.stabilise();
    assert_eq!(o.value(), Ok(108));

    b.insert("five", 105);
    setter.set(b.clone());
    incr.stabilise();
    assert_eq!(o.value(), Ok(113));
    o.save_dot_to_file("incr_map_uf.dot");
}

#[test]
fn incr_map_filter_mapi() {
    let incr = IncrState::new();
    let mut b = BTreeMap::new();
    b.insert("five", 5);
    b.insert("ten", 10);
    let v = incr.var(b);
    let filtered = v
        .incr_filter_mapi(|&k, &v| Some(v).filter(|_| k.len() > 3))
        .observe();
    incr.stabilise();
    let x = IntoIterator::into_iter([("five", 5i32)]);
    assert_eq!(filtered.expect_value(), x.collect());
}

#[test]
fn incr_map_primes() {
    let primes = primes_lt(1_000_000);
    let incr = IncrState::new();
    let mut b = BTreeMap::new();
    b.insert("five", 5);
    b.insert("seven", 7);
    b.insert("ten", 10);
    let v = incr.var(b.clone());
    let filtered = v
        .incr_filter_map(move |&v| Some(v).filter(|x| dbg!(is_prime(*dbg!(x), &primes))))
        .observe();

    incr.stabilise();
    let x = BTreeMap::from([("five", 5), ("seven", 7)]);
    assert_eq!(filtered.expect_value(), x);

    b.remove("seven");
    b.insert("971", 971);
    v.set(b.clone());
    incr.stabilise();
    let x = BTreeMap::from([("971", 971), ("five", 5)]);
    assert_eq!(filtered.expect_value(), x);
}

// https://gist.github.com/glebm/440bbe2fc95e7abee40eb260ec82f85c
fn is_prime(n: usize, primes: &Vec<usize>) -> bool {
    for &p in primes {
        let q = n / p;
        if q < p {
            return true;
        };
        let r = n - q * p;
        if r == 0 {
            return false;
        };
    }
    panic!("too few primes")
}
fn primes_lt(bound: usize) -> Vec<usize> {
    let mut primes: Vec<bool> = (0..bound + 1).map(|num| num == 2 || num & 1 != 0).collect();
    let mut num = 3usize;
    while num * num <= bound {
        let mut j = num * num;
        while j <= bound {
            primes[j] = false;
            j += num;
        }
        num += 2;
    }
    primes
        .into_iter()
        .enumerate()
        .skip(2)
        .filter_map(|(i, p)| if p { Some(i) } else { None })
        .collect::<Vec<usize>>()
}

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
    assert_eq!(o.value(), Ok(10));
    incr.stabilise();
    assert_eq!(o.value(), Ok(20));
    incr.stabilise();
    assert_eq!(o.value(), Ok(30));
}

#[test]
fn var_update_simple() {
    let incr = IncrState::new();
    let var = incr.var(10);
    let o = var.observe();
    incr.stabilise();
    assert_eq!(o.value(), Ok(10));
    var.update(|x| *x += 10);
    incr.stabilise();
    assert_eq!(o.value(), Ok(20));
}

#[test]
fn var_update_during_stabilisation() {
    let incr = IncrState::new();
    let var = incr.var(10);
    let var_ = var.clone();
    let o = var
        .map(move |&x| {
            var_.update(|y| *y += 1);
            x
        })
        .observe();
    incr.stabilise();
    assert_eq!(o.value(), Ok(10));
    incr.stabilise();
    assert_eq!(o.value(), Ok(11));
    var.set(5);
    incr.stabilise();
    assert_eq!(o.value(), Ok(5));
    incr.stabilise();
    assert_eq!(o.value(), Ok(6));
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
    var.update(|x| *x += 1);
    incr.stabilise();
    assert_eq!(o.value(), Ok(11));
    incr.stabilise();
    assert_eq!(o.value(), Ok(99));
}

#[test]
fn test_constant() {
    let incr = IncrState::new();
    let c = incr.constant(5);
    let m = c.map(|x| x + 10);
    let o = m.observe();
    incr.stabilise();
    assert_eq!(o.value(), Ok(15));
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
    assert_eq!(o2.value(), Ok(5));
    flip.set(!flip.get());
    incr.stabilise();
    assert_eq!(o2.value(), Ok(10));
    o2.save_dot_to_file("test_constant.dot");
}

#[test]
#[should_panic = "assertion failed: Rc::ptr_eq"]
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
    v.set_cutoff(Cutoff::Custom(Rc::ptr_eq));

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
    assert_eq!(o.value(), Ok(6));

    var.set(Rc::new(vec![1, 2, 3, 4]));
    incr.stabilise();
    assert_eq!(*sum_counter, 2);
    assert_eq!(o.value(), Ok(10));
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
    assert_eq!(o.value(), Ok(16));

    v.set(vec![6]);
    incr.stabilise();
    assert_eq!(*add10_counter, 1); // the crux
    assert_eq!(o.value(), Ok(16));

    v.set(vec![9, 1]);
    incr.stabilise();
    assert_eq!(*add10_counter, 2); // the crux again
    assert_eq!(o.value(), Ok(20));
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
    assert_eq!(o.expect_value().to_string(), "100".to_string());
    v.set(200);
    incr.stabilise();
    assert_eq!(o.expect_value().to_string(), "200".to_string());
}

#[test]
fn observer_subscribe_drop() {
    let call_count = CallCounter::new("subscriber");
    let call_count_ = call_count.clone();
    let incr = IncrState::new();
    let var = incr.var(10);
    let observer = var.observe();
    observer
        .subscribe(move |value| {
            tracing::info!("received update: {:?}", value);
            call_count_.increment();
        })
        .unwrap();
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
    let token = observer
        .subscribe(move |value| {
            tracing::debug!("received update: {:?}", value);
            call_count_.increment();
        })
        .unwrap();
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
    let token = observer
        .subscribe(|value| {
            tracing::debug!("received update: {:?}", value);
        })
        .unwrap();
    let second = observer.subscribe(|_| ()).unwrap();
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

#[test]
fn incr_map_rc() {
    let counter = CallCounter::new("mapper");
    let counter_ = counter.clone();
    let incr = IncrState::new();
    let rc = Rc::new(BTreeMap::from([(5, "hello"), (10, "goodbye")]));
    let var = incr.var(rc);
    let observer = var
        .incr_mapi(move |&_k, &v| {
            counter_.increment();
            v.to_string() + ", world"
        })
        .observe();
    incr.stabilise();
    assert_eq!(*counter, 2);
    let greetings = observer.expect_value();
    assert_eq!(greetings.get(&5), Some(&String::from("hello, world")));
    incr.stabilise();
    assert_eq!(*counter, 2);
    let rc = Rc::new(BTreeMap::from([(10, "changed")]));
    // we've saved ourselves some clones already
    // (from the Var to its watch node, for example)
    var.set(rc);
    incr.stabilise();
    assert_eq!(*counter, 3);
}

#[test]
fn incr_filter_mapi() {
    let counter = CallCounter::new("mapper");
    let incr = IncrState::new();
    let rc = Rc::new(BTreeMap::from([(5, "hello"), (10, "goodbye")]));
    let var = incr.var(rc);
    let counter_ = counter.clone();
    let observer = var
        .incr_filter_mapi(move |&k, &v| {
            counter_.increment();
            if k < 10 {
                return None;
            }
            Some(v.to_string() + ", world")
        })
        .observe();
    incr.stabilise();
    assert_eq!(*counter, 2);
    let greetings = observer.expect_value();
    tracing::debug!("greetings were: {greetings:?}");
    assert_eq!(greetings.get(&5), None);
    assert_eq!(greetings.get(&10), Some(&"goodbye, world".to_string()));
    incr.stabilise();
    assert_eq!(*counter, 2);
    let rc = Rc::new(BTreeMap::from([(10, "changed")]));
    // we've saved ourselves some clones already
    // (from the Var to its watch node, for example)
    var.set(rc);
    incr.stabilise();
    assert_eq!(*counter, 3);
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
    assert_eq!(obs.value(), Ok(5));

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
    assert_eq!(obs.value(), Ok(10));

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

#[test]
fn test_map_ref() {
    #[derive(PartialEq, Clone, Debug)]
    struct Thing {
        string: String,
    }
    let incr = IncrState::new();
    let var = incr.var(Thing { string: "hello".into() });
    let str = var
        .map_ref(|thing| thing.string.as_str());
}

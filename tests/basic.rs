#[cfg(test)]
use test_log::test;

use std::{cell::Cell, collections::BTreeMap, rc::Rc};

use incremental::{Cutoff, Observer, ObserverError, State, Var};

#[test]
fn testit() {
    let incr = State::new();
    let var = incr.var(5);
    var.set(10);
    let observer = var.watch().observe();
    incr.stabilise();
    assert_eq!(observer.value(), Ok(10));
}

#[test]
fn test_map() {
    let incr = State::new();
    let var = incr.var(5);
    let watch = var.watch();
    let mapped = watch.map(|x| dbg!(dbg!(x) * 10));
    let observer = mapped.observe();
    incr.stabilise();
    assert_eq!(observer.value(), Ok(50));
    var.set(3);
    incr.stabilise();
    assert_eq!(observer.value(), Ok(30));
}

#[test]
fn test_map2() {
    let incr = State::new();
    let a = incr.var(5);
    let b = incr.var(8);
    let a_ = a.watch();
    let b_ = b.watch();
    let mapped = a_.map2(&b_, |a, b| dbg!(dbg!(a) + dbg!(b)));
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
    let incr = State::new();
    let a = incr.var(5);
    let b = incr.var(8);
    let a_ = a.watch();
    let b_ = b.watch();
    let map_left = a_.map(|a| dbg!(a * 10));
    let mapped = map_left.map2(&b_, |left, b| dbg!(dbg!(left) + dbg!(b)));
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
    let incr = State::new();
    let choose = incr.var(Choose::B);
    let b = incr.var(5);
    let c = incr.var(10);
    let b_ = b.watch();
    let c_ = c.watch();
    let bound = choose
        .watch()
        // closures gotta be 'static
        // we move b_ & c_ into the closure. and we clone one of them each time a changes
        .bind(move |choose| match dbg!(choose) {
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
}

/// Exercisees Adjust_heights_heap for a variable that was created from scratch inside a bind
#[test]
fn create_var_in_bind() {
    let incr = State::new();
    let lhs = incr.var(true);
    let state = incr.clone();
    let o = lhs
        .watch()
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
    let incr = State::new();
    let is_short = incr.var(true);
    // this is a very short graph, height 1.
    let short_graph = incr.var(5).watch();
    // this is a very short graph, height 4.
    let long_graph = incr.var(10).watch().map(|x| x).map(|x| x).map(|x| x);
    let obs = is_short
        .watch()
        // BindMain's height is initially bumped to 3 (short=1, lhs=2, main=3)
        .bind(move |s| {
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
    let incr = State::new();
    let is_short = incr.var(true);
    let i2 = incr.clone();
    let obs = is_short
        .watch()
        .bind(move |s| {
            if s {
                i2.var(5).watch()
            } else {
                i2.var(10).watch().map(|x| x).map(|x| x).map(|x| x)
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
    let incr = State::new();
    let lhs = incr.var(true);
    let unused = incr.var(2);
    let unused_w = unused.watch();
    let state = incr.clone();
    let v9 = state.var(9);
    let bound = lhs
        .watch()
        .bind(move |l| if l { v9.watch() } else { unused_w.clone() });
    let o = bound.observe();

    incr.stabilise();
    assert_eq!(o.value(), Ok(9));

    let unrelated = incr.var(100);
    // make sure to add unrelated to recompute heap first
    let _o = unrelated.watch().observe();

    let var = incr.var(10);
    let o1 = var.watch().map(|a| a + 10).observe();
    assert_eq!(o1.value(), Err(ObserverError::NeverStabilised));

    let map = unrelated.watch().map(move |_x| o.value());
    let o2 = map.observe();

    incr.stabilise();
    // ok none of this is guaranteed to work but i'm just trying to break it
    lhs.set(false);
    unused.set(700);
    assert_eq!(o2.value(), Ok(Ok(9)));
    incr.stabilise();
    assert_eq!(o2.value(), Ok(Ok(9)));
    unrelated.set(99);
    incr.stabilise();
    assert_eq!(o2.value(), Ok(Ok(700)));
}

#[test]
fn enumerate() {
    let g = std::cell::Cell::new(0);
    let incr = State::new();
    let v = incr.var("first");
    let m = v.watch().enumerate().map(|(i, x)| {
        g.set(i + 1);
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
fn map_mutable() {
    let mut global = String::new();
    map_mutable_inner(&mut global);
    assert_eq!(global, "abcde");
}

fn map_mutable_inner(global: &mut String) {
    // this is just for fun
    struct Computation<'a> {
        incr: Rc<State<'a>>,
        setter: Var<'a, &'static str>,
        #[allow(dead_code)]
        total_len: Observer<'a, usize>,
    }

    let incr = State::new();
    let setter = incr.var("a");
    let total_len = setter
        .watch()
        .map(|s| {
            global.push_str(s);
            global.len()
        })
        .observe();
    let c = Computation {
        incr: incr.clone(),
        setter,
        total_len,
    };

    c.incr.stabilise();
    c.setter.set("ignored");
    c.setter.set("bcde");
    c.incr.stabilise();
    assert_eq!(c.total_len.expect_value(), 5);
}

#[test]
fn mutable_string() {
    let incr = State::new();
    let v = incr.var("hello");
    let mut buf = String::new();
    let o = v
        .watch()
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
    let incr = State::new();
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
    let incr = State::new();
    let v1 = incr.var(10i32);
    let v2 = incr.var(20);
    let v3 = incr.var(30);
    let vars = incr.var(vec![v1.clone(), v2.clone(), v3.clone()]);
    let sum = vars.watch().binds(|incr, vars| {
        let watches: Vec<_> = vars.iter().map(Var::watch).collect();
        incr.fold(watches, 0, |acc, x| acc + x)
    });
    let obs = sum.observe();
    incr.stabilise();
    assert_eq!(obs.value(), Ok(60));
    v1.set(40);
    incr.stabilise();
    assert_eq!(obs.value(), Ok(90));
    vars.set(vec![v1.clone()]);
    incr.stabilise();
    assert_eq!(obs.value(), Ok(40));
}

#[test]
fn unordered_fold() {
    let incr = State::new();
    let v1 = incr.var(10i32);
    let v2 = incr.var(20);
    let v3 = incr.var(30);
    let vars = incr.var(vec![v1.clone(), v2.clone(), v3.clone()]);
    let sum = vars.watch().binds(|incr, vars| {
        let watches: Vec<_> = vars.iter().map(Var::watch).collect();
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
    assert_eq!(obs.value(), Ok(60));
    v1.set(40);
    incr.stabilise();
    assert_eq!(obs.value(), Ok(90));
    vars.set(vec![v1.clone()]);
    incr.stabilise();
    assert_eq!(obs.value(), Ok(40));
}

#[derive(Debug)]
struct CallCounter(&'static str, Cell<u32>);

#[allow(dead_code)]
impl CallCounter {
    fn new(name: &'static str) -> Self {
        Self(name, Cell::new(0))
    }
    fn count(&self) -> u32 {
        self.1.get()
    }
    fn increment(&self) {
        self.1.set(self.1.get() + 1);
    }
    fn wrap1<'a, A, R>(
        &'a self,
        mut f: impl (FnMut(A) -> R) + 'a + Clone,
    ) -> impl FnMut(A) -> R + 'a + Clone {
        move |a| {
            self.increment();
            f(a)
        }
    }
    fn wrap2<'a, A, B, R>(
        &'a self,
        mut f: impl (FnMut(A, B) -> R) + 'a + Clone,
    ) -> impl FnMut(A, B) -> R + 'a + Clone {
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
    let incr = State::new();
    let v1 = incr.var(10i32);
    let v2 = incr.var(20);
    let v3 = incr.var(30);
    let vars = incr.var(vec![v1.clone(), v2.clone(), v3.clone()]);
    let sum = vars.watch().binds(|incr, vars| {
        let watches: Vec<_> = vars.iter().map(Var::watch).collect();
        incr.unordered_fold_inverse(
            watches,
            0,
            f.wrap2(|acc, x| acc + x),
            finv.wrap2(|acc, x| acc - x), // this time our update function is
            // constructed for us.
            None,
        )
    });
    let obs = sum.observe();
    incr.stabilise();
    assert_eq!(obs.value(), Ok(60));
    assert_eq!(f, 3);
    assert_eq!(finv, 0);

    v1.set(40);
    incr.stabilise();
    assert_eq!(obs.value(), Ok(90));
    assert_eq!(f, 4);
    assert_eq!(finv, 1);

    vars.set(vec![v1.clone()]);
    incr.stabilise();
    assert_eq!(obs.value(), Ok(40));
    assert_eq!(f, 5);
    // we create a new UnorderedArrayFold in the bind.
    // Hence finv was not needed, fold_value was zero
    // and we just folded the array non-incrementally
    assert_eq!(finv, 1);
}

#[test]
fn incr_map_uf() {
    let incr = State::new();
    let mut b = BTreeMap::new();
    b.insert("five", 5);
    b.insert("eight", 8);

    let setter = incr.var(b.clone());

    // let list = vec![ 1, 2, 3 ];
    // let sum = list.into_iter().fold(0, |acc, x| acc + x);

    let sum = incr.btreemap_unordered_fold(
        setter.watch(),
        0i32,
        |acc, _, new| acc + new,
        |acc, _, old| acc - old,
        true,
    );

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
}

#[test]
fn incr_map_filter_mapi() {
    let incr = State::new();
    let mut b = BTreeMap::new();
    b.insert("five", 5);
    b.insert("ten", 10);
    let v = incr.var(b);
    let filtered = incr
        .btreemap_filter_mapi(v.watch(), |&k, &v| Some(v).filter(|_| k.len() > 3))
        .observe();
    incr.stabilise();
    let x = IntoIterator::into_iter([("five", 5i32)]);
    assert_eq!(filtered.expect_value(), x.collect());
}

#[test]
fn incr_map_primes() {
    let primes = primes_lt(1_000_000);
    let incr = State::new();
    let mut b = BTreeMap::new();
    b.insert("five", 5);
    b.insert("seven", 7);
    b.insert("ten", 10);
    let v = incr.var(b.clone());
    let filtered = incr
        .btreemap_filter_mapi(v.watch(), |_k, &v| {
            Some(v).filter(|x| dbg!(is_prime(*dbg!(x), &primes)))
        })
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
    let incr = State::new();
    let v = incr.var(10_i32);
    let o = v
        .watch()
        .map(move |x| {
            v.set(x + 10);
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
fn test_constant() {
    let incr = State::new();
    let c = incr.constant(5);
    let m = c.map(|x| x + 10);
    let o = m.observe();
    incr.stabilise();
    assert_eq!(o.value(), Ok(15));
    let flip = incr.var(true);
    let o2 = flip
        .watch()
        .binds(|incr, t| {
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
}

#[test]
#[should_panic = "assertion failed: Rc::ptr_eq"]
fn two_worlds() {
    let one = State::new();
    let two = State::new();
    let v2 = two.var(9);
    let v2_ = v2.clone();
    let _o = one.constant(5).bind(move |_| v2_.watch()).observe();
    let _o2 = v2.watch().observe();
    two.stabilise();
    one.stabilise();
}

#[test]
fn cutoff_rc_ptr_eq() {
    let sum_counter = CallCounter::new("map on rc");
    let incr = State::new();
    let rc: Rc<Vec<i32>> = Rc::new(vec![1i32, 2, 3]);
    let v = incr.var(rc.clone());
    // Rc::ptr_eq is a fn(&T, &T) -> bool for any T.
    // Note that v.watch() always returns the same node. So you can mutate its cutoff.
    v.watch().set_cutoff(Cutoff::Custom(Rc::ptr_eq));
    // alternatively, this way. No difference, just this one works inline/returns self
    let w = v.watch().cutoff(Cutoff::Custom(Rc::ptr_eq));
    let c = &sum_counter;
    let o = w
        .map(|list| {
            c.increment();
            list.iter().sum::<i32>()
        })
        .observe();

    incr.stabilise();
    assert_eq!(sum_counter, 1);

    // We didn't change the Rc that was returned. So map should not have to call its function again.
    v.set(rc.clone());
    incr.stabilise();
    assert_eq!(sum_counter, 1);
    assert_eq!(o.value(), Ok(6));

    v.set(Rc::new(vec![1, 2, 3, 4]));
    incr.stabilise();
    assert_eq!(sum_counter, 2);
    assert_eq!(o.value(), Ok(10));
}

#[test]
fn cutoff_sum() {
    let add10_counter = CallCounter::new("sum + 10");
    let incr = State::new();
    let vec = vec![1i32, 2, 3];
    let v = incr.var(vec);
    let o = v
        .watch()
        .map(|xs| xs.into_iter().fold(0i32, |acc, x| acc + x))
        .cutoff(Cutoff::PartialEq) // this is the default.
        // because our first map node checks PartialEq before changing
        // its output value, the second map node will not have to run.
        .map(add10_counter.wrap1(|sum| sum + 10))
        .observe();

    incr.stabilise();
    assert_eq!(add10_counter, 1);
    assert_eq!(o.value(), Ok(16));

    v.set(vec![6]);
    incr.stabilise();
    assert_eq!(add10_counter, 1); // the crux
    assert_eq!(o.value(), Ok(16));

    v.set(vec![9, 1]);
    incr.stabilise();
    assert_eq!(add10_counter, 2); // the crux again
    assert_eq!(o.value(), Ok(20));
}

use incremental::{ObserverError, State};

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
    #[derive(Debug, Clone)]
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

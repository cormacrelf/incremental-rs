use incremental::State;

#[test]
fn testit() {
    let incr = State::new();
    let var = incr.var(5);
    var.set(10);
    let observer = var.watch().observe();
    incr.stabilise();
    assert_eq!(observer.value(), 10);
}

#[test]
fn test_map() {
    let incr = State::new();
    let var = incr.var(5);
    let watch = var.watch();
    let mapped = watch.map(|x| dbg!(dbg!(x) * 10));
    let observer = mapped.observe();
    incr.stabilise();
    assert_eq!(observer.value(), 50);
    var.set(3);
    incr.stabilise();
    assert_eq!(observer.value(), 30);
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
    assert_eq!(observer.value(), 13);
    // each of these queues up the watch node into the recompute heap.
    a.set(3);
    b.set(9);
    // during stabilisation the map node gets inserted into the recompute heap
    incr.stabilise();
    assert_eq!(observer.value(), 12);
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
    assert_eq!(observer.value(), 58);
    // each of these queues up the watch node into the recompute heap.
    a.set(3);
    b.set(9);
    // during stabilisation the map node gets inserted into the recompute heap
    incr.stabilise();
    assert_eq!(observer.value(), 39);
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
    assert_eq!(dbg!(obs.value()), 5);

    println!("");
    println!("------------------------------------------------------------------");
    println!("Modifying b, which is currently bound and should cause observable changes");
    println!("------------------------------------------------------------------");
    b.set(50);
    incr.stabilise();
    assert_eq!(dbg!(obs.value()), 50);

    println!("");
    println!("------------------------------------------------------------------");
    println!("Modifying c, which is NOT currently bound, so no change");
    println!("------------------------------------------------------------------");
    c.set(99);
    incr.stabilise();
    assert_eq!(dbg!(obs.value()), 50);

    println!("");
    println!("------------------------------------------------------------------");
    println!("Changing bind's LHS will swap out b for c in the bind");
    println!("So the bind function will be re-evaluated");
    println!("------------------------------------------------------------------");
    choose.set(Choose::C);
    incr.stabilise();
    assert_eq!(dbg!(obs.value()), 99);
}

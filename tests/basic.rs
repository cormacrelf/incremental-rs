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
            println!("--------------------------------");
            println!("created var in bind with id {:?}", v.id());
            println!("--------------------------------");
            v.watch()
        })
        .observe();
    incr.stabilise();
    assert_eq!(o.value(), Ok(9));
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

use incremental::{Incr, IncrState, Observer, Var};
use std::time::Instant;

const NODE_COUNT: u64 = 50;
const ITER_COUNT: u64 = 500000;

fn using_map(mut node: Incr<u64>) -> Incr<u64> {
    for _ in 0..NODE_COUNT {
        node = node.map(|val| val + 1);
    }
    node
}

fn using_bind(mut node: Incr<u64>) -> Incr<u64> {
    for _ in 0..NODE_COUNT {
        node = node.binds(move |incr, &val| incr.constant(val + 1));
    }
    node
}

fn main() {
    let incr = IncrState::new_with_height(1200);
    let first_num = incr.var(0u64);
    let o = first_num.pipe(using_map).observe();
    incr.stabilise();
    // o.save_dot_to_file("stabilise_100.dot");
    assert_eq!(o.value(), Ok(NODE_COUNT));

    let prev_stats = incr.stats();
    let start = Instant::now();
    iter(o, &incr, first_num);
    let dur = Instant::now().duration_since(start);
    let recomputed = (incr.stats() - prev_stats).recomputed;

    println!();
    let expect_total = (NODE_COUNT * ITER_COUNT) as u32;
    println!(
        "recompute count {recomputed} ({:.2}x NODE_COUNT * ITER_COUNT)",
        recomputed as f32 / expect_total as f32
    );
    println!("{:?} per node", dur / recomputed as u32);
}

#[inline(never)]
fn iter(node: Observer<u64>, incr: &IncrState, set_first_num: Var<u64>) {
    let mut update_number = 0;
    for i in 0..ITER_COUNT {
        if i % (ITER_COUNT / 10) == 0 {
            println!("{}%", (i * 100) / (ITER_COUNT));
        }
        update_number += 1;
        set_first_num.set(update_number);
        incr.stabilise();
        assert_eq!(node.value(), Ok(update_number + NODE_COUNT));
    }
}

use std::hint::black_box;
use std::time::Instant;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

use incremental::{Incr, IncrState, Observer, Var};

#[derive(Clone, Copy)]
enum SequenceKind {
    Trivial,
    TrivialBind,
    Spokes,
    Recombine,
    Wide,
}

fn sequence(node: Incr<u64>, kind: SequenceKind, size: u64) -> Incr<u64> {
    match kind {
        SequenceKind::Trivial => (0..size)
            .into_iter()
            .fold(node, |node, _| node.map(|val| val + 1)),
        SequenceKind::TrivialBind => (0..size).into_iter().fold(node, |node, _| {
            node.binds(|incr, &val| incr.constant(val + 1))
        }),
        SequenceKind::Spokes => {
            let all = (0..size)
                .into_iter()
                .map(|_| node.map(|val| val + 1))
                .collect::<Vec<_>>();
            node.state().fold(all, 0, |a, b| a + b)
        }
        SequenceKind::Recombine => (0..size).into_iter().fold(node, |node, _| {
            let a = node.map(|x| x + 1);
            let b = node.map(|x| x + 1);
            (a % b).map(|a, b| a + b)
        }),
        SequenceKind::Wide => {
            let double = |list: Vec<Incr<u64>>| -> Vec<Incr<_>> {
                list.into_iter()
                    .flat_map(|i| {
                        let a = i.map(|x| x + 1);
                        let b = i.map(|x| x + 1);
                        [a, b]
                    })
                    .collect()
            };
            let spread = (0..size).into_iter().fold(vec![node], |acc, _| double(acc));
            reduce_balanced(&spread, |a, b| a.map2(b, |a, b| a + b)).unwrap()
        }
    }
}

fn reduce_balanced<T: Clone>(slice: &[T], mut f: impl FnMut(&T, &T) -> T) -> Option<T> {
    fn reduce_balanced_inner<T: Clone>(slice: &[T], f: &mut impl FnMut(&T, &T) -> T) -> Option<T> {
        let size = slice.len();
        if size == 0 {
            return None;
        }
        if size == 1 {
            return slice.first().cloned();
        }
        let mid = size / 2;
        let left = reduce_balanced_inner(&slice[..mid], f).unwrap();
        let right = reduce_balanced_inner(&slice[mid..], f).unwrap();
        Some(f(&left, &right))
    }
    reduce_balanced_inner(slice, &mut f)
}

#[test]
fn test_reduce_balanced() {
    let slice: &[i32] = &[];
    assert_eq!(reduce_balanced(slice, |a, b| a + b), None);
    let slice = &[1];
    assert_eq!(reduce_balanced(slice, |a, b| a + b), Some(1));
    let slice = &[1, 2];
    assert_eq!(reduce_balanced(slice, |a, b| a + b), Some(3));
    let slice = &[1, 2, 3, 4];
    assert_eq!(reduce_balanced(slice, |a, b| a + b), Some(10));
    let slice = &[1, 2, 3, 4, 5];
    assert_eq!(reduce_balanced(slice, |a, b| a + b), Some(15));
}

fn setup(kind: SequenceKind, size: u64) -> (Var<u64>, IncrState, Observer<u64>, u32) {
    let incr = IncrState::new_with_height(1200);
    let first_num = incr.var(1u64);
    let o = first_num.pipe2(sequence, kind, size).observe();
    incr.stabilise();
    first_num.set(0);
    let stats = incr.stats();
    incr.stabilise();
    let recomputed = incr.stats().diff(stats).recomputed as u32;
    (first_num, incr, o, recomputed)
}

impl From<SequenceKind> for String {
    fn from(value: SequenceKind) -> Self {
        match value {
            SequenceKind::Wide => "wide",
            SequenceKind::Trivial => "trivial",
            SequenceKind::TrivialBind => "trivial-bind",
            SequenceKind::Spokes => "spokes",
            SequenceKind::Recombine => "recombine",
        }
        .into()
    }
}

fn bench_node(c: &mut Criterion, kind: SequenceKind, size: u64) {
    c.bench_with_input(BenchmarkId::new(kind, size), &size, |b, &size| {
        let (var, incr, obs, _recomputed) = setup(kind, size);
        b.iter_custom(|iters| {
            let start = Instant::now();
            let recomputed = incr.stats().recomputed;
            for _ in 0..iters {
                var.update(|x| x + 1);
                incr.stabilise();
                black_box(obs.value());
            }
            let diff = incr.stats().recomputed - recomputed;
            iters as u32 * start.elapsed() / diff as u32
        })
    });
}

fn bench_stabilise(c: &mut Criterion, kind: SequenceKind, size: u64) {
    c.bench_with_input(BenchmarkId::new("stabilise", size), &size, |b, &size| {
        let (_var, incr, obs, _recomputed) = setup(kind, size);
        b.iter(|| {
            incr.stabilise();
            black_box(obs.value());
        })
    });
}

#[tracing::instrument(skip_all)]
fn all(c: &mut Criterion) {
    tracing_subscriber::fmt().init();
    bench_node(c, SequenceKind::Recombine, 50);
    bench_node(c, SequenceKind::Trivial, 1000);
    bench_node(c, SequenceKind::TrivialBind, 50);
    bench_node(c, SequenceKind::Spokes, 1000);
    bench_node(c, SequenceKind::Wide, 5);
    bench_node(c, SequenceKind::Wide, 10);
    bench_stabilise(c, SequenceKind::Recombine, 50);
}

criterion_group!(benches, all);
criterion_main!(benches);

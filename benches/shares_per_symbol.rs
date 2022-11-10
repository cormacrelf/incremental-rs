use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};

use im_rc::{OrdMap, ordmap::Entry};
use incremental::{Incr, Value, IncrState};
use tracing::Level;
use tracing_subscriber::fmt::format::FmtSpan;

#[derive(Copy, Clone, PartialEq, Debug)]
enum Dir {
    Buy,
    Sell,
}

type Symbol = u32;
type Oid = u32;


#[derive(Clone, PartialEq)]
struct Order {
    // OCaml strings are GC'd.
    id: Oid,
    price: f32,
    size: u32,
    sym: Symbol,
    dir: Dir,
}
impl std::fmt::Debug for Order {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Order")
            .field(&self.id)
            .field(&self.dir)
            .field(&self.price)
            .field(&self.size)
            .field(&self.sym)
            .finish()
    }
}

fn index_by<KInner: Value + Ord, KOuter: Value + Ord, V: Value>(
    map: Incr<OrdMap<KInner, V>>,
    get_outer_index: impl Fn(&V) -> KOuter + Clone + 'static,
) -> Incr<OrdMap<KOuter, OrdMap<KInner, V>>> {
    let get_outer_index_ = get_outer_index.clone();
    let add = move |mut acc: OrdMap<KOuter, OrdMap<KInner, V>>, key_inner: &KInner, value: &V| {
        let index = get_outer_index_(value);
        acc.entry(index)
            .or_insert_with(|| OrdMap::new())
            .insert(key_inner.clone(), value.clone());
        acc
    };
    let remove =
        move |mut acc: OrdMap<KOuter, OrdMap<KInner, V>>, key_inner: &KInner, value: &V| {
            let index = get_outer_index(value);
            match acc.entry(index) {
                Entry::Vacant(_) => panic!(),
                Entry::Occupied(mut o) => {
                    let map = o.get_mut();
                    map.remove(key_inner);
                    if map.is_empty() {
                        o.remove();
                    }
                }
            }
            acc
        };
    let indexed = map.incr_unordered_fold(OrdMap::new(), add, remove, false);
    #[cfg(debug_assertions)]
    indexed.set_graphviz_user_data("index_by");
    indexed
}

#[test]
fn test_index_by() {
    let incr = IncrState::new();
    let var = incr.var(OrdMap::<i32, String>::default());
    let o = index_by(var.watch(), |x| x.to_uppercase()).observe();
    let insert = |k: i32, val: &str| {
        var.modify(|map| {
            map.insert(k, val.to_string());
        });
        incr.stabilise();
        o.expect_value()
    };
    use im_rc::ordmap;
    assert_eq!(insert(1, "bar"), ordmap! {
        "BAR".to_string() => ordmap! {
            1i32 => "bar".to_string()
        }
    });
    assert_eq!(insert(1, "foo"), ordmap! {
        "FOO".to_string() => ordmap! {
            1i32 => "foo".to_string()
        }
    });
    assert_eq!(insert(2, "foo"), ordmap! {
        "FOO".to_string() => ordmap! {
            1i32 => "foo".to_string(),
            2i32 => "foo".to_string()
        }
    });
    assert_eq!(insert(3, "bar"), ordmap! {
        "BAR".to_string() => ordmap! {
            3i32 => "bar".to_string()
        },
        "FOO".to_string() => ordmap! {
            1i32 => "foo".to_string(),
            2i32 => "foo".to_string()
        }
    });
    assert_eq!(insert(2, "bar"), ordmap! {
        "BAR".to_string() => ordmap! {
            2i32 => "bar".to_string(),
            3i32 => "bar".to_string()
        },
        "FOO".to_string() => ordmap! {
            1i32 => "foo".to_string()
        }
    });
}

fn shares(orders: Incr<OrdMap<Oid, Order>>) -> Incr<u32> {
    orders.incr_unordered_fold(
        0,
        |acc, _k, x| acc + x.size,
        |acc, _k, x| acc - x.size,
        false,
    )
}

fn shares_per_symbol(orders: Incr<OrdMap<Oid, Order>>) -> Incr<OrdMap<Symbol, u32>> {
    orders
        .pipe1(index_by, |x| x.sym.clone())
        .incr_mapi_(|_k, v| shares(v.clone()), None)
}

fn shares_per_symbol_flat(orders: Incr<OrdMap<Oid, Order>>) -> Incr<OrdMap<Symbol, u32>> {
    fn update_sym_map(op: fn(u32, u32) -> u32) -> impl FnMut(OrdMap<Symbol, u32>, &Symbol, &Order) -> OrdMap<Symbol, u32> {
        move |mut acc, _k, o| {
            match acc.entry(_k.clone()) {
                Entry::Vacant(e) => { e.insert(o.size); }
                Entry::Occupied(mut e) => { e.insert(op(*e.get(), o.size)); }
            }
            acc
        }
    }
    orders
        .incr_unordered_fold(
            OrdMap::new(),
            update_sym_map(|a, b| a + b),
            update_sym_map(|a, b| a - b),
            false,
        )
}

fn random_order(rng: &mut impl rand::Rng) -> Order {
    let num_symbols = 100;
    let sym = rng.gen_range(0..num_symbols);
    let size = rng.gen_range(0..10_000);
    let price = rng.gen_range(0..10_000) as f32 / 100.;
    let dir = if rng.gen_ratio(1, 2) { Dir::Buy } else { Dir::Sell };
    let id = rng.gen_range(0..u32::MAX);
    Order {
        id, dir, price, size, sym
    }
}

fn random_orders(rng: &mut impl rand::Rng, n: u32) -> OrdMap<Oid, Order> {
    (0..n).into_iter().fold(OrdMap::new(), |mut acc, _| {
        let o = random_order(rng);
        acc.insert(o.id.clone(), o);
        acc
    })
}

fn setup(n: u32, sps_fn: fn(Incr<OrdMap<Oid, Order>>) -> Incr<OrdMap<Symbol, u32>>) -> impl FnMut() {
    tracing::info!("setup called");
    let mut rng = rand::thread_rng();
    let init_orders = random_orders(&mut rng, n);
    let incr = IncrState::new();
    let var = incr.var(init_orders.clone());
    let shares = var.pipe(sps_fn).observe();
    incr.stabilise();
    if n < 100 {
        shares.save_dot_to_file(&format!("shares-{}.dot", n));
    }
    move || {
        let random = random_order(&mut rng);
        var.set(init_orders.update(random.id.clone(), random));
        incr.stabilise();
        drop(shares.expect_value());
    }
}

#[tracing::instrument(skip_all)]
fn bench_update(c: &mut Criterion) {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_span_events(FmtSpan::ENTER)
        .init();

    let size = 1_000_000;
    c.bench_with_input(BenchmarkId::new("nested", 20), &20, |b, &size| {
        let iter_fn = setup(size, shares_per_symbol);
        b.iter(iter_fn)
    });
    c.bench_with_input(BenchmarkId::new("nested", size), &size, |b, &size| {
        let iter_fn = setup(size, shares_per_symbol);
        b.iter(iter_fn)
    });
    c.bench_with_input(BenchmarkId::new("flat", size), &size, |b, &size| {
        let iter_fn = setup(size, shares_per_symbol_flat);
        b.iter(iter_fn)
    });
}

criterion_group!(benches, bench_update);
criterion_main!(benches);

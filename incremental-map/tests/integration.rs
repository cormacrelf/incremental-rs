use std::{cell::Cell, collections::BTreeMap, rc::Rc};

use test_log::test;

use incremental::Incr;
use incremental::IncrState;
use incremental_map::prelude::*;

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
}

impl PartialEq<u32> for CallCounter {
    fn eq(&self, other: &u32) -> bool {
        self.1.get().eq(other)
    }
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
    assert_eq!(o.try_get_value(), Ok(13));

    b.remove("five");
    setter.set(b.clone());
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(8));

    b.insert("five", 100);
    setter.set(b.clone());
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(108));

    b.insert("five", 105);
    setter.set(b.clone());
    incr.stabilise();
    assert_eq!(o.try_get_value(), Ok(113));
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
    assert_eq!(filtered.value(), x.collect());
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
    assert_eq!(filtered.value(), x);

    b.remove("seven");
    b.insert("971", 971);
    v.set(b.clone());
    incr.stabilise();
    let x = BTreeMap::from([("971", 971), ("five", 5)]);
    assert_eq!(filtered.value(), x);
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
    let greetings = observer.value();
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
    let greetings = observer.value();
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

#[cfg(feature = "im")]
#[test]
fn incr_partition_mapi() {
    use im_rc::ordmap;

    let incr = IncrState::new();
    let var = incr.var(ordmap! { 2i32 => "Hello", 3 => "three", 4 => "four", 5 => "five" });
    let partitioned = var.watch().incr_partition(|key, _value| *key < 4);

    let left_ = partitioned.map_ref(|(a, _)| a).observe();
    let right = partitioned.map_ref(|(_, b)| b).observe();

    incr.stabilise();
    assert_eq!(left_.value(), ordmap! { 2i32 => "Hello", 3 => "three" });
    assert_eq!(right.value(), ordmap! { 4i32 => "four", 5 => "five" });
    left_.save_dot_to_file("/tmp/incr_partition_mapi.dot");
}

#[cfg(feature = "im")]
#[test]
fn incr_unordered_fold_struct() {
    use incremental::Value;

    struct SumFold;

    impl<K: Value + Ord> UnorderedFold<im_rc::OrdMap<K, i32>, K, i32, i32> for SumFold {
        fn add(&mut self, acc: i32, _key: &K, value: &i32) -> i32 {
            acc + value
        }

        fn remove(&mut self, acc: i32, _key: &K, value: &i32) -> i32 {
            acc - value
        }
        fn revert_to_init_when_empty(&self) -> bool {
            true
        }
        fn initial_fold(&mut self, init: i32, input: &im_rc::OrdMap<K, i32>) -> i32 {
            input.iter().fold(init, |acc, (_k, v)| acc + v)
        }
    }

    use im_rc::ordmap;
    let state = IncrState::new();
    let var = state.var(ordmap! { 1 => 1, 2 => 3 });

    let folded: Incr<i32> = var.watch().incr_unordered_fold_with(0, SumFold);
    let obs = folded.observe();
    state.stabilise();
    assert_eq!(obs.value(), 4);

    var.modify(|omap| {
        omap.insert(100, 7);
    });
    state.stabilise();
    assert_eq!(obs.value(), 11);
    var.modify(|omap| {
        omap.insert(100, 7);
    });
    state.stabilise();
    assert_eq!(obs.value(), 11);
}

#[cfg(feature = "im")]
#[test]
fn test_types() {
    use ::im_rc::ordmap;

    let state = IncrState::new();
    let var = state.var(ordmap! { 1 => 2, 2 => 4, 3 => 6 });
    let fold = ClosureFold::new()
        .add(|acc, _k, v| acc + v)
        .remove(|acc, _k, v| acc - v)
        .update(|acc, _k, old, new| acc - old + new)
        .revert_to_init_when_empty(true);
    let folded = var.watch().incr_unordered_fold_with(0, fold);
    let obs = folded.observe();
    state.stabilise();
    assert_eq!(obs.value(), 12);
}

#[test]
#[cfg(feature = "im")]
fn test_merge() {
    use ::im_rc::ordmap;
    let state = IncrState::new();
    let left = state.var(ordmap! { 1i32 => "a", 2 => "a" });
    let right = state.var(ordmap! {             2 => "b", 3 => "b" });

    let l = left.incr_merge(&right, |_key, merge| merge.into_left().cloned());
    let r = left.incr_merge(&right, |_key, merge| merge.into_right().cloned());

    let m = left.incr_merge(&right, |_key, merge| match merge {
        MergeElement::Left(left) => Some(format!("left: {left}")),
        MergeElement::Right(right) => Some(format!("right: {right}")),
        MergeElement::Both(left, right) => Some(format!("both: {left} + {right}")),
    });

    let l_obs = l.observe();
    state.stabilise();
    assert_eq!(
        l_obs.value(),
        ordmap! {
            1 => "a",
            2 => "a"
        }
    );

    let r_obs = r.observe();
    state.stabilise();
    assert_eq!(
        r_obs.value(),
        ordmap! {
            2 => "b",
            3 => "b"
        }
    );

    let m_obs = m.observe();
    state.stabilise();
    assert_eq!(
        m_obs.value(),
        ordmap! {
            1 => "left: a".to_owned(),
            2 => "both: a + b".to_owned(),
            3 => "right: b".to_owned()
        }
    );

    // Now update. We should merge again with the updated key
    right.modify(|map| _ = map.insert(2, "b (updated)"));
    state.stabilise();

    assert_eq!(
        m_obs.value(),
        ordmap! {
            1 => "left: a".to_owned(),
            2 => "both: a + b (updated)".to_owned(),
            3 => "right: b".to_owned()
        }
    );

    // concurrent modify delete
    left.modify(|map| _ = map.remove(&2));
    right.modify(|map| {
        map.insert(2, "b (updated 2)");
        map.remove(&3);
    });
    state.stabilise();

    assert_eq!(
        m_obs.value(),
        ordmap! {
            1 => "left: a".to_owned(),
            2 => "right: b (updated 2)".to_owned()
        }
    );
}

use std::{cell::Cell, collections::BTreeMap, rc::Rc};

use test_log::test;

use incremental::IncrState;
use incremental_map::Symmetric;

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

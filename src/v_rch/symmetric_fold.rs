use std::{
    collections::{
        btree_map::{Iter, Keys},
        BTreeMap,
    },
    iter::Peekable,
};

// Adapted from itertools.
// For [1, 2, 3].merge([2, 4])
// you should get [1, 2, 3, 4].
struct MergeOnce<I, J>
where
    I: Iterator,
    J: Iterator<Item = I::Item>,
{
    a: Peekable<I>,
    b: Peekable<J>,
    fused: Option<bool>,
}

impl<I, J> MergeOnce<I, J>
where
    I: Iterator,
    J: Iterator<Item = I::Item>,
{
    fn new(a: I, b: J) -> Self {
        Self {
            a: a.peekable(),
            b: b.peekable(),
            fused: None,
        }
    }
}

impl<I, J> Iterator for MergeOnce<I, J>
where
    I: Iterator,
    J: Iterator<Item = I::Item>,
    I::Item: PartialOrd,
{
    type Item = I::Item;
    fn next(&mut self) -> Option<Self::Item> {
        let (less_than, both) = match self.fused {
            Some(lt) => (lt, false),
            None => match (self.a.peek(), self.b.peek()) {
                (Some(a), Some(b)) => (a <= b, a == b),
                (Some(_), None) => {
                    self.fused = Some(true);
                    (true, false)
                }
                (None, Some(_)) => {
                    self.fused = Some(false);
                    (false, false)
                }
                (None, None) => return None,
            },
        };

        if less_than {
            if both {
                drop(self.b.next());
            }
            self.a.next()
        } else {
            if both {
                drop(self.a.next());
            }
            self.b.next()
        }
    }
}

pub(crate) struct SymmetricDifference<'a, K, V> {
    self_: &'a BTreeMap<K, V>,
    other: &'a BTreeMap<K, V>,
    keys: MergeOnce<Keys<'a, K, V>, Keys<'a, K, V>>,
}

pub(crate) enum Has<V> {
    Both(V, V),
    Left(V),
    Right(V),
}

impl<'a, K, V> Iterator for SymmetricDifference<'a, K, V>
where
    K: Ord,
{
    type Item = (&'a K, Has<&'a V>);
    fn next(&mut self) -> Option<Self::Item> {
        let key = self.keys.next()?;
        let s = self.self_.get(key);
        let o = self.other.get(key);
        let has = match (s, o) {
            (Some(a), Some(b)) => Has::Both(a, b),
            (Some(a), _) => Has::Left(a),
            (_, Some(b)) => Has::Right(b),
            _ => return None,
        };
        Some((key, has))
    }
}

#[test]
fn test_merge_once() {
    let i = [1i32, 2, 3][..].into_iter();
    let j = [2i32, 4][..].into_iter();
    let v: Vec<_> = MergeOnce::new(i, j).cloned().collect();
    assert_eq!(v, vec![1, 2, 3, 4]);
}

pub(crate) trait BTreeMapSymmetricFold<K: Ord, V: Eq> {
    fn symmetric_difference<'a>(&'a self, other: &'a Self) -> SymmetricDifference<'a, K, V>;
    fn symmetric_fold<R, FAdd, FRemove>(
        &self,
        other: &BTreeMap<K, V>,
        init: R,
        add: FAdd,
        remove: FRemove,
    ) -> R
    where
        FAdd: FnMut(R, &K, &V) -> R,
        FRemove: FnMut(R, &K, &V) -> R;
}

impl<K: Ord, V: Eq> BTreeMapSymmetricFold<K, V> for BTreeMap<K, V> {
    fn symmetric_difference<'a>(&'a self, other: &'a Self) -> SymmetricDifference<K, V> {
        SymmetricDifference {
            self_: self,
            other,
            keys: MergeOnce::new(self.keys(), other.keys()),
        }
    }
    fn symmetric_fold<R, FAdd, FRemove>(
        &self,
        other: &BTreeMap<K, V>,
        init: R,
        mut add: FAdd,
        mut remove: FRemove,
    ) -> R
    where
        FAdd: FnMut(R, &K, &V) -> R,
        FRemove: FnMut(R, &K, &V) -> R,
    {
        let mut acc = init;
        let iter = self.symmetric_difference(other);
        for (key, has) in iter {
            match has {
                Has::Both(left, right) => {
                    if left != right {
                        acc = add(acc, key, right);
                        acc = remove(acc, key, left);
                    }
                }
                Has::Left(left) => {
                    acc = remove(acc, key, left);
                }
                Has::Right(right) => {
                    acc = add(acc, key, right);
                }
            }
        }
        acc
    }
}

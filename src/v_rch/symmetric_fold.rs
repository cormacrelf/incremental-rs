use std::cmp::Ordering;
use std::collections::{
    btree_map::{IntoIter, Keys},
    BTreeMap,
};
use std::iter::Peekable;

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

pub(crate) struct SymmetricDiffOwned<K, V> {
    self_: Peekable<IntoIter<K, V>>,
    other: Peekable<IntoIter<K, V>>,
    fused: Option<bool>, // keys: MergeOnce<Keys<'a, K, V>, Keys<'a, K, V>>,
}

impl<K, V> Iterator for SymmetricDiffOwned<K, V>
where
    K: Ord,
    V: PartialEq,
{
    type Item = DiffElement<(K, V)>;
    fn next(&mut self) -> Option<Self::Item> {
        let less_than = loop {
            match self.fused {
                Some(lt) => break lt,
                None => match (self.self_.peek(), self.other.peek()) {
                    (Some((ka, va)), Some((kb, vb))) => match ka.cmp(kb) {
                        Ordering::Less => break true,
                        Ordering::Greater => break false,
                        Ordering::Equal => {
                            let unequal = va != vb;
                            let (sk, sv) = self.self_.next()?;
                            let (ok, ov) = self.other.next()?;
                            if unequal {
                                return Some(DiffElement::Unequal((sk, sv), (ok, ov)));
                            } else {
                                continue;
                            }
                        }
                    },
                    (Some(_), None) => {
                        self.fused = Some(true);
                        break true;
                    }
                    (None, Some(_)) => {
                        self.fused = Some(false);
                        break false;
                    }
                    (None, None) => return None,
                },
            }
        };
        if less_than {
            self.self_.next().map(|(k, v)| DiffElement::Left((k, v)))
        } else {
            self.other.next().map(|(k, v)| DiffElement::Right((k, v)))
        }
    }
}

pub(crate) struct SymmetricDiff<'a, K: 'a, V: 'a> {
    self_: &'a BTreeMap<K, V>,
    other: &'a BTreeMap<K, V>,
    keys: MergeOnce<Keys<'a, K, V>, Keys<'a, K, V>>,
}

impl<'a, K: 'a, V: 'a> Iterator for SymmetricDiff<'a, K, V>
where
    K: Ord,
    V: PartialEq,
{
    type Item = DiffElement<(&'a K, &'a V)>;
    fn next(&mut self) -> Option<Self::Item> {
        let mut key;
        let elem = loop {
            key = self.keys.next()?;
            let s = self.self_.get(key);
            let o = self.other.get(key);
            match (s, o) {
                (Some(a), Some(b)) if a != b => break DiffElement::Unequal((key, a), (key, b)),
                (Some(a), Some(b)) => continue,
                (Some(a), _) => break DiffElement::Left((key, a)),
                (_, Some(b)) => break DiffElement::Right((key, b)),
                _ => return None,
            }
        };
        Some(elem)
    }
}

pub(crate) enum DiffElement<V> {
    Unequal(V, V),
    Left(V),
    Right(V),
}

#[test]
fn test_merge_once() {
    let i = [1i32, 2, 3][..].into_iter();
    let j = [2i32, 4][..].into_iter();
    let v: Vec<_> = MergeOnce::new(i, j).cloned().collect();
    assert_eq!(v, vec![1, 2, 3, 4]);
}

pub(crate) trait SymmetricDiffMap<'a, K: 'a, V: 'a> {
    type Iter: Iterator<Item = DiffElement<(&'a K, &'a V)>>;

    fn symmetric_diff(&'a self, other: &'a Self) -> Self::Iter;

    fn symmetric_fold<R, FAdd, FRemove>(
        &'a self,
        other: &'a Self,
        init: R,
        mut add: FAdd,
        mut remove: FRemove,
    ) -> R
    where
        FAdd: FnMut(R, &K, &V) -> R,
        FRemove: FnMut(R, &K, &V) -> R,
        K: Ord,
        V: Eq,
    {
        self.symmetric_diff(other)
            .fold(init, |mut acc, elem| match elem {
                DiffElement::Unequal(left, right) => {
                    acc = add(acc, right.0, right.1);
                    remove(acc, left.0, left.1)
                }
                DiffElement::Left(left) => remove(acc, left.0, left.1),
                DiffElement::Right(right) => add(acc, right.0, right.1),
            })
    }
}

pub(crate) trait SymmetricDiffMapOwned<K, V> {
    type Iter: Iterator<Item = DiffElement<(K, V)>>;

    fn symmetric_diff_owned(self, other: Self) -> Self::Iter;

    fn symmetric_fold_owned<R, FAdd, FRemove>(
        self,
        other: Self,
        init: R,
        mut add: FAdd,
        mut remove: FRemove,
    ) -> R
    where
        FAdd: FnMut(R, K, V) -> R,
        FRemove: FnMut(R, K, V) -> R,
        K: Ord,
        V: Eq,
        Self: Sized,
    {
        self.symmetric_diff_owned(other)
            .fold(init, |mut acc, elem| match elem {
                DiffElement::Unequal(left, right) => {
                    acc = add(acc, right.0, right.1);
                    remove(acc, left.0, left.1)
                }
                DiffElement::Left(left) => remove(acc, left.0, left.1),
                DiffElement::Right(right) => add(acc, right.0, right.1),
            })
    }
}

impl<'a, K: Ord + 'a, V: Eq + 'a> SymmetricDiffMap<'a, K, V> for BTreeMap<K, V> {
    type Iter = SymmetricDiff<'a, K, V>;
    fn symmetric_diff(&'a self, other: &'a Self) -> Self::Iter {
        SymmetricDiff {
            self_: self,
            other,
            keys: MergeOnce::new(self.keys(), other.keys()),
        }
    }
}

impl<K: Ord, V: Eq> SymmetricDiffMapOwned<K, V> for BTreeMap<K, V> {
    type Iter = SymmetricDiffOwned<K, V>;
    fn symmetric_diff_owned(self, other: Self) -> SymmetricDiffOwned<K, V> {
        SymmetricDiffOwned {
            self_: self.into_iter().peekable(),
            other: other.into_iter().peekable(),
            fused: None,
        }
    }
}

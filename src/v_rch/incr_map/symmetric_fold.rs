#[cfg(test)]
use test_log::test;

use std::cmp::Ordering;
use std::collections::{
    btree_map::{IntoIter, Keys},
    BTreeMap,
};
use std::iter::Peekable;
use std::ops::Deref;
use std::rc::Rc;

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

// Same but with a custom comparator
pub(crate) struct MergeOnceWith<I, J, F>
where
    I: Iterator,
    J: Iterator,
    F: Fn(&I::Item, &J::Item) -> Ordering,
{
    a: Peekable<I>,
    b: Peekable<J>,
    f: F,
    fused: Option<bool>,
}

impl<I: Iterator, J: Iterator, F: Fn(&I::Item, &J::Item) -> Ordering> MergeOnceWith<I, J, F> {
    pub(crate) fn new(a: I, b: J, f: F) -> Self {
        Self {
            a: a.peekable(),
            b: b.peekable(),
            f,
            fused: None,
        }
    }
}

impl<I, J, F> Iterator for MergeOnceWith<I, J, F>
where
    I: Iterator,
    J: Iterator,
    F: Fn(&I::Item, &J::Item) -> Ordering,
{
    type Item = MergeElement<I::Item, J::Item>;
    fn next(&mut self) -> Option<Self::Item> {
        let ordering: Ordering = match self.fused {
            Some(true) => Ordering::Less,
            Some(false) => Ordering::Greater,
            None => match (self.a.peek(), self.b.peek()) {
                (Some(a), Some(b)) => (self.f)(a, b),
                (Some(_), None) => {
                    self.fused = Some(true);
                    Ordering::Less
                }
                (None, Some(_)) => {
                    self.fused = Some(false);
                    Ordering::Greater
                }
                (None, None) => return None,
            },
        };

        match ordering {
            Ordering::Equal => self
                .a
                .next()
                .zip(self.b.next())
                .map(|(a, b)| MergeElement::Both(a, b)),
            Ordering::Less => {
                if self.fused.is_none() {
                    drop(self.b.next());
                }
                self.a.next().map(MergeElement::Left)
            }
            Ordering::Greater => {
                if self.fused.is_none() {
                    drop(self.a.next());
                }
                self.b.next().map(MergeElement::Right)
            }
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
    type Item = (&'a K, DiffElement<&'a V>);
    fn next(&mut self) -> Option<Self::Item> {
        let mut key;
        let elem = loop {
            key = self.keys.next()?;
            let s = self.self_.get(key);
            let o = self.other.get(key);
            match (s, o) {
                (Some(a), Some(b)) if a != b => break DiffElement::Unequal(a, b),
                (Some(_), Some(_)) => continue,
                (Some(a), _) => break DiffElement::Left(a),
                (_, Some(b)) => break DiffElement::Right(b),
                _ => return None,
            }
        };
        Some((key, elem))
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MergeElement<L, R> {
    Left(L),
    Right(R),
    Both(L, R),
}

impl<L, R> MergeElement<&L, &R>
where
    L: Clone,
    R: Clone,
{
    pub fn cloned(&self) -> MergeElement<L, R> {
        match *self {
            MergeElement::Both(a, b) => MergeElement::Both(a.clone(), b.clone()),
            MergeElement::Left(a) => MergeElement::Left(a.clone()),
            MergeElement::Right(b) => MergeElement::Right(b.clone()),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DiffElement<V> {
    Unequal(V, V),
    Left(V),
    Right(V),
}

#[test]
fn test_merge_once() {
    let i = [1i32, 2, 3][..].iter();
    let j = [2i32, 4][..].iter();
    let v: Vec<_> = MergeOnce::new(i, j).cloned().collect();
    assert_eq!(v, vec![1, 2, 3, 4]);
}

pub(crate) trait SymmetricDiffMap<'a, K: 'a, V: 'a> {
    type Iter: Iterator<Item = (&'a K, DiffElement<&'a V>)>;

    fn symmetric_diff(&'a self, other: &'a Self) -> Self::Iter;

    fn symmetric_fold_with_inverse<R, FAdd, FRemove>(
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
        V: PartialEq,
    {
        self.symmetric_diff(other)
            .fold(init, |mut acc, (key, elem)| match elem {
                DiffElement::Unequal(left, right) => {
                    acc = add(acc, key, right);
                    remove(acc, key, left)
                }
                DiffElement::Left(left) => remove(acc, key, left),
                DiffElement::Right(right) => add(acc, key, right),
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
        V: PartialEq,
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

impl<'a, K: Ord + 'a, V: PartialEq + 'a> SymmetricDiffMap<'a, K, V> for BTreeMap<K, V> {
    type Iter = SymmetricDiff<'a, K, V>;
    fn symmetric_diff(&'a self, other: &'a Self) -> Self::Iter {
        SymmetricDiff {
            self_: self,
            other,
            keys: MergeOnce::new(self.keys(), other.keys()),
        }
    }
}

impl<K: Ord, V: PartialEq> SymmetricDiffMapOwned<K, V> for BTreeMap<K, V> {
    type Iter = SymmetricDiffOwned<K, V>;
    fn symmetric_diff_owned(self, other: Self) -> SymmetricDiffOwned<K, V> {
        SymmetricDiffOwned {
            self_: self.into_iter().peekable(),
            other: other.into_iter().peekable(),
            fused: None,
        }
    }
}

pub trait GenericMap<K, V> {
    fn remove(&mut self, key: &K) -> Option<V>;
    fn insert(&mut self, key: K, value: V) -> Option<V>;
}

impl<K: Ord, V> GenericMap<K, V> for BTreeMap<K, V> {
    #[inline]
    fn remove(&mut self, key: &K) -> Option<V> {
        self.remove(key)
    }

    #[inline]
    fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.insert(key, value)
    }
}

pub trait SymmetricMapMap<K, V> {
    type UnderlyingMap: GenericMap<K, V>;
    type OutputMap<V2: PartialEq + Clone>: SymmetricMapMap<K, V2>;
    fn make_mut(&mut self) -> &mut Self::UnderlyingMap;
    fn filter_map_collect<V2: PartialEq + Clone>(
        &self,
        f: &mut impl FnMut(&K, &V) -> Option<V2>,
    ) -> Self::OutputMap<V2>;
}

pub trait SymmetricFoldMap<K, V> {
    fn symmetric_fold<R>(
        &self,
        other: &Self,
        init: R,
        f: impl FnMut(R, (&K, DiffElement<&V>)) -> R,
    ) -> R;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn nonincremental_fold<R>(&self, init: R, f: impl FnMut(R, (&K, &V)) -> R) -> R;
}

impl<K: Ord + Clone, V: PartialEq + Clone> SymmetricMapMap<K, V> for Rc<BTreeMap<K, V>> {
    type UnderlyingMap = BTreeMap<K, V>;
    type OutputMap<V2: PartialEq + Clone> = Rc<BTreeMap<K, V2>>;
    #[inline]
    fn make_mut(&mut self) -> &mut Self::UnderlyingMap {
        Rc::make_mut(self)
    }
    #[inline]
    fn filter_map_collect<V2: PartialEq + Clone>(
        &self,
        f: &mut impl FnMut(&K, &V) -> Option<V2>,
    ) -> Self::OutputMap<V2> {
        Rc::new(self.deref().filter_map_collect(f))
    }
}

impl<K: Ord, V: PartialEq> SymmetricFoldMap<K, V> for Rc<BTreeMap<K, V>> {
    fn symmetric_fold<R>(
        &self,
        other: &Self,
        init: R,
        f: impl FnMut(R, (&K, DiffElement<&V>)) -> R,
    ) -> R {
        let self_target = self.deref();
        let other_target = other.deref();
        self_target.symmetric_diff(other_target).fold(init, f)
    }
    #[inline]
    fn len(&self) -> usize {
        self.deref().len()
    }
    fn nonincremental_fold<R>(&self, init: R, f: impl FnMut(R, (&K, &V)) -> R) -> R {
        self.deref().nonincremental_fold(init, f)
    }
}

impl<K: Ord + Clone, V: PartialEq> SymmetricMapMap<K, V> for BTreeMap<K, V> {
    type UnderlyingMap = Self;
    type OutputMap<V2: PartialEq + Clone> = BTreeMap<K, V2>;
    fn make_mut(&mut self) -> &mut Self::UnderlyingMap {
        self
    }
    fn filter_map_collect<V2: PartialEq + Clone>(
        &self,
        f: &mut impl FnMut(&K, &V) -> Option<V2>,
    ) -> Self::OutputMap<V2> {
        self.iter()
            .filter_map(|(k, v)| f(k, v).map(|v2| (k.clone(), v2)))
            .collect()
    }
}
impl<K: Ord, V: PartialEq> SymmetricFoldMap<K, V> for BTreeMap<K, V> {
    fn symmetric_fold<R>(
        &self,
        other: &Self,
        init: R,
        f: impl FnMut(R, (&K, DiffElement<&V>)) -> R,
    ) -> R {
        self.symmetric_diff(other).fold(init, f)
    }
    #[inline]
    fn len(&self) -> usize {
        self.len()
    }
    #[inline]
    fn nonincremental_fold<R>(&self, init: R, f: impl FnMut(R, (&K, &V)) -> R) -> R {
        self.iter().fold(init, f)
    }
}

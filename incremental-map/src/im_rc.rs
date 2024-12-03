use std::{cell::RefCell, marker::PhantomData, rc::Rc};

use im_rc::{ordmap, ordmap::DiffItem, OrdMap};

use incremental::expert::{Dependency, Node, WeakNode};
use incremental::incrsan::NotObserver;
use incremental::{Cutoff, Incr, Value};

use crate::symmetric_fold::{
    DiffElement, GenericMap, MergeElement, MergeOnceWith, MutableMap, SymmetricDiffMap,
    SymmetricFoldMap, SymmetricMapMap,
};

use crate::{FilterMapOperator, IncrMap, MapOperator, Operator, WithOldIO};

impl<'a, V> DiffElement<&'a V> {
    fn from_diff_item<K>(value: DiffItem<'a, K, V>) -> (&'a K, Self) {
        match value {
            // ordmap's nomenclature is tricky. The diff is a list of things to do to self in `self.diff(other)`,
            // to make them equal.
            // if we have to add to self, that means it was only found in other (=Right)
            DiffItem::Add(k, v) => (k, Self::Right(v)),
            // if we have to remove k from self, that means it was only found in self (=Left)
            DiffItem::Remove(k, v) => (k, Self::Left(v)),
            // now we have "old" and "new"...
            DiffItem::Update {
                old: (k, old),
                new: (_, new),
            } => (k, Self::Unequal(old, new)),
        }
    }
}

#[test]
fn test_diff_item_to_element() {
    use std::collections::BTreeMap;
    let mut a = BTreeMap::new();
    a.insert(1, "hi");
    a.insert(2, "lo");
    a.insert(3, "nice");
    let mut b = BTreeMap::new();
    b.insert(1, "hi");
    b.insert(3, "updated in b");
    b.insert(4, "inserted in b");
    let diff: Vec<_> = a.symmetric_diff(&b).collect();
    let expected = vec![
        (&2, DiffElement::Left(&"lo")),
        (&3, DiffElement::Unequal(&"nice", &"updated in b")),
        (&4, DiffElement::Right(&"inserted in b")),
    ];
    assert_eq!(diff, expected);

    let a = ordmap! { 1 => "hi", 2 => "lo", 3 => "nice" };
    let b = ordmap! { 1 => "hi",            3 => "updated in b", 4 => "inserted in b" };
    let diff: Vec<_> = a.diff(&b).map(DiffElement::from_diff_item).collect();
    assert_eq!(diff, expected);
}

impl<'a, K: Ord + 'a, V: PartialEq + 'a> SymmetricDiffMap<'a, K, V> for OrdMap<K, V> {
    type Iter = core::iter::Map<
        ordmap::DiffIter<'a, K, V>,
        fn(DiffItem<'a, K, V>) -> (&'a K, DiffElement<&'a V>),
    >;
    fn symmetric_diff(&'a self, other: &'a Self) -> Self::Iter {
        self.diff(other).map(DiffElement::from_diff_item)
    }
}

impl<K: Ord, V: PartialEq> SymmetricFoldMap<K, V> for OrdMap<K, V> {
    #[inline]
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
        OrdMap::len(self)
    }

    #[inline]
    fn nonincremental_fold<R>(&self, init: R, f: impl FnMut(R, (&K, &V)) -> R) -> R {
        self.iter().fold(init, f)
    }
}

impl<K: Ord + Clone, V: Clone> GenericMap<K, V> for OrdMap<K, V> {
    #[inline]
    fn remove(&mut self, key: &K) -> Option<V> {
        OrdMap::remove(self, key)
    }

    #[inline]
    fn insert(&mut self, key: K, value: V) -> Option<V> {
        OrdMap::insert(self, key, value)
    }
}

impl<K: Ord + Clone, V: Clone> MutableMap<K, V> for OrdMap<K, V> {
    type UnderlyingMap = Self;

    #[inline]
    fn make_mut(&mut self) -> &mut Self::UnderlyingMap {
        self
    }
}

impl<K: Ord + Clone, V: Clone> SymmetricMapMap<K, V> for OrdMap<K, V> {
    type OutputMap<V2: PartialEq + Clone> = OrdMap<K, V2>;

    fn filter_map_collect<V2: PartialEq + Clone>(
        &self,
        f: &mut impl FnMut(&K, &V) -> Option<V2>,
    ) -> Self::OutputMap<V2> {
        self.iter()
            .filter_map(|(k, v)| f(k, v).map(|v2| (k.clone(), v2)))
            .collect()
    }
}

#[test]
fn test_merge() {
    use incremental::IncrState;
    let incr = IncrState::new();
    let o1 = incr.var(ordmap! { 1i32 => "a", 2 => "b", 3 => "c" });
    let o2 = incr.var(ordmap! { 1i32 => "one", 2 => "two", 3 => "three" });
    let merge = o1.incr_merge(&o2, |_key, merge| {
        // simply returning the merge element is akin to an incremental diff,
        // except you get Both(a, b) for the ones that aren't different.
        Some(merge.cloned())
    });
    let obs = merge.observe();
    incr.stabilise();
    use MergeElement::*;
    assert_eq!(
        obs.try_get_value(),
        Ok(ordmap! {
            1 => Both("a", "one"), 2 => Both("b", "two"), 3 => Both("c", "three")
        })
    );

    o1.modify(|map| {
        map.remove(&2);
        map.insert(3, "replaced");
        map.insert(4, "added");
    });
    incr.stabilise();
    assert_eq!(
        dbg!(obs.try_get_value()),
        Ok(ordmap! {
            1 => Both("a", "one"), 2 => Right("two"), 3 => Both("replaced", "three"),
            4 => Left("added")
        })
    );
    obs.save_dot_to_file("im_incr_merge.dot");
}

#[test]
fn test_ptr_eq() {
    #[derive(PartialEq, Clone)]
    enum NotEq {
        A,
        B,
    }
    let o1 = ordmap! { 1 => NotEq::A, 2 => NotEq::B };
    let o2 = o1.clone();
    assert!(o1.ptr_eq(&o2));
}

/// On nightly rust, im will specialize the PartialEq impl to use `ptr_eq() || diff().next().is_none()`.
/// (When K, V : Eq.) We can use cutoff functions to do that ourselves.
fn ordmap_fast_eq<K: Ord + Eq, V: Eq>(a: &OrdMap<K, V>, b: &OrdMap<K, V>) -> bool {
    a.ptr_eq(b) || a.eq(b)
}
use std::hash::Hash;
fn hashmap_fast_eq<K: Hash + Eq, V: Eq>(
    a: &im_rc::HashMap<K, V>,
    b: &im_rc::HashMap<K, V>,
) -> bool {
    a.ptr_eq(b) || a.eq(b)
}
pub fn im_ordmap_cutoff<K: Ord + Eq, V: Eq>() -> Cutoff<OrdMap<K, V>> {
    Cutoff::Fn(ordmap_fast_eq)
}
pub fn im_hashmap_cutoff<K: Hash + Eq, V: Eq>() -> Cutoff<im_rc::HashMap<K, V>> {
    Cutoff::Fn(hashmap_fast_eq)
}

pub(crate) fn merge_shared_impl<
    K: Clone + Ord,
    V1: Clone + PartialEq,
    V2: Clone + PartialEq,
    R: Clone,
>(
    old: Option<(OrdMap<K, V1>, OrdMap<K, V2>, OrdMap<K, R>)>,
    new_left_map: &OrdMap<K, V1>,
    new_right_map: &OrdMap<K, V2>,
    mut f: impl FnMut(
        OrdMap<K, R>,
        &K,
        MergeElement<(&K, DiffElement<&V1>), (&K, DiffElement<&V2>)>,
    ) -> OrdMap<K, R>,
) -> OrdMap<K, R> {
    let (old_left_map, old_right_map, old_output) = match old {
        None => (OrdMap::new(), OrdMap::new(), OrdMap::new()),
        Some(x) => x,
    };
    let left_diff = old_left_map.symmetric_diff(new_left_map);
    let right_diff = old_right_map.symmetric_diff(new_right_map);
    // relies on the key iteration being sorted, as in BTreeMap.
    let merge = MergeOnceWith::new(left_diff, right_diff, |(k, _), (k2, _)| k.cmp(k2));
    merge.fold(old_output, |output, merge_elem| {
        let key = match merge_elem {
            MergeElement::Left((key, _)) | MergeElement::Right((key, _)) => key,
            MergeElement::Both((left_key, _), (_right_key, _)) => {
                // comparisons can be expensive
                // assert_eq!(left_key, right_key);
                left_key
            }
        };
        f(output, key, merge_elem)
    })
}

macro_rules! doc {
    ($ident:ident) => { doc!($ident, @map, "") };
    ($ident:ident, @map, $cutoff:literal) => {
        doc!(
            $ident,
            "Splits the thing",
            ""
        )
    };
    ($ident:ident, @filter, $cutoff:literal) => { doc!($ident, "Filter", "") };
    ($ident:ident, $desc:literal, $cutoff:literal) => {
        concat! {
            $desc,
            $cutoff
        }
    };
}

/// Used for [IncrOrdMap::incr_partition_mapi]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Either<A, B> {
    Left(A),
    Right(B),
}

/// Incremental map and filter_map for `im_rc::OrdMap`.

pub trait IncrOrdMap<K: Value + Ord, V: Value> {
    #[doc = doc!(incr_mapi_)]
    fn incr_mapi_<F, V2>(&self, f: F) -> Incr<OrdMap<K, V2>>
    where
        V2: Value,
        F: FnMut(&K, Incr<V>) -> Incr<V2> + 'static + NotObserver;

    #[doc = doc!(incr_mapi_cutoff)]
    fn incr_mapi_cutoff<F, V2>(&self, f: F, cutoff: Cutoff<V>) -> Incr<OrdMap<K, V2>>
    where
        V2: Value,
        F: FnMut(&K, Incr<V>) -> Incr<V2> + 'static + NotObserver;

    #[doc = doc!(incr_filter_mapi_)]
    fn incr_filter_mapi_<F, V2>(&self, f: F) -> Incr<OrdMap<K, V2>>
    where
        V2: Value,
        F: FnMut(&K, Incr<V>) -> Incr<Option<V2>> + 'static + NotObserver;

    #[doc = doc!(incr_filter_mapi_cutoff)]
    fn incr_filter_mapi_cutoff<F, V2>(&self, f: F, cutoff: Cutoff<V>) -> Incr<OrdMap<K, V2>>
    where
        V2: Value,
        F: FnMut(&K, Incr<V>) -> Incr<Option<V2>> + 'static + NotObserver;

    fn incr_merge<F, V2, R>(&self, other: &Incr<OrdMap<K, V2>>, f: F) -> Incr<OrdMap<K, R>>
    where
        V2: Value,
        R: Value,
        F: FnMut(&K, MergeElement<&V, &V2>) -> Option<R> + 'static + NotObserver;

    /// Partitions the input such that key-value pairs for which the predicate
    /// returns [`Either::Left`] are in the left map, and pairs for which it returns
    /// [`Either::Right`] are in the right map.
    fn incr_partition_mapi<F, A, B>(&self, f: F) -> Incr<(OrdMap<K, A>, OrdMap<K, B>)>
    where
        A: Value,
        B: Value,
        F: FnMut(&K, &V) -> Either<A, B> + 'static + NotObserver;

    /// Partitions the input such that key-value pairs for which the predicate
    /// returns `true` are in the left map, and pairs for which it returns `false`
    /// are in the right map.
    fn incr_partition<F>(&self, mut pred: F) -> Incr<(OrdMap<K, V>, OrdMap<K, V>)>
    where
        F: FnMut(&K, &V) -> bool + 'static + NotObserver,
    {
        self.incr_partition_mapi(move |k, v| {
            if pred(k, v) {
                Either::Left(v.clone())
            } else {
                Either::Right(v.clone())
            }
        })
    }
}

impl<K: Value + Ord, V: Value> IncrOrdMap<K, V> for Incr<OrdMap<K, V>> {
    fn incr_mapi_<F, V2>(&self, f: F) -> Incr<OrdMap<K, V2>>
    where
        V2: Value,
        F: FnMut(&K, Incr<V>) -> Incr<V2> + 'static + NotObserver,
    {
        incr_filter_mapi_ordmap(self, MapOperator(f, PhantomData), None)
    }

    fn incr_mapi_cutoff<F, V2>(&self, f: F, cutoff: Cutoff<V>) -> Incr<OrdMap<K, V2>>
    where
        V2: Value,
        F: FnMut(&K, Incr<V>) -> Incr<V2> + 'static + NotObserver,
    {
        incr_filter_mapi_ordmap(self, MapOperator(f, PhantomData), Some(cutoff))
    }

    fn incr_filter_mapi_<F, V2>(&self, f: F) -> Incr<OrdMap<K, V2>>
    where
        V2: Value,
        F: FnMut(&K, Incr<V>) -> Incr<Option<V2>> + 'static + NotObserver,
    {
        incr_filter_mapi_ordmap(self, FilterMapOperator(f, PhantomData), None)
    }

    fn incr_filter_mapi_cutoff<F, V2>(&self, f: F, cutoff: Cutoff<V>) -> Incr<OrdMap<K, V2>>
    where
        V2: Value,
        F: FnMut(&K, Incr<V>) -> Incr<Option<V2>> + 'static + NotObserver,
    {
        incr_filter_mapi_ordmap(self, FilterMapOperator(f, PhantomData), Some(cutoff))
    }

    fn incr_merge<F, V2, R>(&self, other: &Incr<OrdMap<K, V2>>, mut f: F) -> Incr<OrdMap<K, R>>
    where
        V2: Value,
        R: Value,
        F: FnMut(&K, MergeElement<&V, &V2>) -> Option<R> + 'static + NotObserver,
    {
        let i = self.with_old_input_output2(other, move |old, new_left_map, new_right_map| {
            let mut did_change = false;
            let output = merge_shared_impl(
                old,
                new_left_map,
                new_right_map,
                |mut acc_output, key, merge_elem| {
                    use MergeElement::*;
                    did_change = true;
                    let data = match merge_elem {
                        Both((_, left_diff), (_, right_diff)) => {
                            (left_diff.new_data(), right_diff.new_data())
                        }
                        Left((_, left_diff)) => (left_diff.new_data(), new_right_map.get(key)),
                        Right((_, right_diff)) => (new_left_map.get(key), right_diff.new_data()),
                    };
                    let output_data_opt = match data {
                        (None, None) => None,
                        (Some(x), None) => f(key, MergeElement::Left(x)),
                        (None, Some(x)) => f(key, MergeElement::Right(x)),
                        (Some(a), Some(b)) => f(key, MergeElement::Both(a, b)),
                    };
                    match output_data_opt {
                        None => acc_output.remove(key),
                        Some(r) => acc_output.insert(key.clone(), r),
                    };
                    acc_output
                },
            );
            (output, did_change)
        });
        #[cfg(debug_assertions)]
        i.set_graphviz_user_data(Box::new(format!(
            "incr_merge -> {}",
            std::any::type_name::<OrdMap<K, R>>()
        )));
        i
    }

    fn incr_partition_mapi<F, A, B>(&self, f: F) -> Incr<(OrdMap<K, A>, OrdMap<K, B>)>
    where
        A: Value,
        B: Value,
        F: FnMut(&K, &V) -> Either<A, B> + 'static + NotObserver,
    {
        let empty = OrdMap::new();
        let empty2 = OrdMap::new();
        let f1 = Rc::new(RefCell::new(f));
        let f2 = f1.clone();
        self.incr_unordered_fold_update(
            (empty, empty2),
            move |(first, second), key, data| match f1.borrow_mut()(key, data) {
                Either::Left(data) => (OrdMap::update(&first, key.clone(), data), second),
                Either::Right(data) => (first, OrdMap::update(&second, key.clone(), data)),
            },
            move |(first, second), key, _| {
                (OrdMap::without(&first, key), OrdMap::without(&second, key))
            },
            move |(first, second), key, _old_data, data| match f2.borrow_mut()(key, data) {
                Either::Left(data) => (
                    OrdMap::update(&first, key.clone(), data),
                    OrdMap::without(&second, key),
                ),
                Either::Right(data) => (
                    OrdMap::without(&first, key),
                    OrdMap::update(&second, key.clone(), data),
                ),
            },
            true,
        )
    }
}

fn incr_filter_mapi_ordmap<K, V, O, V2>(
    lhs: &Incr<OrdMap<K, V>>,
    mut f: O,
    cutoff: Option<Cutoff<V>>,
) -> Incr<OrdMap<K, V2>>
where
    K: Value + Ord,
    V: Value,
    O: Operator<K, V, V2> + 'static + NotObserver,
    O::Output: Value,
    V2: Value,
{
    let state = lhs.state();
    let prev_map = Rc::new(RefCell::new(OrdMap::<K, V>::new()));
    let acc = Rc::new(RefCell::new(OrdMap::new()));
    let result = Node::<OrdMap<K, V2>>::new(&state, {
        let acc_ = acc.clone();
        move || OrdMap::clone(&acc_.borrow())
    });
    let on_inner_change = {
        let acc_ = acc.clone();
        move |key: &K, output: &O::Output| {
            let mut acc = acc_.borrow_mut();
            let opt = O::as_opt(output);
            match opt {
                None => {
                    acc.remove(key);
                }
                Some(x) => {
                    acc.insert(key.clone(), x.clone());
                }
            }
            drop(acc);
        }
    };
    let mut prev_nodes = OrdMap::<K, (WeakNode<_>, Dependency<_>)>::new();
    let result_weak = result.weak();

    // this one is just moved into the closure
    let lhs_change = lhs.map_cyclic({
        move |lhs_change, map| {
            let mut prev_map_mut = prev_map.borrow_mut();
            prev_map_mut.symmetric_fold(map, &mut prev_nodes, |nodes, (key, diff)| {
                match diff {
                    DiffElement::Unequal(_, _) => {
                        let (node, _dep) = nodes.get(key).unwrap();
                        node.make_stale();
                        nodes
                    }
                    DiffElement::Left(_) => {
                        let (node, dep) = nodes.remove(key).unwrap();
                        // remove_dependency will cause node's weak ref to die.
                        // so we upgrade it first.
                        let node = node.upgrade().unwrap();
                        result_weak.remove_dependency(dep);
                        let mut acc = acc.borrow_mut();
                        acc.remove(key);
                        // Invalidate does have to happen after remove_dependency.
                        node.invalidate();
                        nodes
                    }
                    DiffElement::Right(_) => {
                        let key = key.clone();
                        let node = Node::<V>::new(&state, {
                            let key_ = key.clone();
                            let prev_map_ = prev_map.clone();
                            move || {
                                let prev_map = prev_map_.borrow();
                                prev_map.get(&key_).unwrap().clone()
                            }
                        });
                        if let Some(cutoff) = cutoff.clone() {
                            node.watch().set_cutoff(cutoff);
                        }
                        let lhs_change = lhs_change.upgrade().unwrap();
                        node.add_dependency(&lhs_change);
                        let mapped: Incr<O::Output> = f.call_fn(&key, node.watch());
                        let user_function_dep: Dependency<O::Output> = result_weak
                            .add_dependency_with(&mapped, {
                                let key = key.clone();
                                let on_inner_change = on_inner_change.clone();
                                move |v| on_inner_change(&key, v)
                            });
                        nodes.insert(key, (node.weak(), user_function_dep));
                        nodes
                    }
                }
            });
            *prev_map_mut = map.clone();
        }
    });
    result.add_dependency(&lhs_change);
    result.watch()
}

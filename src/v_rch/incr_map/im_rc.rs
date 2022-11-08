use im_rc::{OrdMap, ordmap::DiffItem, ordmap};

use crate::{Value, Incr};

use super::symmetric_fold::{MergeElement, DiffElement, MergeOnceWith, SymmetricDiffMap, SymmetricFoldMap};

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
            DiffItem::Update { old: (k, old), new: (_, new) } => (k, Self::Unequal(old, new)),
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
        fn (DiffItem<'a, K, V>) -> (&'a K, DiffElement<&'a V>)
    >;
    fn symmetric_diff(&'a self, other: &'a Self) -> Self::Iter {
        self.diff(other)
            .map(DiffElement::from_diff_item)
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
        self.symmetric_diff(other)
            .fold(init, f)
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

#[test]
fn test_merge() {
    use crate::IncrState;
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
    assert_eq!(obs.value(), Ok(ordmap! {
        1 => Both("a", "one"), 2 => Both("b", "two"), 3 => Both("c", "three")
    }));

    o1.update(|map| {
        map.remove(&2);
        map.insert(3, "replaced");
        map.insert(4, "added");
    });
    incr.stabilise();
    assert_eq!(dbg!(obs.value()), Ok(ordmap! {
        1 => Both("a", "one"), 2 => Right("two"), 3 => Both("replaced", "three"),
        4 => Left("added")
    }));
    obs.save_dot_to_file("im_incr_merge.dot");
    assert!(false);
}

pub(crate) fn merge_shared_impl<K: Clone + Ord, V1: Clone + PartialEq, V2: Clone + PartialEq, R: Clone>(
    old: Option<(OrdMap<K, V1>, OrdMap<K, V2>, OrdMap<K, R>)>,
    new_left_map: &OrdMap<K, V1>,
    new_right_map: &OrdMap<K, V2>,
    mut f: impl FnMut(OrdMap<K, R>, &K, MergeElement<(&K, DiffElement<&V1>), (&K, DiffElement<&V2>)>) -> OrdMap<K, R>,
) -> OrdMap<K, R> {
    let (old_left_map, old_right_map, old_output) = match old {
        None => (OrdMap::new(), OrdMap::new(), OrdMap::new()),
        Some(x) => x,
    };
    let left_diff = old_left_map.symmetric_diff(new_left_map);
    let right_diff = old_right_map.symmetric_diff(new_right_map);
    // relies on the key iteration being sorted, as in BTreeMap.
    let merge = MergeOnceWith::new(
        left_diff,
        right_diff,
        |(k, _), (k2, _)| k.cmp(k2)
    );
    merge
        .fold(old_output, |output, merge_elem| {
            let key = match merge_elem {
                MergeElement::Left((key, _))| MergeElement::Right((key, _)) => key,
                MergeElement::Both((left_key, _), (right_key, _)) => {
                    // comparisons can be expensive
                    // assert_eq!(left_key, right_key);
                    left_key
                }
            };
            f(output, key, merge_elem)
        })
}

impl<K: Value + Ord, V: Value> Incr<OrdMap<K, V>> {
    pub fn incr_merge<F, V2, R>(&self, other: &Incr<OrdMap<K, V2>>, mut f: F) -> Incr<OrdMap<K, R>>
    where
        V2: Value,
        R: Value,
        F: FnMut(&K, MergeElement<&V, &V2>) -> Option<R> + 'static,
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
                        Left((_, left_diff)) => {
                            (left_diff.new_data(), new_right_map.get(key))
                        }
                        Right((_, right_diff)) => {
                            (new_left_map.get(key), right_diff.new_data())
                        }
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
                }
            );
            (output, did_change)
        });
        i.node.set_graphviz_user_data(Box::new(format!("incr_merge -> {}", std::any::type_name::<OrdMap<K, R>>())));
        i
    }
}


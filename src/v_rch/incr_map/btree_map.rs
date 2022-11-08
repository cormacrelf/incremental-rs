use std::{collections::BTreeMap, cell::RefCell, rc::Rc};

use crate::{Value, Incr, Cutoff, v_rch::incr_map::symmetric_fold::SymmetricFoldMap};
use super::symmetric_fold::{MergeElement, DiffElement, MergeOnceWith, SymmetricDiffMap};

pub(crate) fn merge_shared_impl<K: Clone + Ord, V1: Clone + PartialEq, V2: Clone + PartialEq, R: Clone>(
    old: Option<(BTreeMap<K, V1>, BTreeMap<K, V2>, BTreeMap<K, R>)>,
    new_left_map: &BTreeMap<K, V1>,
    new_right_map: &BTreeMap<K, V2>,
    mut f: impl FnMut(BTreeMap<K, R>, &K, MergeElement<(&K, DiffElement<&V1>), (&K, DiffElement<&V2>)>) -> BTreeMap<K, R>,
) -> BTreeMap<K, R> {
    let (old_left_map, old_right_map, old_output) = match old {
        None => (BTreeMap::new(), BTreeMap::new(), BTreeMap::new()),
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


impl<K: Value + Ord, V: Value> Incr<BTreeMap<K, V>> {
    pub fn incr_merge<F, V2, R>(&self, other: &Incr<BTreeMap<K, V2>>, mut f: F) -> Incr<BTreeMap<K, R>>
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
        i.node.set_graphviz_user_data(Box::new(format!("incr_merge -> {}", std::any::type_name::<BTreeMap<K, R>>())));
        i
    }

    pub fn incr_mapi_<F, V2>(&self, mut f: F, cutoff: Option<Cutoff<V>>) -> Incr<BTreeMap<K, V2>>
    where
        V2: Value,
        F: FnMut(&K, Incr<V>) -> Incr<V2> + 'static,
    {
        self.incr_filter_mapi_(move |k, incr_v| {
            f(k, incr_v).map(|x| Some(x.clone()))
        }, cutoff)
    }
    pub fn incr_filter_mapi_<F, V2>(&self, mut f: F, cutoff: Option<Cutoff<V>>) -> Incr<BTreeMap<K, V2>>
    where
        V2: Value,
        F: FnMut(&K, Incr<V>) -> Incr<Option<V2>> + 'static,
    {
        use crate::expert::{Node, Dependency};
        let state = self.state();
        let lhs = self;
        let incremental_state = lhs.state();
        let prev_map: Rc<RefCell<BTreeMap<K, V>>> = Rc::new(RefCell::new(BTreeMap::new()));
        let prev_nodes = Rc::new(RefCell::new(BTreeMap::<K, (Node<_, _>, Dependency<_>)>::new()));
        let acc = Rc::new(RefCell::new(BTreeMap::<K, V2>::new()));
        let result = Node::<BTreeMap<K, V2>, Option<V2>>::new(&state, {
            let acc_ = acc.clone();
            move || {
                acc_.borrow().clone()
            }
        });
        let on_inner_change = {
            let acc_ = acc.clone();
            move |key: &K, opt: Option<&V2>| {
                let mut acc = acc_.borrow_mut();
                match opt {
                    None => { acc.remove(key); }
                    Some(x) => { acc.insert(key.clone(), x.clone()); }
                }
                drop(acc);
            }
        };
        let lhs_change = lhs.map_cyclic({
            let prev_map_ = prev_map.clone();
            let prev_nodes_ = prev_nodes.clone();
            let acc_ = acc.clone();
            let result = result.clone();
            move |lhs_change, map| {
                let mut prev_map = prev_map_.borrow_mut();
                let mut prev_nodes = prev_nodes_.borrow_mut();
                let new_nodes = prev_map.symmetric_fold(map, &mut *prev_nodes, |nodes, (key, diff)| {
                    match diff {
                        DiffElement::Unequal(_, _) => {
                            let (node, dep) = nodes.get(key).unwrap();
                            node.make_stale();
                            nodes
                        }
                        DiffElement::Left(_) => {
                            let (node, dep) = nodes.remove(key).unwrap();
                            result.remove_dependency(dep);
                            let mut acc = acc_.borrow_mut();
                            acc.remove(key);
                            node.invalidate();
                            nodes
                        }
                        DiffElement::Right(_) => {
                            let key = key.clone();
                            let node = Node::<V>::new(&state, {
                                let key_ = key.clone();
                                let prev_map_ = prev_map_.clone();
                                move || {
                                    let prev_map = prev_map_.borrow();
                                    prev_map.get(&key_).unwrap().clone()
                                }
                            });
                            if let Some(cutoff) = cutoff {
                                node.watch().set_cutoff(cutoff);
                            }
                            let lhs_change = lhs_change.upgrade().unwrap();
                            node.add_dependency_unit(Dependency::new(&lhs_change));
                            let mapped = f(&key, node.watch());
                            let user_function_dep = Dependency::new_on_change(&mapped, {
                                let key = key.clone();
                                let on_inner_change = on_inner_change.clone();
                                move |v| on_inner_change(&key, v.as_ref())
                            });
                            result.add_dependency(user_function_dep.clone());
                            nodes.insert(key, (node, user_function_dep));
                            nodes
                        }
                    }
                });
                *prev_map = map.clone();
            }
        });
        result.add_dependency_unit(Dependency::new(&lhs_change));
        result.watch()
    }

}

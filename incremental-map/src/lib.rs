// We have some really complicated types. Most of them can't be typedef'd to be any shorter.
#![allow(clippy::type_complexity)]

use std::{cell::RefCell, marker::PhantomData};

use incremental::{Incr, Value};

pub mod btree_map;
#[cfg(feature = "im-rc")]
pub mod im_rc;
pub(crate) mod symmetric_fold;

pub use self::symmetric_fold::{DiffElement, MergeElement};

use symmetric_fold::{GenericMap, SymmetricFoldMap, SymmetricMapMap};

trait Operator<K, V, V2> {
    type Output;
    type Function;
    fn as_opt(output: &Self::Output) -> Option<&V2>;
    fn call_fn(&mut self, key: &K, input: Incr<V>) -> Incr<Self::Output>;
}

struct MapOperator<K, V, V2, F>(F, PhantomData<(K, V, V2)>)
where
    F: FnMut(&K, Incr<V>) -> Incr<V2>;

impl<K, V, V2, F> Operator<K, V, V2> for MapOperator<K, V, V2, F>
where
    F: FnMut(&K, Incr<V>) -> Incr<V2>,
{
    type Output = V2;
    type Function = F;
    #[inline]
    fn as_opt(output: &Self::Output) -> Option<&V2> {
        Some(output)
    }
    #[inline]
    fn call_fn(&mut self, key: &K, input: Incr<V>) -> Incr<Self::Output> {
        (self.0)(key, input)
    }
}

struct FilterMapOperator<K, V, V2, F>(F, PhantomData<(K, V, V2)>)
where
    F: FnMut(&K, Incr<V>) -> Incr<Option<V2>>;

impl<K, V, V2, F> Operator<K, V, V2> for FilterMapOperator<K, V, V2, F>
where
    F: FnMut(&K, Incr<V>) -> Incr<Option<V2>>,
{
    type Output = Option<V2>;
    type Function = F;
    #[inline]
    fn as_opt(output: &Self::Output) -> Option<&V2> {
        output.as_ref()
    }
    #[inline]
    fn call_fn(&mut self, key: &K, input: Incr<V>) -> Incr<Self::Output> {
        (self.0)(key, input)
    }
}

pub(crate) trait WithOldIO<T> {
    fn with_old_input_output<R, F>(&self, f: F) -> Incr<R>
    where
        R: Value,
        F: FnMut(Option<(T, R)>, &T) -> (R, bool) + 'static;

    fn with_old_input_output2<R, T2, F>(&self, other: &Incr<T2>, f: F) -> Incr<R>
    where
        R: Value,
        T2: Value,
        F: FnMut(Option<(T, T2, R)>, &T, &T2) -> (R, bool) + 'static;
}

impl<T: Value> WithOldIO<T> for Incr<T> {
    fn with_old_input_output<R, F>(&self, mut f: F) -> Incr<R>
    where
        R: Value,
        F: FnMut(Option<(T, R)>, &T) -> (R, bool) + 'static,
    {
        let old_input: RefCell<Option<T>> = RefCell::new(None);
        self.map_with_old(move |old_opt, a| {
            let mut oi = old_input.borrow_mut();
            let (b, didchange) = f(old_opt.and_then(|x| oi.take().map(|oi| (oi, x))), a);
            *oi = Some(a.clone());
            (b, didchange)
        })
    }

    fn with_old_input_output2<R, T2, F>(&self, other: &Incr<T2>, mut f: F) -> Incr<R>
    where
        R: Value,
        T2: Value,
        F: FnMut(Option<(T, T2, R)>, &T, &T2) -> (R, bool) + 'static,
    {
        let old_input: RefCell<Option<(T, T2)>> = RefCell::new(None);
        // TODO: too much cloning
        self.map2(other, |a, b| (a.clone(), b.clone()))
            .map_with_old(move |old_opt, (a, b)| {
                let mut oi = old_input.borrow_mut();
                let (r, didchange) = f(
                    old_opt.and_then(|x| oi.take().map(|(oia, oib)| (oia, oib, x))),
                    a,
                    b,
                );
                *oi = Some((a.clone(), b.clone()));
                (r, didchange)
            })
    }
}

pub trait Symmetric<T> {
    fn incr_map<F, K, V, V2>(&self, f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&V) -> V2 + 'static,
        T::OutputMap<V2>: Value;

    fn incr_filter_map<F, K, V, V2>(&self, f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&V) -> Option<V2> + 'static,
        T::OutputMap<V2>: Value;

    fn incr_mapi<F, K, V, V2>(&self, f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&K, &V) -> V2 + 'static,
        T::OutputMap<V2>: Value;

    fn incr_filter_mapi<F, K, V, V2>(&self, f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&K, &V) -> Option<V2> + 'static,
        T::OutputMap<V2>: Value;

    fn incr_unordered_fold<FAdd, FRemove, K, V, R>(
        &self,
        init: R,
        add: FAdd,
        remove: FRemove,
        revert_to_init_when_empty: bool,
    ) -> Incr<R>
    where
        T: SymmetricFoldMap<K, V>,
        K: Value + Ord,
        V: Value,
        R: Value,
        FAdd: FnMut(R, &K, &V) -> R + 'static,
        FRemove: FnMut(R, &K, &V) -> R + 'static;
}

impl<T: Value> Symmetric<T> for Incr<T> {
    #[inline]
    fn incr_map<F, K, V, V2>(&self, mut f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&V) -> V2 + 'static,
        T::OutputMap<V2>: Value,
    {
        let i = self.incr_filter_mapi(move |_k, v| Some(f(v)));
        #[cfg(debug_assertions)]
        i.set_graphviz_user_data(Box::new(format!(
            "incr_map -> {}",
            std::any::type_name::<T::OutputMap<V2>>()
        )));
        i
    }

    #[inline]
    fn incr_filter_map<F, K, V, V2>(&self, mut f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&V) -> Option<V2> + 'static,
        T::OutputMap<V2>: Value,
    {
        let i = self.incr_filter_mapi(move |_k, v| f(v));
        #[cfg(debug_assertions)]
        i.set_graphviz_user_data(Box::new(format!(
            "incr_filter_map -> {}",
            std::any::type_name::<T::OutputMap<V2>>()
        )));
        i
    }

    #[inline]
    fn incr_mapi<F, K, V, V2>(&self, mut f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&K, &V) -> V2 + 'static,
        T::OutputMap<V2>: Value,
    {
        let i = self.incr_filter_mapi(move |k, v| Some(f(k, v)));
        #[cfg(debug_assertions)]
        i.set_graphviz_user_data(Box::new(format!(
            "incr_mapi -> {}",
            std::any::type_name::<T::OutputMap<V2>>()
        )));
        i
    }

    fn incr_filter_mapi<F, K, V, V2>(&self, mut f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&K, &V) -> Option<V2> + 'static,
        T::OutputMap<V2>: Value,
    {
        let i = self.with_old_input_output(move |old, input| match (old, input.len()) {
            (_, 0) | (None, _) => (input.filter_map_collect(&mut f), true),
            (Some((old_in, mut old_out)), _) => {
                let mut did_change = false;
                let old_out_mut = old_out.make_mut();
                let _: &mut <T::OutputMap<V2> as SymmetricMapMap<K, V2>>::UnderlyingMap = old_in
                    .symmetric_fold(input, old_out_mut, |out, (key, change)| {
                        did_change = true;
                        match change {
                            DiffElement::Left(_) => {
                                out.remove(key);
                            }
                            DiffElement::Right(newval) => {
                                if let Some(v2) = f(key, newval) {
                                    out.insert(key.clone(), v2);
                                } else {
                                    out.remove(key);
                                }
                            }
                            DiffElement::Unequal(_, newval) => {
                                if let Some(v2) = f(key, newval) {
                                    out.insert(key.clone(), v2);
                                } else {
                                    out.remove(key);
                                }
                            }
                        }
                        out
                    });
                (old_out, did_change)
            }
        });
        #[cfg(debug_assertions)]
        i.set_graphviz_user_data(Box::new(format!(
            "incr_filter_mapi -> {}",
            std::any::type_name::<T::OutputMap<V2>>()
        )));
        i
    }

    fn incr_unordered_fold<FAdd, FRemove, K, V, R>(
        &self,
        init: R,
        mut add: FAdd,
        mut remove: FRemove,
        revert_to_init_when_empty: bool,
    ) -> Incr<R>
    where
        T: SymmetricFoldMap<K, V>,
        K: Value + Ord,
        V: Value,
        R: Value,
        FAdd: FnMut(R, &K, &V) -> R + 'static,
        FRemove: FnMut(R, &K, &V) -> R + 'static,
    {
        let i = self.with_old_input_output(move |old, new_in| match old {
            None => {
                let newmap = new_in.nonincremental_fold(init.clone(), |acc, (k, v)| add(acc, k, v));
                (newmap, true)
            }
            Some((old_in, old_out)) => {
                if revert_to_init_when_empty && new_in.is_empty() {
                    return (init.clone(), !old_in.is_empty());
                }
                let mut did_change = false;
                let folded: R =
                    old_in.symmetric_fold(new_in, old_out, |mut acc, (key, difference)| {
                        did_change = true;
                        match difference {
                            DiffElement::Left(value) => remove(acc, key, value),
                            DiffElement::Right(value) => add(acc, key, value),
                            DiffElement::Unequal(lv, rv) => {
                                acc = remove(acc, key, lv);
                                add(acc, key, rv)
                            }
                        }
                    });
                (folded, did_change)
            }
        });
        #[cfg(debug_assertions)]
        i.set_graphviz_user_data(Box::new(format!(
            "incr_unordered_fold -> {}",
            std::any::type_name::<R>()
        )));
        i
    }
}

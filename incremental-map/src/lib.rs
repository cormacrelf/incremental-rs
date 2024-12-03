// We have some really complicated types. Most of them can't be typedef'd to be any shorter.
#![allow(clippy::type_complexity)]

use std::marker::PhantomData;

use incremental::incrsan::NotObserver;
use incremental::{Incr, Value};

pub mod btree_map;
#[cfg(feature = "im")]
pub mod im_rc;
pub mod symmetric_fold;

pub use self::symmetric_fold::{DiffElement, MergeElement};

pub mod prelude {
    pub use super::btree_map::IncrBTreeMap;
    #[cfg(feature = "im")]
    pub use super::im_rc::IncrOrdMap;
    pub use super::symmetric_fold::DiffElement;
    pub use super::symmetric_fold::GenericMap;
    pub use super::symmetric_fold::MergeElement;
    pub use super::symmetric_fold::MutableMap;
    pub use super::symmetric_fold::SymmetricFoldMap;
    pub use super::symmetric_fold::SymmetricMapMap;
    pub use super::IncrMap;
    pub use super::UnorderedFold;
}

use symmetric_fold::{GenericMap, MutableMap, SymmetricFoldMap, SymmetricMapMap};

trait Operator<K, V, V2> {
    type Output;
    type Function;
    fn as_opt(output: &Self::Output) -> Option<&V2>;
    fn call_fn(&mut self, key: &K, input: Incr<V>) -> Incr<Self::Output>;
}

struct MapOperator<K, V, V2, F>(F, PhantomData<(K, V, V2)>)
where
    F: FnMut(&K, Incr<V>) -> Incr<V2> + NotObserver;

impl<K, V, V2, F> Operator<K, V, V2> for MapOperator<K, V, V2, F>
where
    F: FnMut(&K, Incr<V>) -> Incr<V2> + NotObserver,
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
    F: FnMut(&K, Incr<V>) -> Incr<Option<V2>> + NotObserver;

impl<K, V, V2, F> Operator<K, V, V2> for FilterMapOperator<K, V, V2, F>
where
    F: FnMut(&K, Incr<V>) -> Incr<Option<V2>> + NotObserver,
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

/// Internal -- variations on map_with_old
pub(crate) trait WithOldIO<T> {
    fn with_old_input_output<R, F>(&self, f: F) -> Incr<R>
    where
        R: Value,
        F: FnMut(Option<(T, R)>, &T) -> (R, bool) + 'static + NotObserver;

    fn with_old_input_output2<R, T2, F>(&self, other: &Incr<T2>, f: F) -> Incr<R>
    where
        R: Value,
        T2: Value,
        F: FnMut(Option<(T, T2, R)>, &T, &T2) -> (R, bool) + 'static + NotObserver;
}

impl<T: Value> WithOldIO<T> for Incr<T> {
    fn with_old_input_output<R, F>(&self, mut f: F) -> Incr<R>
    where
        R: Value,
        F: FnMut(Option<(T, R)>, &T) -> (R, bool) + 'static + NotObserver,
    {
        let mut old_input: Option<T> = None;
        self.map_with_old(move |old_opt, a| {
            let oi = &mut old_input;
            let (b, didchange) = f(old_opt.and_then(|x| oi.take().map(|oi| (oi, x))), a);
            *oi = Some(a.clone());
            (b, didchange)
        })
    }

    fn with_old_input_output2<R, T2, F>(&self, other: &Incr<T2>, mut f: F) -> Incr<R>
    where
        R: Value,
        T2: Value,
        F: FnMut(Option<(T, T2, R)>, &T, &T2) -> (R, bool) + 'static + NotObserver,
    {
        let mut old_input: Option<(T, T2)> = None;
        // TODO: too much cloning
        self.zip(other).map_with_old(move |old_opt, (a, b)| {
            let oi = &mut old_input;
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
/// Incremental maps, filter maps and folds on incremental key-value containers (maps).
///
/// Common functions available on `Incr<BTreeMap>` etc, with blanket
/// implementation covering `M: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>`.
///
/// So to get a lot of functionality for a new map type, you don't have to
/// implement much. Just those two traits, and then you get all these methods
/// for free.
///
/// **NOTE**: there are additional methods available on [crate::prelude::IncrBTreeMap] and
/// [crate::prelude::IncrOrdMap] (with the `im` feature).
///
pub trait IncrMap<M> {
    fn incr_map<F, K, V, V2>(&self, f: F) -> Incr<M::OutputMap<V2>>
    where
        M: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&V) -> V2 + 'static + NotObserver,
        M::OutputMap<V2>: Value;

    fn incr_filter_map<F, K, V, V2>(&self, f: F) -> Incr<M::OutputMap<V2>>
    where
        M: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&V) -> Option<V2> + 'static + NotObserver,
        M::OutputMap<V2>: Value;

    fn incr_mapi<F, K, V, V2>(&self, f: F) -> Incr<M::OutputMap<V2>>
    where
        M: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&K, &V) -> V2 + 'static + NotObserver,
        M::OutputMap<V2>: Value;

    fn incr_filter_mapi<F, K, V, V2>(&self, f: F) -> Incr<M::OutputMap<V2>>
    where
        M: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&K, &V) -> Option<V2> + 'static + NotObserver,
        M::OutputMap<V2>: Value;

    fn incr_unordered_fold_with<K, V, R, F>(&self, init: R, fold: F) -> Incr<R>
    where
        M: SymmetricFoldMap<K, V>,
        K: Value + Ord,
        V: Value,
        R: Value,
        F: UnorderedFold<M, K, V, R> + 'static + NotObserver;

    fn incr_unordered_fold<FAdd, FRemove, K, V, R>(
        &self,
        init: R,
        add: FAdd,
        remove: FRemove,
        revert_to_init_when_empty: bool,
    ) -> Incr<R>
    where
        M: SymmetricFoldMap<K, V>,
        K: Value + Ord,
        V: Value,
        R: Value,
        FAdd: FnMut(R, &K, &V) -> R + 'static + NotObserver,
        FRemove: FnMut(R, &K, &V) -> R + 'static + NotObserver;

    fn incr_unordered_fold_update<FAdd, FRemove, FUpdate, K, V, R>(
        &self,
        init: R,
        add: FAdd,
        remove: FRemove,
        update: FUpdate,
        revert_to_init_when_empty: bool,
    ) -> Incr<R>
    where
        M: SymmetricFoldMap<K, V>,
        K: Value + Ord,
        V: Value,
        R: Value,
        FAdd: FnMut(R, &K, &V) -> R + 'static + NotObserver,
        FRemove: FnMut(R, &K, &V) -> R + 'static + NotObserver,
        FUpdate: FnMut(R, &K, &V, &V) -> R + 'static + NotObserver;
}

impl<M: Value> IncrMap<M> for Incr<M> {
    #[inline]
    fn incr_map<F, K, V, V2>(&self, mut f: F) -> Incr<M::OutputMap<V2>>
    where
        M: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&V) -> V2 + 'static + NotObserver,
        M::OutputMap<V2>: Value,
    {
        let i = self.incr_filter_mapi(move |_k, v| Some(f(v)));
        #[cfg(debug_assertions)]
        i.set_graphviz_user_data(Box::new(format!(
            "incr_map -> {}",
            std::any::type_name::<M::OutputMap<V2>>()
        )));
        i
    }

    #[inline]
    fn incr_filter_map<F, K, V, V2>(&self, mut f: F) -> Incr<M::OutputMap<V2>>
    where
        M: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&V) -> Option<V2> + 'static + NotObserver,
        M::OutputMap<V2>: Value,
    {
        let i = self.incr_filter_mapi(move |_k, v| f(v));
        #[cfg(debug_assertions)]
        i.set_graphviz_user_data(Box::new(format!(
            "incr_filter_map -> {}",
            std::any::type_name::<M::OutputMap<V2>>()
        )));
        i
    }

    #[inline]
    fn incr_mapi<F, K, V, V2>(&self, mut f: F) -> Incr<M::OutputMap<V2>>
    where
        M: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&K, &V) -> V2 + 'static + NotObserver,
        M::OutputMap<V2>: Value,
    {
        let i = self.incr_filter_mapi(move |k, v| Some(f(k, v)));
        #[cfg(debug_assertions)]
        i.set_graphviz_user_data(Box::new(format!(
            "incr_mapi -> {}",
            std::any::type_name::<M::OutputMap<V2>>()
        )));
        i
    }

    fn incr_filter_mapi<F, K, V, V2>(&self, mut f: F) -> Incr<M::OutputMap<V2>>
    where
        M: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&K, &V) -> Option<V2> + 'static + NotObserver,
        M::OutputMap<V2>: Value,
    {
        let i = self.with_old_input_output(move |old, input| match (old, input.len()) {
            (_, 0) | (None, _) => (input.filter_map_collect(&mut f), true),
            (Some((old_in, mut old_out)), _) => {
                let mut did_change = false;
                let old_out_mut = old_out.make_mut();
                let _: &mut <M::OutputMap<V2> as MutableMap<K, V2>>::UnderlyingMap = old_in
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
            std::any::type_name::<M::OutputMap<V2>>()
        )));
        i
    }

    fn incr_unordered_fold<FAdd, FRemove, K, V, R>(
        &self,
        init: R,
        add: FAdd,
        remove: FRemove,
        revert_to_init_when_empty: bool,
    ) -> Incr<R>
    where
        M: SymmetricFoldMap<K, V>,
        K: Value + Ord,
        V: Value,
        R: Value,
        FAdd: FnMut(R, &K, &V) -> R + 'static + NotObserver,
        FRemove: FnMut(R, &K, &V) -> R + 'static + NotObserver,
    {
        self.incr_unordered_fold_with(
            init,
            PlainUnorderedFold {
                add,
                remove,
                revert_to_init_when_empty,
                phantom: PhantomData,
            },
        )
    }

    fn incr_unordered_fold_update<FAdd, FRemove, FUpdate, K, V, R>(
        &self,
        init: R,
        add: FAdd,
        remove: FRemove,
        update: FUpdate,
        revert_to_init_when_empty: bool,
    ) -> Incr<R>
    where
        M: SymmetricFoldMap<K, V>,
        K: Value + Ord,
        V: Value,
        R: Value,
        FAdd: FnMut(R, &K, &V) -> R + 'static + NotObserver,
        FRemove: FnMut(R, &K, &V) -> R + 'static + NotObserver,
        FUpdate: FnMut(R, &K, &V, &V) -> R + 'static + NotObserver,
    {
        let i = self.incr_unordered_fold_with(
            init,
            UpdateUnorderedFold {
                add,
                remove,
                update,
                revert_to_init_when_empty,
                phantom: PhantomData,
            },
        );
        #[cfg(debug_assertions)]
        i.set_graphviz_user_data(Box::new(format!(
            "incr_unordered_fold_update -> {}",
            std::any::type_name::<R>()
        )));
        i
    }

    fn incr_unordered_fold_with<K, V, R, F>(&self, init: R, mut fold: F) -> Incr<R>
    where
        M: SymmetricFoldMap<K, V>,
        K: Value + Ord,
        V: Value,
        R: Value,
        F: UnorderedFold<M, K, V, R> + 'static + NotObserver,
    {
        let i = self.with_old_input_output(move |old, new_in| match old {
            None => {
                let newmap = fold.initial_fold(init.clone(), new_in);
                (newmap, true)
            }
            Some((old_in, old_out)) => {
                if fold.revert_to_init_when_empty() && new_in.is_empty() {
                    return (init.clone(), !old_in.is_empty());
                }
                let mut did_change = false;
                let folded: R = old_in.symmetric_fold(new_in, old_out, |acc, (key, difference)| {
                    did_change = true;
                    match difference {
                        DiffElement::Left(value) => fold.remove(acc, key, value),
                        DiffElement::Right(value) => fold.add(acc, key, value),
                        DiffElement::Unequal(lv, rv) => fold.update(acc, key, lv, rv),
                    }
                });
                (folded, did_change)
            }
        });
        #[cfg(debug_assertions)]
        i.set_graphviz_user_data(Box::new(format!(
            "incr_unordered_fold_with -> {}",
            std::any::type_name::<R>()
        )));
        i
    }
}

/// Defines an unordered fold for a given map, key, value and output type.
///
/// Used with [IncrMap::incr_unordered_fold_with].
///
/// Implementations get &mut access to self. So you can store things in
/// the type that implements this.
pub trait UnorderedFold<M, K, V, R>
where
    M: SymmetricFoldMap<K, V>,
    K: Value + Ord,
    V: Value,
{
    /// How to add a key/value pair to the fold value
    ///
    /// E.g. `|acc, _, value| acc + value` for a signed integer.
    fn add(&mut self, acc: R, key: &K, value: &V) -> R;

    /// How to remove a key/value pair from the fold value
    ///
    /// E.g. `|acc, _, value| acc - value` for a signed integer.
    fn remove(&mut self, acc: R, key: &K, value: &V) -> R;

    /// Default implementation is `self.add(self.remove(acc, key, old), key, new)`
    fn update(&mut self, mut acc: R, key: &K, old: &V, new: &V) -> R {
        acc = self.remove(acc, key, old);
        self.add(acc, key, new)
    }

    /// If we have emptied the map, can we just reset to the initial value?
    /// Or do we have to call remove() on everything that was removed?
    #[inline]
    fn revert_to_init_when_empty(&self) -> bool {
        false
    }

    /// Optimize the initial fold
    fn initial_fold(&mut self, acc: R, input: &M) -> R {
        input.nonincremental_fold(acc, |acc, (k, v)| self.add(acc, k, v))
    }
}

struct PlainUnorderedFold<M, K, V, R, FAdd, FRemove> {
    add: FAdd,
    remove: FRemove,
    revert_to_init_when_empty: bool,
    phantom: PhantomData<(M, K, V, R)>,
}

impl<M, K: Value + Ord, V: Value, R: Value, FAdd, FRemove> UnorderedFold<M, K, V, R>
    for PlainUnorderedFold<M, K, V, R, FAdd, FRemove>
where
    M: SymmetricFoldMap<K, V>,
    FAdd: FnMut(R, &K, &V) -> R + 'static + NotObserver,
    FRemove: FnMut(R, &K, &V) -> R + 'static + NotObserver,
{
    fn add(&mut self, acc: R, key: &K, value: &V) -> R {
        (self.add)(acc, key, value)
    }
    fn remove(&mut self, acc: R, key: &K, value: &V) -> R {
        (self.remove)(acc, key, value)
    }
    fn revert_to_init_when_empty(&self) -> bool {
        self.revert_to_init_when_empty
    }
}

struct UpdateUnorderedFold<M, K, V, R, FAdd, FRemove, FUpdate> {
    add: FAdd,
    remove: FRemove,
    update: FUpdate,
    revert_to_init_when_empty: bool,
    phantom: PhantomData<(M, K, V, R)>,
}

impl<M, K: Value + Ord, V: Value, R: Value, FAdd, FRemove, FUpdate> UnorderedFold<M, K, V, R>
    for UpdateUnorderedFold<M, K, V, R, FAdd, FRemove, FUpdate>
where
    M: SymmetricFoldMap<K, V>,
    FAdd: FnMut(R, &K, &V) -> R + 'static + NotObserver,
    FUpdate: FnMut(R, &K, &V, &V) -> R + 'static + NotObserver,
    FRemove: FnMut(R, &K, &V) -> R + 'static + NotObserver,
{
    fn add(&mut self, acc: R, key: &K, value: &V) -> R {
        (self.add)(acc, key, value)
    }
    fn remove(&mut self, acc: R, key: &K, value: &V) -> R {
        (self.remove)(acc, key, value)
    }
    fn update(&mut self, acc: R, key: &K, old: &V, new: &V) -> R {
        (self.update)(acc, key, old, new)
    }
    fn revert_to_init_when_empty(&self) -> bool {
        self.revert_to_init_when_empty
    }
}

/// An implementation of [UnorderedFold] using a builder pattern and closures.
pub struct ClosureFold<M, K, V, R, FAdd, FRemove, FUpdate, FInitial> {
    add: FAdd,
    remove: FRemove,
    update: Option<FUpdate>,
    specialized_initial: Option<FInitial>,
    revert_to_init_when_empty: bool,
    phantom: PhantomData<(M, K, V, R)>,
}

impl ClosureFold<(), (), (), (), (), (), (), ()> {
    pub fn new<M, K, V, R>(
    ) -> ClosureFold<M, K, V, R, (), (), fn(R, &K, &V, &V) -> R, fn(R, &M) -> R>
where {
        ClosureFold::<M, K, V, R, _, _, _, _> {
            add: (),
            remove: (),
            update: None,
            specialized_initial: None,
            revert_to_init_when_empty: false,
            phantom: PhantomData,
        }
    }

    pub fn new_add_remove<M, K, V, R, FAdd, FRemove>(
        add: FAdd,
        remove: FRemove,
    ) -> ClosureFold<M, K, V, R, FAdd, FRemove, fn(R, &K, &V, &V) -> R, fn(R, &M) -> R>
    where
        FAdd: for<'a> FnMut(R, &'a K, &'a V) -> R + 'static + NotObserver,
        FRemove: for<'a> FnMut(R, &'a K, &'a V) -> R + 'static + NotObserver,
    {
        ClosureFold {
            add,
            remove,
            update: None,
            specialized_initial: None,
            revert_to_init_when_empty: false,
            phantom: PhantomData,
        }
    }
}

impl<M, K, V, R, FAdd_, FRemove_, FUpdate_, FInitial_>
    ClosureFold<M, K, V, R, FAdd_, FRemove_, FUpdate_, FInitial_>
{
    pub fn add<FAdd>(
        self,
        add: FAdd,
    ) -> ClosureFold<M, K, V, R, FAdd, FRemove_, FUpdate_, FInitial_>
    where
        FAdd: for<'a> FnMut(R, &'a K, &'a V) -> R + 'static + NotObserver,
    {
        ClosureFold {
            add,
            remove: self.remove,
            update: None,
            specialized_initial: None,
            revert_to_init_when_empty: false,
            phantom: PhantomData,
        }
    }
    pub fn remove<FRemove>(
        self,
        remove: FRemove,
    ) -> ClosureFold<M, K, V, R, FAdd_, FRemove, FUpdate_, FInitial_>
    where
        FRemove: for<'a> FnMut(R, &'a K, &'a V) -> R + 'static + NotObserver,
    {
        ClosureFold {
            add: self.add,
            remove,
            update: None,
            specialized_initial: None,
            revert_to_init_when_empty: false,
            phantom: PhantomData,
        }
    }
    pub fn update<FUpdate>(
        self,
        update: FUpdate,
    ) -> ClosureFold<M, K, V, R, FAdd_, FRemove_, FUpdate, FInitial_>
    where
        FUpdate: for<'a> FnMut(R, &'a K, &'a V, &'a V) -> R + 'static + NotObserver,
    {
        ClosureFold {
            add: self.add,
            remove: self.remove,
            update: Some(update),
            specialized_initial: self.specialized_initial,
            revert_to_init_when_empty: self.revert_to_init_when_empty,
            phantom: self.phantom,
        }
    }

    pub fn specialized_initial<FInitial>(
        self,
        specialized_initial: FInitial,
    ) -> ClosureFold<M, K, V, R, FAdd_, FRemove_, FUpdate_, FInitial>
    where
        FInitial: for<'a> FnMut(R, &'a M) -> R + 'static + NotObserver,
    {
        ClosureFold {
            add: self.add,
            remove: self.remove,
            update: self.update,
            specialized_initial: Some(specialized_initial),
            revert_to_init_when_empty: self.revert_to_init_when_empty,
            phantom: self.phantom,
        }
    }
    pub fn revert_to_init_when_empty(
        self,
        revert_to_init_when_empty: bool,
    ) -> ClosureFold<M, K, V, R, FAdd_, FRemove_, FUpdate_, FInitial_> {
        ClosureFold {
            add: self.add,
            remove: self.remove,
            update: self.update,
            specialized_initial: self.specialized_initial,
            revert_to_init_when_empty,
            phantom: self.phantom,
        }
    }
}

impl<M, K, V, R, FAdd, FRemove, FUpdate, FInitial> UnorderedFold<M, K, V, R>
    for ClosureFold<M, K, V, R, FAdd, FRemove, FUpdate, FInitial>
where
    M: SymmetricFoldMap<K, V>,
    K: Value + Ord,
    V: Value,
    R: Value,
    FAdd: for<'a> FnMut(R, &'a K, &'a V) -> R + 'static + NotObserver,
    FRemove: for<'a> FnMut(R, &'a K, &'a V) -> R + 'static + NotObserver,
    FUpdate: for<'a> FnMut(R, &'a K, &'a V, &'a V) -> R + 'static + NotObserver,
    FInitial: for<'a> FnMut(R, &'a M) -> R + 'static + NotObserver,
{
    fn add(&mut self, acc: R, key: &K, value: &V) -> R {
        (self.add)(acc, key, value)
    }

    fn remove(&mut self, acc: R, key: &K, value: &V) -> R {
        (self.remove)(acc, key, value)
    }

    fn update(&mut self, acc: R, key: &K, old: &V, new: &V) -> R {
        if let Some(closure) = &mut self.update {
            closure(acc, key, old, new)
        } else {
            let r = self.remove(acc, key, old);
            let r = self.add(r, key, new);
            r
        }
    }

    fn revert_to_init_when_empty(&self) -> bool {
        self.revert_to_init_when_empty
    }

    fn initial_fold(&mut self, init: R, input: &M) -> R {
        if let Some(closure) = &mut self.specialized_initial {
            closure(init, input)
        } else {
            input.nonincremental_fold(init, |acc, (k, v)| self.add(acc, k, v))
        }
    }
}

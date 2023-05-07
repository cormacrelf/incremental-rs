// We have some really complicated types. Most of them can't be typedef'd to be any shorter.
#![allow(clippy::type_complexity)]

use std::{cell::RefCell, marker::PhantomData, rc::Rc};

use incremental::incrsan::NotObserver;
use incremental::{Incr, Value};

pub mod btree_map;
#[cfg(feature = "im")]
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
        F: FnMut(Option<(T, T2, R)>, &T, &T2) -> (R, bool) + 'static + NotObserver,
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
        F: FnMut(&V) -> V2 + 'static + NotObserver,
        T::OutputMap<V2>: Value;

    fn incr_filter_map<F, K, V, V2>(&self, f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&V) -> Option<V2> + 'static + NotObserver,
        T::OutputMap<V2>: Value;

    fn incr_mapi<F, K, V, V2>(&self, f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&K, &V) -> V2 + 'static + NotObserver,
        T::OutputMap<V2>: Value;

    fn incr_filter_mapi<F, K, V, V2>(&self, f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&K, &V) -> Option<V2> + 'static + NotObserver,
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
        T: SymmetricFoldMap<K, V>,
        K: Value + Ord,
        V: Value,
        R: Value,
        FAdd: FnMut(R, &K, &V) -> R + 'static + NotObserver,
        FRemove: FnMut(R, &K, &V) -> R + 'static + NotObserver,
        FUpdate: FnMut(R, &K, &V, &V) -> R + 'static + NotObserver;
}

impl<T: Value> Symmetric<T> for Incr<T> {
    #[inline]
    fn incr_map<F, K, V, V2>(&self, mut f: F) -> Incr<T::OutputMap<V2>>
    where
        T: SymmetricFoldMap<K, V> + SymmetricMapMap<K, V>,
        K: Value + Ord,
        V: Value,
        V2: Value,
        F: FnMut(&V) -> V2 + 'static + NotObserver,
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
        F: FnMut(&V) -> Option<V2> + 'static + NotObserver,
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
        F: FnMut(&K, &V) -> V2 + 'static + NotObserver,
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
        F: FnMut(&K, &V) -> Option<V2> + 'static + NotObserver,
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
        add: FAdd,
        remove: FRemove,
        revert_to_init_when_empty: bool,
    ) -> Incr<R>
    where
        T: SymmetricFoldMap<K, V>,
        K: Value + Ord,
        V: Value,
        R: Value,
        FAdd: FnMut(R, &K, &V) -> R + 'static + NotObserver,
        FRemove: FnMut(R, &K, &V) -> R + 'static + NotObserver,
    {
        let add = Rc::new(RefCell::new(add));
        let add2 = add.clone();
        let remove = Rc::new(RefCell::new(remove));
        let remove2 = remove.clone();
        self.incr_unordered_fold_update(
            init,
            move |acc, key, data| add.borrow_mut()(acc, key, data),
            move |acc, key, data| remove.borrow_mut()(acc, key, data),
            move |acc, key, old, new| {
                let acc = remove2.borrow_mut()(acc, key, old);
                add2.borrow_mut()(acc, key, new)
            },
            revert_to_init_when_empty,
        )
    }

    fn incr_unordered_fold_update<FAdd, FRemove, FUpdate, K, V, R>(
        &self,
        init: R,
        mut add: FAdd,
        mut remove: FRemove,
        mut update: FUpdate,
        revert_to_init_when_empty: bool,
    ) -> Incr<R>
    where
        T: SymmetricFoldMap<K, V>,
        K: Value + Ord,
        V: Value,
        R: Value,
        FAdd: FnMut(R, &K, &V) -> R + 'static + NotObserver,
        FRemove: FnMut(R, &K, &V) -> R + 'static + NotObserver,
        FUpdate: FnMut(R, &K, &V, &V) -> R + 'static + NotObserver,
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
                let folded: R = old_in.symmetric_fold(new_in, old_out, |acc, (key, difference)| {
                    did_change = true;
                    match difference {
                        DiffElement::Left(value) => remove(acc, key, value),
                        DiffElement::Right(value) => add(acc, key, value),
                        DiffElement::Unequal(lv, rv) => update(acc, key, lv, rv),
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

pub trait IncrUnorderedFoldWith<K: Value + Ord, V: Value> {
    fn incr_unordered_fold_with<R, F>(&self, init: R, fold: F) -> Incr<R>
    where
        R: Value,
        F: UnorderedFold<K, V, R> + 'static + NotObserver;
}

impl<K: Value + Ord, V: Value> IncrUnorderedFoldWith<K, V> for Incr<::im_rc::OrdMap<K, V>> {
    fn incr_unordered_fold_with<R, F>(&self, init: R, mut fold: F) -> Incr<R>
    where
        R: Value,
        F: UnorderedFold<K, V, R> + 'static + NotObserver,
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
            "incr_unordered_fold -> {}",
            std::any::type_name::<R>()
        )));
        i
    }
}

pub trait UnorderedFold<K, V, R>
where
    K: Value + Ord,
    V: Value,
{
    fn add(&mut self, acc: R, key: &K, value: &V) -> R;
    fn remove(&mut self, acc: R, key: &K, value: &V) -> R;
    fn update(&mut self, acc: R, key: &K, old: &V, new: &V) -> R {
        let r = self.remove(acc, key, old);
        let r = self.add(r, key, new);
        r
    }
    #[inline]
    fn revert_to_init_when_empty(&self) -> bool {
        false
    }
    fn initial_fold(&mut self, acc: R, input: &::im_rc::OrdMap<K, V>) -> R {
        input.iter().fold(acc, |acc, (k, v)| self.add(acc, k, v))
    }
}

/// An implementation of [UnorderedFold] using a builder pattern and closures.
pub struct ClosureFold<K, V, R, FAdd, FRemove, FUpdate, FInitial> {
    add: FAdd,
    remove: FRemove,
    update: Option<FUpdate>,
    specialized_initial: Option<FInitial>,
    revert_to_init_when_empty: bool,
    phantom: PhantomData<(K, V, R)>,
}

impl ClosureFold<(), (), (), (), (), (), ()> {
    pub fn new<K, V, R>(
    ) -> ClosureFold<K, V, R, (), (), fn(R, &K, &V, &V) -> R, fn(R, &::im_rc::OrdMap<K, V>) -> R>
where {
        ClosureFold {
            add: (),
            remove: (),
            update: None,
            specialized_initial: None,
            revert_to_init_when_empty: false,
            phantom: PhantomData,
        }
    }

    pub fn new_add_remove<K, V, R, FAdd, FRemove>(
        add: FAdd,
        remove: FRemove,
    ) -> ClosureFold<
        K,
        V,
        R,
        FAdd,
        FRemove,
        fn(R, &K, &V, &V) -> R,
        fn(R, &::im_rc::OrdMap<K, V>) -> R,
    >
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

impl<K, V, R, FAdd_, FRemove_, FUpdate_, FInitial_>
    ClosureFold<K, V, R, FAdd_, FRemove_, FUpdate_, FInitial_>
{
    pub fn add<FAdd>(self, add: FAdd) -> ClosureFold<K, V, R, FAdd, FRemove_, FUpdate_, FInitial_>
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
    ) -> ClosureFold<K, V, R, FAdd_, FRemove, FUpdate_, FInitial_>
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
    ) -> ClosureFold<K, V, R, FAdd_, FRemove_, FUpdate, FInitial_>
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
    ) -> ClosureFold<K, V, R, FAdd_, FRemove_, FUpdate_, FInitial>
    where
        FInitial: for<'a> FnMut(R, &'a ::im_rc::OrdMap<K, V>) -> R + 'static + NotObserver,
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
    ) -> ClosureFold<K, V, R, FAdd_, FRemove_, FUpdate_, FInitial_> {
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

impl<K, V, R, FAdd, FRemove, FUpdate, FInitial> UnorderedFold<K, V, R>
    for ClosureFold<K, V, R, FAdd, FRemove, FUpdate, FInitial>
where
    K: Value + Ord,
    V: Value,
    R: Value,
    FAdd: for<'a> FnMut(R, &'a K, &'a V) -> R + 'static + NotObserver,
    FRemove: for<'a> FnMut(R, &'a K, &'a V) -> R + 'static + NotObserver,
    FUpdate: for<'a> FnMut(R, &'a K, &'a V, &'a V) -> R + 'static + NotObserver,
    FInitial: for<'a> FnMut(R, &'a ::im_rc::OrdMap<K, V>) -> R + 'static + NotObserver,
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

    fn initial_fold(&mut self, init: R, input: &::im_rc::OrdMap<K, V>) -> R {
        if let Some(closure) = &mut self.specialized_initial {
            closure(init, input)
        } else {
            input.iter().fold(init, |acc, (k, v)| self.add(acc, k, v))
        }
    }
}

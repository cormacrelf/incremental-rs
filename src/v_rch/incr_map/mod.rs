use std::marker::PhantomData;

use crate::Incr;

#[cfg(feature = "im-rc")]
pub(crate) mod im_rc;

pub(crate) mod btree_map;
pub(crate) mod symmetric_fold;

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
    F: FnMut(&K, Incr<V>) -> Incr<V2>
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
    F: FnMut(&K, Incr<V>) -> Incr<Option<V2>>
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


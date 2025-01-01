use std::cell::RefCell;
use std::marker::PhantomData;
use std::{cell::Cell, fmt};

use super::NodeGenerics;
use crate::incrsan::NotObserver;
use crate::node::Input;
use crate::Incr;
use crate::Value;

pub(crate) struct MapRefNode<F, T, R>
where
    F: Fn(&T) -> &R + 'static + NotObserver,
{
    pub(crate) input: Input<T>,
    pub(crate) mapper: F,
    pub(crate) did_change: Cell<bool>,
}

impl<F, T, R> NodeGenerics for MapRefNode<F, T, R>
where
    T: Value,
    R: Value,
    F: Fn(&T) -> &R + 'static + NotObserver,
{
    type R = R;
    type BindLhs = ();
    type BindRhs = ();
    type I1 = T;
    type FRef = F;
    node_generics_default! { I2, I3, I4, I5, I6 }
    node_generics_default! { F1, F2, F3, F4, F5, F6 }
    node_generics_default! { B1, Fold, Update, WithOld, Recompute, ObsChange }
}

impl<F, T, R> fmt::Debug for MapRefNode<F, T, R>
where
    F: Fn(&T) -> &R + 'static + NotObserver,
    R: Value,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MapRefNode")
            .field("did_change", &self.did_change.get())
            .finish()
    }
}

macro_rules! map_node {
    (@rest) => {
        node_generics_default! { B1, BindLhs, BindRhs }
        node_generics_default! { Fold, Update, WithOld, FRef, Recompute, ObsChange }
    };
    (@FnMut $($param:ident,)*) => {
        FnMut($($param,)*) -> Self::R
    };
    (@head $t1:ident, $($t2:ident,)*) => {
        $t1
    };
    (@tail_args $tfield1:ident: $t1:ident, $($tfield2:ident: $t2:ident,)*) => {
        $($tfield2: &Incr<$t2>,)*
    };
    (@tail_mapper $mapnode:ident { $f:ident, $self:ident, $tfield1:ident, $($tfield2:ident,)* }) => {
        $mapnode {
            $tfield1: $self.clone().node,
            $($tfield2: $tfield2.clone().node,)*
            mapper: Box::new(RefCell::new($f)),
        }
    };
    ($vis:vis struct $mapnode:ident <
         inputs {
             $tfield1:ident: $t1:ident = $i1:ident,
             $(
                 $tfield:ident : $t:ident = $i:ident,
             )*
         }
         fn {
             $ffield:ident : $fparam:ident(.. $(, $t2:ident)*) -> $r:ident,
         }
     > {
         default < $($d:ident),* >,
         impl Incr::$methodname:ident, Kind::$kind:ident
     }) => {
        $vis struct $mapnode <$t1, $($t,)* $r>
        where
            $r: Value,
        {
            $vis $tfield1: Input<$t1>,
            $($vis $tfield: Input<$t>,)*
            $vis $ffield: Box<RefCell<dyn FnMut(&$t1, $(&$t2,)*) -> $r>>,
        }

        impl<$t1, $($t,)* $r> NodeGenerics for $mapnode<$t1, $($t2,)* $r>
        where
            $t1: Value,
            $($t: Value,)*
            $r: Value,
        {
            map_node!{ @rest }
            type R = $r;
            type $i1 = $t1;
            $(type $i = $t2;)*
            crate::node_generics_default! { $($d,)* $fparam }
        }
        impl<$t1, $($t,)* $r> fmt::Debug for $mapnode<$t1, $($t,)* $r>
        where
            $($t: Value,)*
            $r: Value,
        {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.debug_struct(stringify!($mapnode)).finish()
            }
        }
        impl<$t1: Value> Incr<$t1> {
            /// Like [Incr::map] and [Incr::map2], but with more input incrementals.
            ///
            /// If you don't feel like counting, try using the `(i1 % i2 % ...).map((|_, _, ...| ...))` syntax.
            pub fn $methodname<$fparam, $($t2,)* $r>(&self, $($tfield: &Incr<$t>,)* f: $fparam) -> Incr<R>
            where
                $($t: Value,)*
                $r: Value,
                $fparam : FnMut(&$t1, $(&$t2,)*) -> $r + 'static + NotObserver,
            {
                let mapper = map_node! {
                    @tail_mapper $mapnode {
                        f,
                        self,
                        $tfield1,
                        $($tfield,)*
                    }
                };
                let state = self.node.state();
                let node = crate::node::Node::<$mapnode<$t1, $($t2,)* $r>>::create_rc(
                    state.weak(),
                    state.current_scope.borrow().clone(),
                    crate::kind::Kind::$kind(mapper),
                );
                Incr { node }
            }
        }
    };
}

map_node! {
    pub(crate) struct MapNode<
        inputs {
            input: T1 = I1,
        }
        fn { mapper: F1(..) -> R, }
    > {
        default < F2, F3, F4, F5, F6, I2, I3, I4, I5, I6 >,
        impl Incr::map, Kind::Map
    }
}

// impl<T1, F> MapNode<T1, F> {
//     fn thing(&self) {
//         self.mapper
//     }
// }

map_node! {
    pub(crate) struct Map2Node<
        inputs {
            one: T1 = I1,
            two: T2 = I2,
        }
        fn { mapper: F2(.., T2) -> R, }
    > {
        default < F1, F3, F4, F5, F6, I3, I4, I5, I6 >,
        impl Incr::map2, Kind::Map2
    }
}

map_node! {
    pub(crate) struct Map3Node<
        inputs {
            one: T1 = I1,
            two: T2 = I2,
            three: T3 = I3,
        }
        fn { mapper: F3(.., T2, T3) -> R, }
    > {
        default < F1, F2, F4, F5, F6, I4, I5, I6 >,
        impl Incr::map3, Kind::Map3
    }
}

map_node! {
    pub(crate) struct Map4Node<
        inputs {
            one: T1 = I1,
            two: T2 = I2,
            three: T3 = I3,
            four: T4 = I4,
        }
        fn { mapper: F4(.., T2, T3, T4) -> R, }
    > {
        default < F1, F2, F3, F5, F6, I5, I6 >,
        impl Incr::map4, Kind::Map4
    }
}

map_node! {
    pub(crate) struct Map5Node<
        inputs {
            one: T1 = I1,
            two: T2 = I2,
            three: T3 = I3,
            four: T4 = I4,
            five: T5 = I5,
        }
        fn { mapper: F5(.., T2, T3, T4, T5) -> R, }
    > {
        default < F1, F2, F3, F4, F6, I6 >,
        impl Incr::map5, Kind::Map5
    }
}

map_node! {
    pub(crate) struct Map6Node<
        inputs {
            one: T1 = I1,
            two: T2 = I2,
            three: T3 = I3,
            four: T4 = I4,
            five: T5 = I5,
            six: T6 = I6,
        }
        fn { mapper: F6(.., T2, T3, T4, T5, T6) -> R, }
    > {
        default < F1, F2, F3, F4, F5 >,
        impl Incr::map6, Kind::Map6
    }
}

/// Lets you dismantle the old R for parts.
pub(crate) struct MapWithOld<F, T, R>
where
    F: FnMut(Option<R>, &T) -> (R, bool),
{
    pub input: Input<T>,
    pub mapper: RefCell<F>,
    pub _p: PhantomData<R>,
}

impl<F, T, R> MapWithOld<F, T, R>
where
    F: FnMut(Option<R>, &T) -> (R, bool) + NotObserver,
{
    pub fn new(input: Input<T>, mapper: F) -> Self {
        Self {
            input,
            mapper: RefCell::new(mapper),
            _p: PhantomData,
        }
    }
}

impl<F, T, R> NodeGenerics for MapWithOld<F, T, R>
where
    T: Value,
    R: Value,
    // WARN: we ignore this boolean now
    F: FnMut(Option<R>, &T) -> (R, bool) + 'static + NotObserver,
{
    type I1 = T;
    type R = R;
    type WithOld = F;
    node_generics_default! { I2, I3, I4, I5, I6 }
    node_generics_default! { F1, F2, F3, F4, F5, F6 }
    node_generics_default! { B1, BindLhs, BindRhs }
    node_generics_default! { Fold, Update, FRef, Recompute, ObsChange }
}

impl<F, T, R> fmt::Debug for MapWithOld<F, T, R>
where
    F: FnMut(Option<R>, &T) -> (R, bool) + NotObserver,
    R: Value,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MapWithOld").finish()
    }
}

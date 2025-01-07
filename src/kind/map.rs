use std::cell::RefCell;
use std::{cell::Cell, fmt};

use crate::boxes::SmallBox;
use crate::incrsan::NotObserver;
use crate::{Incr, NodeRef};
use crate::{Value, ValueInternal};

pub(crate) trait FRef:
    (for<'a> Fn(&'a dyn ValueInternal) -> &'a dyn ValueInternal) + 'static + NotObserver
{
}
impl<F> FRef for F where
    F: (for<'a> Fn(&'a dyn ValueInternal) -> &'a dyn ValueInternal) + 'static + NotObserver
{
}

pub(crate) struct MapRefNode {
    pub(crate) input: NodeRef,
    // Can't make this one Miny because of some weird issues with lifetimes?
    pub(crate) mapper: Box<dyn FRef>,
    pub(crate) did_change: Cell<bool>,
}

impl fmt::Debug for MapRefNode {
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
    (@tail_mapper $mapnode:ident { $f:expr, $self:ident, $tfield1:ident, $($tfield2:ident,)* }) => {
        $mapnode {
            $tfield1: $self.node.packed(),
            $($tfield2: $tfield2.node.packed(),)*
            mapper: RefCell::new($crate::boxes::new_unsized!($f)),
        }
    };
    (@any $type:ty) => {dyn $crate::ValueInternal};
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
         $(#[$method_meta:meta])*
         impl Incr::$methodname:ident, Kind::$kind:ident
     }) => {
        $vis struct $mapnode {
            $vis $tfield1: crate::NodeRef,
            $($vis $tfield: crate::NodeRef,)*
            $vis $ffield: RefCell<$fparam>,
        }

        $crate::incrsan::not_observer_boxed_trait! {
            pub(crate) type $fparam = crate::boxes::SmallBox<dyn (FnMut(&dyn $crate::ValueInternal, $(&map_node!(@any $t2),)*) -> $crate::boxes::SmallBox<dyn $crate::ValueInternal>)>;
        }

        impl fmt::Debug for $mapnode
        {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.debug_struct(stringify!($mapnode)).finish()
            }
        }
        impl<$t1: Value> Incr<$t1> {
            $(#[$method_meta])*
            pub fn $methodname<$fparam, $($t2,)* $r>(
                &self,
                $($tfield: &Incr<$t>,)*
                mut f: $fparam) -> Incr<R>
            where
                $($t: Value,)*
                $r: Value,
                $fparam : FnMut(&$t1, $(&$t2,)*) -> $r + 'static + NotObserver,
            {
                let mapper = map_node! {
                    @tail_mapper $mapnode {
                        move |
                            $tfield1: &dyn $crate::ValueInternal,
                            $(
                                $tfield: &dyn $crate::ValueInternal,
                            )*
                            | -> $crate::boxes::SmallBox<dyn $crate::ValueInternal>
                        {
                            let $tfield1 = $tfield1.as_any().downcast_ref::<$t1>().expect("Type error in map function");
                            $(
                                let $tfield = $tfield.as_any().downcast_ref::<$t>().expect("Type error in map function");
                            )*
                            $crate::boxes::new_unsized!(f( $tfield1, $($tfield,)* ))
                        },
                        self,
                        $tfield1,
                        $($tfield,)*
                    }
                };
                let state = self.node.state();
                let node = crate::node::Node::create_rc::<$r>(
                    state.weak(),
                    state.current_scope.borrow().clone(),
                    crate::kind::Kind::$kind(mapper),
                );
                Incr { node }
            }
        }
    };
}

macro_rules! default_doc {
    () => {
        r#"
Like [Incr::map] and [Incr::map2], but with more input incrementals.

If you don't feel like counting, try using the `(i1 % i2 % ...).map(|_, _, ...| ...)` syntax.
"#
    };
}

map_node! {
    pub(crate) struct MapNode<
        inputs {
            input: T1 = I1,
        }
        fn { mapper: F1(..) -> R, }
    > {
        default < F2, F3, F4, F5, F6, I1, I2, I3, I4, I5, I6 >,
        /// Takes an incremental (self), and produces a new incremental whose value
        /// is the result of applying a function `f` to the first value.
        ///
        /// ## Example
        ///
        /// ```
        /// # use incremental::*;
        /// let state = IncrState::new();
        /// let var = state.var(20);
        ///
        /// // produce a new incremental that adds ten
        /// let plus10 = var.map(|x| *x + 10);
        ///
        /// let observer = plus10.observe();
        /// state.stabilise();
        /// assert_eq!(observer.value(), 30);
        /// var.set(400);
        /// state.stabilise();
        /// assert_eq!(observer.value(), 410);
        /// ```
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
        default < F1, F3, F4, F5, F6, I1, I2, I3, I4, I5, I6 >,
        /// Like [Incr::map], but with two inputs.
        ///
        /// ```
        /// # use incremental::*;
        /// let state = IncrState::new();
        /// let v1 = state.var(1);
        /// let v2 = state.var(1);
        /// let add = v1.map2(&v2, |a, b| *a + *b);
        /// ```
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
        default < F1, F2, F4, F5, F6, I1, I2, I3, I4, I5, I6 >,
        #[doc = default_doc!()]
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
        default < F1, F2, F3, F5, F6, I1, I2, I3, I4, I5, I6 >,
        #[doc = default_doc!()]
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
        default < F1, F2, F3, F4, F6, I1, I2, I3, I4, I5, I6 >,
        #[doc = default_doc!()]
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
        default < F1, F2, F3, F4, F5, I1, I2, I3, I4, I5, I6 >,
        #[doc = default_doc!()]
        impl Incr::map6, Kind::Map6
    }
}

pub(crate) trait FWithOld:
    FnMut(
        Option<SmallBox<dyn ValueInternal>>,
        &dyn ValueInternal,
    ) -> (SmallBox<dyn ValueInternal>, bool)
    + 'static
    + NotObserver
{
}
impl<F> FWithOld for F where
    F: FnMut(
            Option<SmallBox<dyn ValueInternal>>,
            &dyn ValueInternal,
        ) -> (SmallBox<dyn ValueInternal>, bool)
        + 'static
        + NotObserver
{
}

/// Lets you dismantle the old R for parts.
pub(crate) struct MapWithOld {
    pub input: NodeRef,
    pub mapper: RefCell<SmallBox<dyn FWithOld>>,
}

impl fmt::Debug for MapWithOld {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("MapWithOld").finish()
    }
}

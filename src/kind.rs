use std::fmt::Debug;
use std::rc::Rc;

use super::node::Node;
use super::var::Var;
use crate::incrsan::NotObserver;
use crate::{Incr, Value};

#[macro_export]
macro_rules! node_generics_default {
    (@fn $name:ident ( $($param:ident),* )) => {
        type $name = fn($(&Self::$param,)*) -> Self::R;
    };
    (@single F1) => { /* snipped */ };
    (@single F2) => { /* snipped */ };
    (@single F3) => { /* snipped */ };
    (@single F4) => { /* snipped */ };
    (@single F5) => { /* snipped */ };
    (@single F6) => { /* snipped */ };

    (@single I1) => { /* snipped */ };
    (@single I2) => { /* snipped */ };
    (@single I3) => { /* snipped */ };
    (@single I4) => { /* snipped */ };
    (@single I5) => { /* snipped */ };
    (@single I6) => { /* snipped */ };

    (@single BindLhs) => { /* snipped */ };
    (@single BindRhs) => { type BindRhs = (); };
    (@single B1) => { /* snipped */ };
    (@single Fold) => { /* snipped */ };
    (@single Update) => { /* snipped */ };
    (@single WithOld) => { /* snipped */ };
    (@single FRef) => { /* snipped */ };
    (@single Recompute) => { /* snipped */ };
    (@single ObsChange) => { /* snipped */ };

    ($($ident:ident),*) => {
        $(
            node_generics_default! { @single $ident }
        )*
    }
}

mod array_fold;
mod bind;
pub(crate) mod expert;
mod map;

pub(crate) use array_fold::*;
pub(crate) use bind::*;
pub(crate) use expert::ExpertNode;
pub(crate) use map::*;

pub(crate) trait NodeGenerics: 'static + NotObserver {
    type R: Value;
    type BindRhs: Value;
}

pub(crate) enum Kind<G: NodeGenerics> {
    Constant(G::R),
    ArrayFold(Box<dyn array_fold::ArrayFoldTrait<G::R>>),
    // We have a strong reference to the Var, because (e.g.) the user's public::Var
    // may have been set and then dropped before the next stabilise().
    Var(Rc<Var<G::R>>),
    Map(map::MapNode<G::R>),
    MapWithOld(map::MapWithOld<G::R>),
    MapRef(map::MapRefNode<G::R>),
    Map2(map::Map2Node<G::R>),
    Map3(map::Map3Node<G::R>),
    Map4(map::Map4Node<G::R>),
    Map5(map::Map5Node<G::R>),
    Map6(map::Map6Node<G::R>),
    BindLhsChange {
        casts: bind::BindLhsId<G>,
        bind: Rc<bind::BindNode<G::BindRhs>>,
    },
    BindMain {
        bind: Rc<bind::BindNode<G::R>>,
        // Ownership goes
        // a Kind::BindMain holds a BindNode & the BindLhsChange
        // a Kind::BindLhsChange holds a BindNode
        // BindNode holds weak refs to both
        lhs_change: Rc<Node<bind::BindLhsChangeGen<G::R>>>,
    },
    Expert(expert::ExpertNode<G::R>),
}

impl<G: NodeGenerics> Debug for Kind<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Kind::Constant(v) => write!(f, "Constant({v:?})"),
            Kind::ArrayFold(af) => write!(f, "ArrayFold({af:?})"),
            Kind::Var(var) => write!(f, "Var({:?})", var),
            Kind::Map(map) => write!(f, "Map({:?})", map),
            Kind::MapWithOld(map) => write!(f, "MapWithOld({:?})", map),
            Kind::Map2(map) => write!(f, "Map2({:?})", map),
            Kind::Map3(map) => write!(f, "Map3({:?})", map),
            Kind::Map4(map) => write!(f, "Map4({:?})", map),
            Kind::Map5(map) => write!(f, "Map5({:?})", map),
            Kind::Map6(map) => write!(f, "Map6({:?})", map),
            Kind::BindLhsChange { bind, .. } => write!(f, "BindLhsChange({:?})", bind),
            Kind::BindMain { bind, .. } => write!(f, "BindMain({:?})", bind),
            Kind::MapRef(mapref) => write!(f, "MapRef({:?})", mapref),
            Kind::Expert(expert) => write!(f, "Expert({:?})", expert),
        }
    }
}

impl<G: NodeGenerics> Kind<G> {
    pub(crate) fn debug_ty(&self) -> impl Debug + '_ {
        return KindDebugTy(self);
        struct KindDebugTy<'a, G: NodeGenerics>(&'a Kind<G>);
        impl<'a, G: NodeGenerics> Debug for KindDebugTy<'a, G> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self.0 {
                    Kind::Constant(_) => {
                        write!(f, "Constant<{}>", std::any::type_name::<G::R>())
                    }
                    Kind::ArrayFold(af) => af.debug_ty(f),
                    Kind::Var(_) => write!(f, "Var<{}>", std::any::type_name::<G::R>()),
                    Kind::Map(..) => {
                        write!(f, "Map<(...) -> {}>", std::any::type_name::<G::R>())
                    }
                    Kind::MapWithOld(..) => {
                        write!(f, "MapWithOld<(...) -> {}>", std::any::type_name::<G::R>())
                    }
                    Kind::MapRef(..) => {
                        write!(f, "MapRef<(...) -> {}>", std::any::type_name::<G::R>())
                    }
                    Kind::Map2(..) => {
                        write!(f, "Map2<(...) -> {}>", std::any::type_name::<G::R>())
                    }
                    Kind::Map3(..) => {
                        write!(f, "Map3<(...) -> {}>", std::any::type_name::<G::R>())
                    }
                    Kind::Map4(..) => {
                        write!(f, "Map4<(...) -> {}>", std::any::type_name::<G::R>())
                    }
                    Kind::Map5(..) => {
                        write!(f, "Map5<(...) -> {}>", std::any::type_name::<G::R>())
                    }
                    Kind::Map6(..) => {
                        write!(f, "Map6<(...) -> {}>", std::any::type_name::<G::R>())
                    }
                    Kind::BindLhsChange { .. } => {
                        write!(f, "BindLhsChange",)
                    }
                    Kind::BindMain { .. } => {
                        write!(
                            f,
                            "BindMain<(lhs: dynamic) -> {}>",
                            std::any::type_name::<G::BindRhs>()
                        )
                    }
                    Kind::Expert(_) => write!(f, "Expert<{}>", std::any::type_name::<G::R>()),
                }
            }
        }
    }
}

impl<G: NodeGenerics> Kind<G> {
    pub(crate) const BIND_RHS_CHILD_INDEX: i32 = 1;
    pub(crate) fn initial_num_children(&self) -> usize {
        match self {
            Self::Constant(_) => 0,
            Self::ArrayFold(af) => af.children_len(),
            Self::Var(_) => 0,
            Self::Map(_) | Self::MapWithOld(_) | Self::MapRef(_) => 1,
            Self::Map2(_) => 2,
            Self::Map3(_) => 3,
            Self::Map4(_) => 4,
            Self::Map5(_) => 5,
            Self::Map6(_) => 6,
            Self::BindLhsChange { .. } => 1,
            Self::BindMain { .. } => 2,
            Self::Expert(_) => 0,
        }
    }
}

pub(crate) struct Constant<T>(std::marker::PhantomData<T>);

impl<T: Value> NodeGenerics for Constant<T> {
    type R = T;
    node_generics_default! { I1, I2, I3, I4, I5, I6 }
    node_generics_default! { F1, F2, F3, F4, F5, F6 }
    node_generics_default! { B1, BindLhs, BindRhs, Fold, Update, WithOld, FRef, Recompute, ObsChange }
}

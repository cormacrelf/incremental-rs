use std::fmt::Debug;
use std::rc::Rc;

use super::node::Node;
use super::var::Var;
use crate::{Incr, Value};

#[macro_export]
macro_rules! node_generics_default {
    (@fn $name:ident ( $($param:ident),* )) => {
        type $name = fn($(&Self::$param,)*) -> Self::R;
    };
    (F1) => { node_generics_default!{ @fn F1 (I1) } };
    (F2) => { node_generics_default!{ @fn F2 (I1, I2) } };
    (F3) => { node_generics_default!{ @fn F3 (I1, I2, I3) } };
    (F4) => { node_generics_default!{ @fn F4 (I1, I2, I3, I4) } };
    (F5) => { node_generics_default!{ @fn F5 (I1, I2, I3, I4, I5) } };

    (I1) => { type I1 = (); };
    (I2) => { type I2 = (); };
    (I3) => { type I3 = (); };
    (I4) => { type I4 = (); };
    (I5) => { type I5 = (); };

    (BindLhs) => { type BindLhs = (); };
    (BindRhs) => { type BindRhs = (); };
    (B1) => { type B1 = fn(&Self::BindLhs) -> Incr<Self::BindRhs>; };
    (Fold) => { type Fold = fn(Self::R, &Self::I1) -> Self::R; };
    (Update) => { type Update = fn(Self::R, &Self::I1, &Self::I1) -> Self::R; };
    (WithOld) => { type WithOld = fn(Option<Self::R>, &Self::I1) -> (Self::R, bool); };
    (FRef) => { type FRef = fn(&Self::I1) -> &Self::R; };
    (Recompute) => { type Recompute = fn() -> Self::R; };
    (ObsChange) => { type ObsChange = fn(bool); };

    ($($ident:ident),*) => {
        $(
            node_generics_default! { $ident }
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

pub(crate) trait NodeGenerics: 'static {
    type R: Value;
    type BindLhs: Value;
    type BindRhs: Value;
    type I1: Value;
    type I2: Value;
    type I3: Value;
    type I4: Value;
    type I5: Value;
    type F1: FnMut(&Self::I1) -> Self::R;
    type FRef: Fn(&Self::I1) -> &Self::R;
    type F2: FnMut(&Self::I1, &Self::I2) -> Self::R;
    type F3: FnMut(&Self::I1, &Self::I2, &Self::I3) -> Self::R;
    type F4: FnMut(&Self::I1, &Self::I2, &Self::I3, &Self::I4) -> Self::R;
    type F5: FnMut(&Self::I1, &Self::I2, &Self::I3, &Self::I4, &Self::I5) -> Self::R;
    type B1: FnMut(&Self::BindLhs) -> Incr<Self::BindRhs>;
    type Fold: FnMut(Self::R, &Self::I1) -> Self::R;
    type Update: FnMut(Self::R, &Self::I1, &Self::I1) -> Self::R;
    type WithOld: FnMut(Option<Self::R>, &Self::I1) -> (Self::R, bool);
    type Recompute: FnMut() -> Self::R;
    type ObsChange: FnMut(bool);
}

pub(crate) enum Kind<G: NodeGenerics> {
    Constant(G::R),
    ArrayFold(array_fold::ArrayFold<G::Fold, G::I1, G::R>),
    // We have a strong reference to the Var, because (e.g.) the user's public::Var
    // may have been set and then dropped before the next stabilise().
    Var(Rc<Var<G::R>>),
    Map(map::MapNode<G::F1, G::I1, G::R>),
    MapWithOld(map::MapWithOld<G::WithOld, G::I1, G::R>),
    MapRef(map::MapRefNode<G::FRef, G::I1, G::R>),
    Map2(map::Map2Node<G::F2, G::I1, G::I2, G::R>),
    Map3(map::Map3Node<G::F3, G::I1, G::I2, G::I3, G::R>),
    Map4(map::Map4Node<G::F4, G::I1, G::I2, G::I3, G::I4, G::R>),
    Map5(map::Map5Node<G::F5, G::I1, G::I2, G::I3, G::I4, G::I5, G::R>),
    BindLhsChange {
        casts: bind::BindLhsId<G>,
        bind: Rc<bind::BindNode<G::B1, G::BindLhs, G::BindRhs>>,
    },
    BindMain {
        casts: bind::BindMainId<G>,
        bind: Rc<bind::BindNode<G::B1, G::BindLhs, G::BindRhs>>,
        // Ownership goes
        // a Kind::BindMain holds a BindNode & the BindLhsChange
        // a Kind::BindLhsChange holds a BindNode
        // BindNode holds weak refs to both
        lhs_change: Rc<Node<bind::BindLhsChangeGen<G::B1, G::BindLhs, G::BindRhs>>>,
    },
    Expert(expert::ExpertNode<G::R, G::I1, G::Recompute, G::ObsChange>),
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
            Kind::BindLhsChange { bind, .. } => write!(f, "BindLhsChange({:?})", bind),
            Kind::BindMain { bind, .. } => write!(f, "BindMain({:?})", bind),
            Kind::MapRef(mapref) => write!(f, "MapRef({:?})", mapref),
            Kind::Expert(expert) => write!(f, "Expert({:?})", expert),
        }
    }
}

impl<G: NodeGenerics> Kind<G> {
    pub(crate) const BIND_RHS_CHILD_INDEX: i32 = 1;
    pub(crate) fn initial_num_children(&self) -> usize {
        match self {
            Self::Constant(_) => 0,
            Self::ArrayFold(af) => af.children.len(),
            Self::Var(_) => 0,
            Self::Map(_) | Self::MapWithOld(_) | Self::MapRef(_) => 1,
            Self::Map2(_) => 2,
            Self::Map3(_) => 3,
            Self::Map4(_) => 4,
            Self::Map5(_) => 5,
            Self::BindLhsChange { .. } => 1,
            Self::BindMain { .. } => 2,
            Self::Expert(_) => 0,
        }
    }
}

pub(crate) struct Constant<T>(std::marker::PhantomData<T>);

impl<T: Value> NodeGenerics for Constant<T> {
    type R = T;
    node_generics_default! { I1, I2, I3, I4, I5 }
    node_generics_default! { F1, F2, F3, F4, F5 }
    node_generics_default! { B1, BindLhs, BindRhs, Fold, Update, WithOld, FRef, Recompute, ObsChange }
}

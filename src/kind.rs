use std::fmt::Debug;
use std::rc::Rc;

use super::node::Node;
use super::var::Var;
use crate::{Incr, Value};

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
    type F1: FnMut(&Self::I1) -> Self::R;
    type FRef: Fn(&Self::I1) -> &Self::R;
    type F2: FnMut(&Self::I1, &Self::I2) -> Self::R;
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
            Kind::Map2(map2) => write!(f, "Map2({:?})", map2),
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
            Self::BindLhsChange { .. } => 1,
            Self::BindMain { .. } => 2,
            Self::Expert(_) => 0,
        }
    }
}

pub(crate) struct Constant<T>(std::marker::PhantomData<T>);

impl<T: Value> NodeGenerics for Constant<T> {
    type R = T;
    type BindLhs = ();
    type BindRhs = ();
    type I1 = ();
    type I2 = ();
    type F1 = fn(&Self::I1) -> Self::R;
    type F2 = fn(&Self::I1, &Self::I2) -> Self::R;
    type B1 = fn(&Self::BindLhs) -> Incr<Self::BindRhs>;
    type Fold = fn(Self::R, &Self::I1) -> Self::R;
    type Update = fn(Self::R, &Self::I1, &Self::I1) -> Self::R;
    type WithOld = fn(Option<Self::R>, &Self::I1) -> (Self::R, bool);
    type FRef = fn(&Self::I1) -> &Self::R;
    type Recompute = fn() -> Self::R;
    type ObsChange = fn(bool);
}

use std::fmt::Debug;
use std::rc::{Rc, Weak};

use crate::{Incr, Value};

use refl::Id;

use super::array_fold::ArrayFold;
use super::node::{Input, Node};
use super::unordered_fold::UnorderedArrayFold;
use super::var::Var;
use super::{BindNode, Map2Node, MapNode, MapWithOld, MapRefNode, BindLhsChangeNodeGenerics};

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
}

pub(crate) enum Kind<G: NodeGenerics> {
    Constant(G::R),
    ArrayFold(ArrayFold<G::Fold, G::I1, G::R>),
    UnorderedArrayFold(UnorderedArrayFold<G::Fold, G::Update, G::I1, G::R>),
    // We have a strong reference to the Var, because (e.g.) the user's public::Var
    // may have been set and then dropped before the next stabilise().
    Var(Rc<Var<G::R>>),
    Map(MapNode<G::F1, G::I1, G::R>),
    MapWithOld(MapWithOld<G::WithOld, G::I1, G::R>),
    MapRef(MapRefNode<G::FRef, G::I1, G::R>),
    Map2(Map2Node<G::F2, G::I1, G::I2, G::R>),
    BindLhsChange(
        BindLhsId<G>,
        // Ownership goes
        // a Kind::BindMain holds a BindNode & the BindLhsChange
        // a Kind::BindLhsChange holds a BindNode
        // BindNode holds weak refs to both
        Rc<BindNode<G::B1, G::BindLhs, G::BindRhs>>,
    ),
    BindMain(
        BindMainId<G>,
        Rc<BindNode<G::B1, G::BindLhs, G::BindRhs>>,
        Rc<Node<BindLhsChangeNodeGenerics<G::B1, G::BindLhs, G::BindRhs>>>,
    ),
}

pub(crate) struct BindLhsId<G: NodeGenerics> {
    pub(crate) r_unit: Id<(), G::R>,
    pub(crate) input_lhs_i2: Id<Input<G::BindLhs>, Input<G::I2>>,
}

impl<G: NodeGenerics> Debug for BindLhsId<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BindLhsId").finish()
    }
}

pub(crate) struct BindMainId<G: NodeGenerics> {
    pub(crate) rhs_r: Id<G::BindRhs, G::R>,
    pub(crate) input_rhs_i1: Id<Input<G::BindRhs>, Input<G::I1>>,
    pub(crate) input_lhs_i2: Id<Input<()>, Input<G::I2>>,
}

impl<G: NodeGenerics> Debug for BindMainId<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BindMainId")
            .field("d_eq_r", &self.rhs_r)
            .field("input_lhs_change_unit", &self.input_lhs_i2)
            .finish()
    }
}

impl<G: NodeGenerics> Debug for Kind<G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Kind::Constant(v) => write!(f, "Constant({v:?})"),
            Kind::ArrayFold(af) => write!(f, "ArrayFold({af:?})"),
            Kind::UnorderedArrayFold(uaf) => write!(f, "UnorderedArrayFold({uaf:?})"),
            Kind::Var(var) => write!(f, "Var({:?})", var),
            Kind::Map(map) => write!(f, "Map({:?})", map),
            Kind::MapWithOld(map) => write!(f, "MapWithOld({:?})", map),
            Kind::Map2(map2) => write!(f, "Map2({:?})", map2),
            Kind::BindLhsChange(_, bind) => write!(f, "BindLhsChange({:?})", bind),
            Kind::BindMain(_, bind, _) => write!(f, "BindMain({:?})", bind),
            Kind::MapRef(mapref) => write!(f, "MapRef({:?})", mapref),
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
            Self::BindLhsChange(..) => 1,
            Self::BindMain(..) => 2,
            Self::UnorderedArrayFold(uaf) => uaf.children.len(),
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
}

use std::rc::Rc;
use std::fmt::Debug;

use crate::{Incr, Value};

use refl::Id;
type Unit<T> = Id<(), T>;

use super::node::Input;
use super::unordered_fold::UnorderedArrayFold;
use super::{BindNode, MapNode, Map2Node};
use super::array_fold::ArrayFold;
use super::var::Var;

pub(crate) trait NodeGenerics<'a>: 'a {
    type R: Value<'a>;
    type BindLhs: Value<'a>;
    type BindRhs: Value<'a>;
    type I1: Value<'a>;
    type I2: Value<'a>;
    type F1: FnMut(Self::I1) -> Self::R + 'a;
    type F2: FnMut(Self::I1, Self::I2) -> Self::R + 'a;
    type B1: FnMut(Self::BindLhs) -> Incr<'a, Self::BindRhs> + 'a;
    type Fold: FnMut(Self::R, Self::I1) -> Self::R + 'a;
    type Update: FnMut(Self::R, Self::I1, Self::I1) -> Self::R + 'a;
}

pub(crate) enum Kind<'a, G: NodeGenerics<'a>> {
    Invalid,
    Uninitialised,
    ArrayFold(ArrayFold<'a, G::Fold, G::I1, G::R>),
    UnorderedArrayFold(UnorderedArrayFold<'a, G::Fold, G::Update, G::I1, G::R>),
    Var(Rc<Var<'a, G::R>>),
    Map(MapNode<'a, G::F1, G::I1, G::R>),
    Map2(Map2Node<'a, G::F2, G::I1, G::I2, G::R>),
    BindLhsChange(BindLhsId<'a,G>, Rc<BindNode<'a, G::B1, G::BindLhs, G::BindRhs>>),
    BindMain(BindMainId<'a, G>, Rc<BindNode<'a, G::B1, G::BindLhs, G::BindRhs>>),
    Cutoff(super::CutoffNode<'a, G::R>),
}

pub(crate) struct BindLhsId<'a, G: NodeGenerics<'a>> {
    pub(crate) r_unit: Id<(), G::R>,
    pub(crate) input_lhs_i2: Id<Input<'a, G::BindLhs>, Input<'a, G::I2>>,
}

impl<'a, G: NodeGenerics<'a>> Debug for BindLhsId<'a, G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BindLhsId")
            .finish()
    }
}

pub(crate) struct BindMainId<'a, G: NodeGenerics<'a>> {
    pub(crate) rhs_r: Id<G::BindRhs, G::R>,
    pub(crate) input_rhs_i1: Id<Input<'a, G::BindRhs>, Input<'a, G::I1>>,
    pub(crate) input_lhs_i2: Id<Input<'a, ()>, Input<'a, G::I2>>,
}

impl<'a, G: NodeGenerics<'a>> Debug for BindMainId<'a, G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BindMainId")
            .field("d_eq_r", &self.rhs_r)
            .field("input_lhs_change_unit", &self.input_lhs_i2)
            .finish()
    }
}

impl<'a, G: NodeGenerics<'a>> Debug for Kind<'a, G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Kind::Invalid => write!(f, "Invalid"),
            Kind::Uninitialised => write!(f, "Uninitialised"),
            Kind::ArrayFold(af) => write!(f, "ArrayFold({af:?})"),
            Kind::UnorderedArrayFold(uaf) => write!(f, "UnorderedArrayFold({uaf:?})"),
            Kind::Var(var) => write!(f, "Var({:?})", var),
            Kind::Map(map) => write!(f, "Map({:?})", map),
            Kind::Map2(map2) => write!(f, "Map2({:?})", map2),
            Kind::BindLhsChange(_, bind) => write!(f, "BindLhsChange({:?})", bind),
            Kind::BindMain(_, bind) => write!(f, "BindMain({:?})", bind),
            Kind::Cutoff(cutoff) => write!(f, "Cutoff({:?})", cutoff),
        }
    }
}

impl<'a, G: NodeGenerics<'a>> Kind<'a, G> {
    pub(crate) const BIND_RHS_CHILD_INDEX: i32 = 1;
    pub(crate) fn initial_num_children(&self) -> usize {
        match self {
            Self::Invalid => 0,
            Self::Uninitialised => 0,
            Self::ArrayFold(af) => af.children.len(),
            Self::Var(_) => 0,
            Self::Map(_) | Self::Cutoff(_) => 1,
            Self::Map2(_) => 2,
            Self::BindLhsChange(..) => 1,
            Self::BindMain(..) => 2,
            Self::UnorderedArrayFold(uaf) => uaf.children.len(),
        }
    }
}



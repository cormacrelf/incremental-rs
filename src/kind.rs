use std::fmt::{self, Debug};
use std::rc::Rc;

use crate::boxes::{new_unsized, SmallBox};
use crate::incrsan::NotObserver;
use crate::var::ErasedVariable;
use crate::{Incr, NodeRef, Value, ValueInternal};

pub(crate) trait KindTrait: 'static + NotObserver + Debug {
    fn compute(&self) -> SmallBox<dyn ValueInternal>;
    fn children_len(&self) -> usize;
    fn iter_children_packed(&self) -> Box<dyn Iterator<Item = NodeRef> + '_>;
    fn slow_get_child(&self, index: usize) -> NodeRef;
    fn debug_ty(&self, f: &mut fmt::Formatter) -> fmt::Result;
}

mod array_fold;
mod bind;
pub(crate) mod expert;
mod map;

pub(crate) use array_fold::*;
pub(crate) use bind::*;
pub(crate) use expert::ExpertNode;
pub(crate) use map::*;

pub(crate) enum Kind {
    Constant(SmallBox<dyn ValueInternal>),
    ArrayFold(SmallBox<dyn KindTrait>),
    // We have a strong reference to the Var, because (e.g.) the user's public::Var
    // may have been set and then dropped before the next stabilise().
    Var(Rc<dyn ErasedVariable>),
    Map(map::MapNode),
    MapWithOld(map::MapWithOld),
    MapRef(map::MapRefNode),
    Map2(map::Map2Node),
    Map3(map::Map3Node),
    Map4(map::Map4Node),
    Map5(map::Map5Node),
    Map6(map::Map6Node),
    BindLhsChange {
        bind: Rc<bind::BindNode>,
    },
    BindMain {
        bind: Rc<bind::BindNode>,
        // Ownership goes
        // a Kind::BindMain holds a BindNode & the BindLhsChange
        // a Kind::BindLhsChange holds a BindNode
        // BindNode holds weak refs to both
        lhs_change: NodeRef,
    },
    Expert(expert::ExpertNode),
}

impl Debug for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Kind::Constant(_) => write!(f, "Constant"),
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

impl Kind {
    pub(crate) fn debug_ty(&self) -> impl Debug + '_ {
        return KindDebugTy(self);
        struct KindDebugTy<'a>(&'a Kind);
        impl<'a> Debug for KindDebugTy<'a> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self.0 {
                    Kind::Constant(_) => write!(f, "Constant"),
                    Kind::ArrayFold(af) => af.debug_ty(f),
                    Kind::Var(var) => var.debug_ty(f),
                    Kind::Map(..) => {
                        write!(f, "Map<(...) -> ...>")
                    }
                    Kind::MapWithOld(..) => {
                        write!(f, "MapWithOld<(...) -> ...>")
                    }
                    Kind::MapRef(..) => {
                        write!(f, "MapRef<(...) -> ...>")
                    }
                    Kind::Map2(..) => {
                        write!(f, "Map2<(...) -> ...>")
                    }
                    Kind::Map3(..) => {
                        write!(f, "Map3<(...) -> ...>")
                    }
                    Kind::Map4(..) => {
                        write!(f, "Map4<(...) -> ...>")
                    }
                    Kind::Map5(..) => {
                        write!(f, "Map5<(...) -> ...>")
                    }
                    Kind::Map6(..) => {
                        write!(f, "Map6<(...) -> ...>")
                    }
                    Kind::BindLhsChange { .. } => {
                        write!(f, "BindLhsChange",)
                    }
                    Kind::BindMain { .. } => {
                        write!(f, "BindMain<(lhs: dynamic) -> (dynamic)>",)
                    }
                    Kind::Expert(_) => write!(f, "Expert"),
                }
            }
        }
    }

    pub(crate) fn constant<T: Value>(value: T) -> Kind {
        Kind::Constant(new_unsized!(value))
    }
}

impl Kind {
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

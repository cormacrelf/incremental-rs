use core::fmt;
use std::rc::Weak;

use super::{node::NodeId, NodeRef, WeakNode};
use crate::incrsan::NotObserver;
use crate::node::ErasedNode;

pub(crate) trait BindScope: fmt::Debug + NotObserver {
    fn id(&self) -> NodeId;
    fn is_valid(&self) -> bool;
    fn is_necessary(&self) -> bool;
    fn height(&self) -> i32;
    fn add_node(&self, node: WeakNode);
}

#[derive(Clone)]
pub(crate) enum Scope {
    Top,
    Bind(Weak<dyn BindScope>),
}

impl fmt::Debug for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scope::Top => write!(f, "Scope::Top"),
            Scope::Bind(bind) => {
                let Some(bind) = bind.upgrade() else {
                    return write!(f, "Scope::Bind(weak, deallocated)");
                };
                write!(f, "Scope::Bind({:?})", bind.id())
            }
        }
    }
}

impl Scope {
    fn equals(&self, other: &Self) -> bool {
        match self {
            Scope::Top => matches!(other, Scope::Top),
            Scope::Bind(w1) => match other {
                Scope::Bind(w2) => crate::weak_thin_ptr_eq(w1, w2),
                _ => false,
            },
        }
    }
    pub(crate) fn height(&self) -> i32 {
        match self {
            Self::Top => 0,
            Self::Bind(weak) => {
                let strong = weak.upgrade().unwrap();
                strong.height()
            }
        }
    }
    pub(crate) fn is_valid(&self) -> bool {
        match self {
            Self::Top => true,
            Self::Bind(weak) => {
                let strong = weak.upgrade().unwrap();
                strong.is_valid()
            }
        }
    }
    pub(crate) fn is_necessary(&self) -> bool {
        match self {
            Self::Top => true,
            Self::Bind(weak) => {
                let strong = weak.upgrade().unwrap();
                strong.is_necessary()
            }
        }
    }
    pub(crate) fn add_node(&self, node: NodeRef) {
        assert!(node.created_in().equals(self));
        match self {
            Self::Top => {}
            Self::Bind(bind_weak) => {
                let bind = bind_weak.upgrade().unwrap();
                bind.add_node(node.weak());
            }
        }
    }
}

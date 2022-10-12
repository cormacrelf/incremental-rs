use std::rc::Weak;

use super::{adjust_heights_heap::NodeRef, BindScope};

#[derive(Debug, Clone)]
pub(crate) enum Scope<'a> {
    Top,
    Bind(Weak<dyn BindScope<'a>>),
}

impl<'a> Scope<'a> {
    fn equals(&self, other: &Self) -> bool {
        match self {
            Scope::Top => match other {
                Scope::Top => true,
                _ => false,
            },
            Scope::Bind(w1) => match other {
                Scope::Bind(w2) => Weak::ptr_eq(w1, w2),
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
    pub(crate) fn is_necessary(&self) -> bool {
        match self {
            Self::Top => true,
            Self::Bind(weak) => {
                let strong = weak.upgrade().unwrap();
                strong.is_necessary()
            }
        }
    }
    pub(crate) fn add_node(&self, node: NodeRef<'a>) {
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
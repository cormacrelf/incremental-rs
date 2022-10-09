use std::rc::Weak;

use super::BindScope;

#[derive(Debug, Clone)]
pub(crate) enum Scope {
    Top,
    Bind(Weak<dyn BindScope>),
}

impl Scope {
    pub(crate) fn height(&self) -> i32 {
        match self {
            Self::Top => 0,
            Self::Bind(weak) => {
                let Some(strong) = weak.upgrade() else { panic!() };
                strong.height()
            }
        }
    }
    pub(crate) fn is_necessary(&self) -> bool {
        match self {
            Self::Top => true,
            Self::Bind(weak) => {
                let Some(strong) = weak.upgrade() else { panic!() };
                strong.is_necessary()
            }
        }
    }
}

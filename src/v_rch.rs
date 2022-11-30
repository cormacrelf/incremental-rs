mod adjust_heights_heap;
mod cutoff;
mod expert;

mod incr;
mod incr_map;
mod internal_observer;
mod kind;
mod node;
mod node_update;
mod recompute_heap;
mod scope;
mod stabilisation_num;
mod state;
mod syntax;
mod var;

use fmt::Debug;
use std::cell::Cell;
use std::fmt;
use std::rc::{Rc, Weak};

use self::node::ErasedNode;
use crate::Incr;

pub mod public;

/// Trait alias for `Debug + Clone + 'static`
pub trait Value: Debug + Clone + PartialEq + 'static {}
impl<T> Value for T where T: Debug + Clone + PartialEq + 'static {}
pub(crate) type NodeRef = Rc<dyn ErasedNode>;
pub(crate) type WeakNode = Weak<dyn ErasedNode>;

/// Little helper trait for bumping a statistic.
pub(crate) trait CellIncrement {
    type Num;
    fn increment(&self);
    fn decrement(&self);
    // std is going to add Cell:update... someday...
    fn update_val(&self, f: impl FnOnce(Self::Num) -> Self::Num);
}

macro_rules! impl_cell_increment {
    ($num_ty:ty) => {
        impl CellIncrement for Cell<$num_ty> {
            type Num = $num_ty;
            #[inline]
            fn update_val(&self, f: impl FnOnce(Self::Num) -> Self::Num) {
                self.set(f(self.get()));
            }
            #[inline(always)]
            fn increment(&self) {
                self.update_val(|x| x + 1)
            }
            #[inline(always)]
            fn decrement(&self) {
                self.update_val(|x| x - 1)
            }
        }
    };
}
impl_cell_increment!(i32);
impl_cell_increment!(usize);

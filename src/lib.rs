// #![feature(type_alias_impl_trait)]
#![doc = include_str!("../README.md")]
// We have some really complicated types. Most of them can't be typedef'd to be any shorter.
#![allow(clippy::type_complexity)]
// #![allow(clippy::single_match)]
#![cfg_attr(feature = "nightly-incrsan", feature(negative_impls))]
#![cfg_attr(feature = "nightly-incrsan", feature(auto_traits))]

mod adjust_heights_heap;
mod cutoff;
mod incr;
pub mod incrsan;
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

mod public;
pub use public::*;

use fmt::Debug;
use std::any::Any;
use std::cell::Cell;
use std::fmt;
use std::rc::{Rc, Weak};

use self::incrsan::NotObserver;
use self::node::ErasedNode;

/// Trait alias for `Debug + Clone + 'static`
pub trait Value: Debug + Clone + PartialEq + NotObserver + 'static {
    fn as_any(&self) -> &dyn Any;
}
impl<T> Value for T
where
    T: Debug + Clone + PartialEq + NotObserver + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }
}
pub(crate) type NodeRef = Rc<dyn ErasedNode>;
pub(crate) type WeakNode = Weak<dyn ErasedNode>;

pub trait Invariant {
    fn invariant(&self);
}

/// Solves the problem of `Rc::<dyn Trait>::ptr_eq` producing bad results, since
/// it compares fat pointers and their vtables which may differ between crates
/// for the same underlying type, or be the same for two different underlying types
/// when rustc uses the same vtable for each.
///
/// Probably don't use this for traits implemented by ZSTs... but there is no
/// good way to do pointer equality in that case anyway, without any allocations to
/// compare.
pub(crate) fn rc_thin_ptr_eq<T: ?Sized>(one: &Rc<T>, two: &Rc<T>) -> bool {
    let one_: *const () = Rc::as_ptr(one).cast();
    let two_: *const () = Rc::as_ptr(two).cast();
    one_ == two_
}
pub(crate) fn rc_thin_ptr_eq_t2<T: ?Sized, T2: ?Sized>(one: &Rc<T>, two: &Rc<T2>) -> bool {
    let one_: *const () = Rc::as_ptr(one).cast();
    let two_: *const () = Rc::as_ptr(two).cast();
    one_ == two_
}
pub(crate) fn weak_thin_ptr_eq<T: ?Sized>(one: &Weak<T>, two: &Weak<T>) -> bool {
    let one_: *const () = Weak::as_ptr(one).cast();
    let two_: *const () = Weak::as_ptr(two).cast();
    one_ == two_
}
pub(crate) fn dyn_thin_ptr_eq<T: ?Sized>(one: &T, two: &T) -> bool {
    let one_: *const () = one as *const T as *const ();
    let two_: *const () = two as *const T as *const ();
    one_ == two_
}

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

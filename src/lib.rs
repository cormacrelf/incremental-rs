// #![feature(type_alias_impl_trait)]

// We have some really complicated types. Most of them can't be typedef'd to be any shorter.
#![allow(clippy::type_complexity)]
// #![allow(clippy::single_match)]

mod order;
mod v1;
mod v_rch;

use std::rc::{Rc, Weak};

pub use v_rch::public::*;

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
pub(crate) fn weak_thin_ptr_eq<T: ?Sized>(one: &Weak<T>, two: &Weak<T>) -> bool {
    let one_: *const () = Weak::as_ptr(one).cast();
    let two_: *const () = Weak::as_ptr(two).cast();
    one_ == two_
}

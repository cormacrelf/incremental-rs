mod order;
mod v1;
mod v_rch;

pub use v_rch::public::*;

pub trait Invariant {
    fn invariant(&self);
}

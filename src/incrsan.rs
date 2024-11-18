//! Sanitisers for using incremental correctly.
//!
//! These are enabled by the `nightly-incrsan` feature flag, which requires a nightly compiler.
//! They primarily work through auto-traits, which can be a bit of a pain since incremental code
//! often interfaces with other code exposing `Box<dyn Fn()>`-like APIs. So it is not enabled by
//! default.
//!
//! You can opt-in by:
//!
//! - enabling the `nightly-incrsan` feature;
//! - peppering around [`+ NotObserver`][NotObserver] bounds on things like `impl FnMut() -> ...`;
//! - wrapping foreign types in [AssertNotObserver] when you are sure they do not contain observers

// Rustc will parse things inside the cfg attribute even if the feature is not enabled.
// But #[path = "..."] will help the compiler only parse one version of this code.
//
#[cfg_attr(feature = "nightly-incrsan", path = "incrsan/nightly.rs")]
#[cfg_attr(not(feature = "nightly-incrsan"), path = "incrsan/stable.rs")]
mod implementation;

pub use implementation::*;

/// A wrapper struct to assert that its contents are not observers, in the vein of
/// [`std::panic::AssertUnwindSafe`].
///
/// Only does anything with the `nightly-incrsan` feature enabled.
///
/// This is good if you have a `Box<dyn Trait>` you want to use somewhere, where the trait is some
/// foreign trait and you can't prove it to the compiler, but it doesn't have any observers in it.
///
/// For example:
///
/// ```
/// use incremental::*;
/// use incremental::incrsan::*;
///
/// let state = IncrState::new();
/// let constant = state.constant(1);
/// let not_observer: Box<dyn Fn()> = Box::new(|| println!("hello"));
///
/// // wrap in this to assert to the compiler it's ok, since you know what you put in the box
/// let not_observer = AssertNotObserver(not_observer);
///
/// // now you can use it freely inside map nodes, observer.subscribe() callbacks, etc
/// let map = constant.map(move |_| {
///     not_observer(); // No longer a compiler error
///     1234
/// });
/// ```
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssertNotObserver<T>(pub T);

impl<T> std::ops::Deref for AssertNotObserver<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> std::ops::DerefMut for AssertNotObserver<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Let the compiler check for you that T does not contain an observer.
///
/// Useful if you're about to wrap in an AssertNotObserver, and then
/// wrap in some kind of container that has no observers, that you know
/// of, but you still want to check that the type you're putting in the
/// container is `NotObserver`.
pub fn check_not_observer<T: NotObserver>(value: T) -> T {
    value
}

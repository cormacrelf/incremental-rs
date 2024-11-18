///
/// Nightly-only auto trait to prevent having an incremental node own an Observer, which is
/// invalid use of the API and will panic when dropped.
///
/// ```compile_fail
/// use incremental::*;
///
/// let state = IncrState::new();
/// let constant = state.constant(1);
/// let observer = constant.observe();
/// let map = constant.map(move |_| {
///     let _ = observer.value(); // invalid! will panic, so don't try!
///     1234
/// });
/// ```
///
#[diagnostic::on_unimplemented(
    message = "Type contains an incremental::Observer",
    // {Self} expands to the thing that doesn't implement NotObserver, which is e.g.
    // `Observer<i32>`.
    label = "Contains {Self}, which is either an Observer or cannot be proven not to contain one.",
    note = "Observers are the roots for garbage-collecting incremental nodes.\nThey hold incrementals, not the other way round.\nIf you need access to another incremental value in a map node, try map2.\nIf you know this type has no observers, you can use incremental::incrsan::AssertNotObserver.\n"
)]
pub auto trait NotObserver {}

impl<T> !NotObserver for crate::public::Observer<T> {}

impl<T> NotObserver for super::AssertNotObserver<T> {}

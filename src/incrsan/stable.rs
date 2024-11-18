/// Inactive auto trait, as `nightly-incrsan` feature is not enabled.
///
/// Designed to prevent having an incremental node own an Observer, which is
/// invalid use of the API and will panic when dropped.
///
/// This type is implemented for every T, which means it does not restrict anything.
/// You should only have to deal with this trait if you enable the `nightly-incrsan` feature.
pub trait NotObserver {}
impl<T: ?Sized> NotObserver for T {}

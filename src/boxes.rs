#[cfg(not(feature = "nightly-miny"))]
#[macro_use]
mod impls {
    macro_rules! new_unsized {
        ($expr:expr $(,)?) => {
            ::std::boxed::Box::new($expr)
        };
    }
    /// Type alias for [Box], because feature nightly-miny is disabled.
    pub(crate) type SmallBox<T> = Box<T>;
    pub(crate) use new_unsized;

    use crate::ValueInternal;
    pub fn downcast_inner<V: 'static>(boxed: SmallBox<dyn ValueInternal>) -> Option<V> {
        let boxed = downcast_box(boxed)?;
        Some(*boxed)
    }
    fn downcast_box<V: 'static>(boxed: SmallBox<dyn ValueInternal>) -> Option<SmallBox<V>> {
        if !boxed.as_any().is::<V>() {
            return None;
        }
        // Safety: we checked that boxed holds V
        Some(unsafe { downcast_unchecked(boxed) })
    }
    unsafe fn downcast_unchecked<V: 'static>(boxed: SmallBox<dyn ValueInternal>) -> SmallBox<V> {
        debug_assert!(boxed.as_any().is::<V>());
        // Safety: passed to caller
        unsafe {
            let raw: *mut (dyn ValueInternal) = Box::into_raw(boxed);
            Box::from_raw(raw as *mut V)
        }
    }
}

#[cfg(feature = "nightly-miny")]
#[macro_use]
mod impls {
    use crate::ValueInternal;

    macro_rules! new_unsized {
        ($expr:expr $(,)?) => {
            ::miny::Miny::new_unsized($expr)
        };
    }
    /// Type alias for [miny::Miny], because feature nightly-miny is enabled.
    pub type SmallBox<T> = miny::Miny<T>;
    pub(crate) use new_unsized;

    pub fn downcast_inner<V: 'static>(boxed: SmallBox<dyn ValueInternal>) -> Option<V> {
        // I'm not convinced that Miny::downcast is not too general -- it may allow
        miny::Miny::downcast(boxed).ok()
    }
}

pub(crate) use impls::*;

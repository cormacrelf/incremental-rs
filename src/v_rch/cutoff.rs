use std::{
    marker::PhantomData,
    rc::{Rc, Weak},
};

pub(crate) enum CutoffType<T> {
    Always,
    Never,
    PhysEqual,
    PartialEq,
    _T(PhantomData<T>),
}

// pub struct Cutoff<T>(CutoffType<T>);

pub trait ShouldCutoff {
    fn should_cutoff(&self, b: &Self) -> bool;
}

// struct PartialEqKind;
// struct PhysEqualKind;
// struct CustomKind;

impl<T> ShouldCutoff for T {
    default fn should_cutoff(&self, b: &T) -> bool {
        false
    }
}

impl<T: PhysEqual> ShouldCutoff for T {
    fn should_cutoff(&self, b: &T) -> bool {
        self.phys_equal(b)
    }
}

// pub struct NeverCutoff;
// impl<T> ShouldCutoff<T> for NeverCutoff {
//     fn should_cutoff(&self, _: &T) -> bool {
//         false
//     }
// }

// pub struct AlwaysCutoff;
// impl<T> ShouldCutoff<T> for AlwaysCutoff {
//     fn should_cutoff(&self, _: &T) -> bool {
//         true
//     }
// }

// pub struct PartialEqCutoff<T: PartialEq>(PhantomData<T>);
// impl<T> ShouldCutoff<T> for PartialEqCutoff<T>
// where
//     T: PartialEq,
// {
//     fn should_cutoff(&self, b: &T) -> bool {
//         PartialEq::eq(a, b)
//     }
// }

#[rustc_specialization_trait]
pub trait PhysEqual {
    fn phys_equal(&self, other: &Self) -> bool;
}

// pub struct PhysEqualCutoff;
// impl<T> ShouldCutoff<T> for PhysEqualCutoff
// where
//     T: PhysEqual,
// {
//     fn should_cutoff(&self,  b: &T) -> bool {
//         a.phys_equal(b)
//     }
// }

impl<T> PhysEqual for Rc<T> {
    fn phys_equal(&self, other: &Self) -> bool {
        Rc::ptr_eq(self, other)
    }
}
impl<T> PhysEqual for Weak<T> {
    fn phys_equal(&self, other: &Self) -> bool {
        Weak::ptr_eq(self, other)
    }
}
impl<T> PhysEqual for &T {
    fn phys_equal(&self, other: &Self) -> bool {
        std::ptr::eq(*self, *other)
    }
}
impl<T> PhysEqual for *const T {
    fn phys_equal(&self, other: &Self) -> bool {
        std::ptr::eq(*self, *other)
    }
}
// impl<T> Cutoff<T> {
//     fn always() -> Self {
//         Self(CutoffType::Always)
//     }
//     fn never() -> Self {
//         Self(CutoffType::Never)
//     }
//     fn phys_equal() -> Self
//     where
//         T: PhysEqual,
//     {
//         Self(CutoffType::PhysEqual)
//     }
//     fn partial_eq<T>() -> Self
//     where
//         T: PartialEq,
//     {
//         Self(CutoffType::PartialEq(PartialEqCutoff::<T>(PhantomData)))
//     }
// }


use std::rc::{Rc, Weak};

#[non_exhaustive]
pub enum Cutoff<T> {
    Always,
    Never,
    PartialEq,
    Custom(fn(&T, &T) -> bool),
}

impl<T> Copy for Cutoff<T> {}
impl<T> Clone for Cutoff<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Always => Self::Always,
            Self::Never => Self::Never,
            Self::PartialEq => Self::PartialEq,
            Self::Custom(f) => Self::Custom(*f),
        }
    }
}

impl<T> Cutoff<T>
where
    T: PartialEq,
{
    pub fn should_cutoff(&self, a: &T, b: &T) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::PartialEq => a.eq(b),
            Self::Custom(comparator) => comparator(a, b),
        }
    }
}

// struct PartialEqKind;
// struct PhysEqualKind;
// struct CustomKind;

// impl<T> ShouldCutoff for CutoffType<T> {
//     fn should_cutoff(&self, b: &T) -> bool {
//         false
//     }
// }

// impl<T: PhysEqual> ShouldCutoff for T {
//     fn should_cutoff(&self, b: &T) -> bool {
//         self.phys_equal(b)
//     }
// }

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

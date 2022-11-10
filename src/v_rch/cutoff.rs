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

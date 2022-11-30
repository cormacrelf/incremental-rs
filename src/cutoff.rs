use std::marker::PhantomData;

#[non_exhaustive]
#[derive(Clone)]
pub enum Cutoff<T, F = fn(&T, &T) -> bool> {
    Always,
    Never,
    PartialEq,
    Custom(F),
    #[doc(hidden)]
    __Phantom(PhantomData<T>),
}

impl<T, F> Copy for Cutoff<T, F>
where
    F: Copy,
    T: Clone,
{
}

impl<T, F> Cutoff<T, F>
where
    T: PartialEq,
    F: FnMut(&T, &T) -> bool,
{
    pub fn boxed(self) -> Cutoff<T, Box<dyn FnMut(&T, &T) -> bool>>
    where
        F: 'static,
    {
        match self {
            Self::Always => Cutoff::Always,
            Self::Never => Cutoff::Never,
            Self::PartialEq => Cutoff::PartialEq,
            Self::Custom(comparator) => Cutoff::Custom(Box::new(comparator)),
            Self::__Phantom(_) => Cutoff::__Phantom(PhantomData),
        }
    }
    pub fn should_cutoff(&mut self, a: &T, b: &T) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::PartialEq => a.eq(b),
            Self::Custom(comparator) => comparator(a, b),
            Self::__Phantom(_) => unreachable!(),
        }
    }
}

use std::any::Any;

use crate::incrsan::NotObserver;

pub(crate) struct ErasedCutoff {
    should_cutoff: Box<dyn CutoffClosure<dyn Any>>,
}

impl ErasedCutoff {
    pub(crate) fn new<T: PartialEq + Clone + 'static>(mut cutoff: Cutoff<T>) -> Self {
        Self {
            // codegen: one copy of this closure is generated for each T
            should_cutoff: Box::new(move |a: &dyn Any, b: &dyn Any| -> bool {
                let Some(a) = a.downcast_ref::<T>() else {
                    return false;
                };
                let Some(b) = b.downcast_ref::<T>() else {
                    return false;
                };
                cutoff.should_cutoff(a, b)
            }),
        }
    }
    pub(crate) fn should_cutoff(&mut self, a: &dyn Any, b: &dyn Any) -> bool {
        (&mut *self.should_cutoff)(a, b)
    }
}

#[derive(Clone)]
#[non_exhaustive]
pub enum Cutoff<T: ?Sized> {
    Always,
    Never,
    PartialEq,
    Fn(fn(&T, &T) -> bool),
    FnBoxed(Box<dyn CutoffClosure<T>>),
}

pub trait CutoffClosure<T: ?Sized>: FnMut(&T, &T) -> bool + NotObserver {
    fn clone_box(&self) -> Box<dyn CutoffClosure<T>>;
}

impl<T: ?Sized, F> CutoffClosure<T> for F
where
    F: FnMut(&T, &T) -> bool + Clone + 'static + NotObserver,
{
    fn clone_box(&self) -> Box<dyn CutoffClosure<T>> {
        Box::new(self.clone())
    }
}

impl<T> Clone for Box<dyn CutoffClosure<T>> {
    fn clone(&self) -> Self {
        (**self).clone_box()
    }
}

impl<T: ?Sized> Cutoff<T>
where
    T: PartialEq,
{
    pub fn should_cutoff(&mut self, a: &T, b: &T) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::PartialEq => a.eq(b),
            Self::Fn(comparator) => comparator(a, b),
            Self::FnBoxed(comparator) => comparator(a, b),
        }
    }
}

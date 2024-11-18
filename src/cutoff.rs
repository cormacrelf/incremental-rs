use crate::incrsan::NotObserver;

#[derive(Clone)]
#[non_exhaustive]
pub enum Cutoff<T> {
    Always,
    Never,
    PartialEq,
    Fn(fn(&T, &T) -> bool),
    FnBoxed(Box<dyn CutoffClosure<T>>),
}

pub trait CutoffClosure<T>: FnMut(&T, &T) -> bool + NotObserver {
    fn clone_box(&self) -> Box<dyn CutoffClosure<T>>;
}

impl<T, F> CutoffClosure<T> for F
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

impl<T> Cutoff<T>
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

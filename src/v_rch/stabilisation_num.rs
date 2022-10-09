use crate::order::PartialOrder;

#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct StabilisationNum(pub i32);

impl StabilisationNum {
    pub fn init() -> Self {
        Self(-1_i32)
    }
    pub fn add1(self) -> Self {
        Self(self.0 + 1)
    }
    pub fn is_none(&self) -> bool {
        self.0 == -1
    }
}

impl PartialOrder for StabilisationNum {
    fn less_equal(&self, other: &Self) -> bool {
        self.0 <= other.0
    }
}

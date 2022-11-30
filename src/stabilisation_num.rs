#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct StabilisationNum(pub i32);

impl StabilisationNum {
    pub fn init() -> Self {
        Self(-1)
    }
    pub fn add1(self) -> Self {
        Self(self.0 + 1)
    }
    pub fn is_never(&self) -> bool {
        self.0 == -1
    }
}

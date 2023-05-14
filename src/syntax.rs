//! This module has some syntax helpers.
use std::ops::Rem;

use super::Incr;
use super::Value;

/// Produced by the syntax `i1 % i2` for two `Incr`s.
pub struct MapBuilder2<I1, I2>(Incr<I1>, Incr<I2>);
/// Produced by the syntax `i1 % i2 % i3` for three `Incr`s.
pub struct MapBuilder3<I1, I2, I3>(Incr<I1>, Incr<I2>, Incr<I3>);
impl<I1, I2> Rem<Incr<I2>> for Incr<I1> {
    type Output = MapBuilder2<I1, I2>;
    fn rem(self, rhs: Incr<I2>) -> Self::Output {
        MapBuilder2(self, rhs)
    }
}
impl<I1, I2> Rem<&Incr<I2>> for &Incr<I1> {
    type Output = MapBuilder2<I1, I2>;
    fn rem(self, rhs: &Incr<I2>) -> Self::Output {
        MapBuilder2(self.clone(), rhs.clone())
    }
}

impl<I1, I2, I3> Rem<Incr<I3>> for MapBuilder2<I1, I2> {
    type Output = MapBuilder3<I1, I2, I3>;
    fn rem(self, rhs: Incr<I3>) -> Self::Output {
        MapBuilder3(self.0, self.1, rhs)
    }
}
impl<I1, I2, I3> Rem<&Incr<I3>> for MapBuilder2<I1, I2> {
    type Output = MapBuilder3<I1, I2, I3>;
    fn rem(self, rhs: &Incr<I3>) -> Self::Output {
        MapBuilder3(self.0, self.1, rhs.clone())
    }
}

impl<I1: Value, I2: Value> MapBuilder2<I1, I2> {
    pub fn map<R: Value>(&self, f: impl FnMut(&I1, &I2) -> R + 'static) -> Incr<R> {
        let Self(i1, i2) = self;
        i1.map2(i2, f)
    }
    pub fn zip(&self) -> Incr<(I1, I2)> {
        let Self(i1, i2) = self;
        i1.zip(i2)
    }
}

impl<I1: Value, I2: Value, I3: Value> MapBuilder3<I1, I2, I3> {
    pub fn map<R: Value>(&self, f: impl FnMut(&I1, &I2, &I3) -> R + 'static) -> Incr<R> {
        let Self(i1, i2, i3) = self;
        i1.map3(i2, i3, f)
    }
    pub fn zip(&self) -> Incr<(I1, I2, I3)> {
        let Self(i1, i2, i3) = self;
        i1.map3(i2, i3, |a, b, c| (a.clone(), b.clone(), c.clone()))
    }
}

// TODO: implement MapBuilder4 and beyond

#[test]
fn test_syntax() {
    let incr = crate::IncrState::new();
    let i1 = incr.constant(5);
    let i2 = incr.constant(10);
    let i3 = incr.constant(9);
    let out = (i1 % i2 % i3).map(|&a, &b, &c| a * b * c);
    let obs = out.observe();
    incr.stabilise();
    assert_eq!(obs.try_get_value(), Ok(450));
}

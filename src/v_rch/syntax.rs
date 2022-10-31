//! This module has some syntax helpers.
use std::ops::Rem;

use super::Incr;
use super::Value;

/// Produced by the syntax `i1 % i2` for two `Incr`s.
pub struct MapBuilder2<'a, I1, I2>(Incr<'a, I1>, Incr<'a, I2>);
/// Produced by the syntax `i1 % i2 % i3` for three `Incr`s.
pub struct MapBuilder3<'a, I1, I2, I3>(Incr<'a, I1>, Incr<'a, I2>, Incr<'a, I3>);

impl<'a, I1, I2> Rem<Incr<'a, I2>> for Incr<'a, I1> {
    type Output = MapBuilder2<'a, I1, I2>;
    fn rem(self, rhs: Incr<'a, I2>) -> Self::Output {
        MapBuilder2(self, rhs)
    }
}

impl<'a, I1, I2, I3> Rem<Incr<'a, I3>> for MapBuilder2<'a, I1, I2> {
    type Output = MapBuilder3<'a, I1, I2, I3>;
    fn rem(self, rhs: Incr<'a, I3>) -> Self::Output {
        MapBuilder3(self.0, self.1, rhs)
    }
}

impl<'a, I1: Value<'a>, I2: Value<'a>> MapBuilder2<'a, I1, I2> {
    pub fn map<R: Value<'a>>(&self, f: impl FnMut(&I1, &I2) -> R + 'a) -> Incr<'a, R> {
        let Self(i1, i2) = self;
        i1.map2(i2, f)
    }
}

impl<'a, I1: Value<'a>, I2: Value<'a>, I3: Value<'a>> MapBuilder3<'a, I1, I2, I3> {
    pub fn map<R: Value<'a>>(&self, mut f: impl FnMut(&I1, &I2, &I3) -> R + 'a) -> Incr<'a, R> {
        let Self(i1, i2, i3) = self;
        // TODO: implement map3 and beyond
        let one_two: Incr<(I1, I2)> = i1.map2(i2, |a, b| (a.clone(), b.clone()));
        one_two.map2(i3, move |(a, b), c| f(a, b, c))
    }
}

#[test]
fn test_syntax() {
    let incr = crate::State::new();
    let i1 = incr.constant(5);
    let i2 = incr.constant(10);
    let i3 = incr.constant(9);
    let out = (i1 % i2 % i3).map(|&a, &b, &c| a * b * c);
    let obs = out.observe();
    incr.stabilise();
    assert_eq!(obs.value(), Ok(450));
}

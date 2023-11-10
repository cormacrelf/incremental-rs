//! This module has some syntax helpers.
use std::ops::Rem;

use super::Incr;
use super::Value;

/// Produced by the syntax `i1 % i2` for two `Incr`s.
pub struct MapBuilder2<I1, I2>(Incr<I1>, Incr<I2>);
/// Produced by the syntax `i1 % i2 % i3` for three `Incr`s.
impl<I1, I2> Rem<Incr<I2>> for Incr<I1> {
    type Output = MapBuilder2<I1, I2>;
    fn rem(self, rhs: Incr<I2>) -> Self::Output {
        MapBuilder2(self, rhs)
    }
}

macro_rules! map_builder {
    (@inner $n:ident<$($upto_i:ident),+ > => $n_plus_1:ident<.., $i_plus_1:ident>) => {
        pub struct $n_plus_1<$($upto_i,)+ $i_plus_1>($(Incr<$upto_i>,)+ Incr<$i_plus_1>);

        impl<$($upto_i,)+ $i_plus_1> Rem<Incr<$i_plus_1>> for $n<$($upto_i,)+> {
            type Output = $n_plus_1<$($upto_i,)+ $i_plus_1>;
            fn rem(self, rhs: Incr<$i_plus_1>) -> Self::Output {
                $n_plus_1(self.0, self.1, rhs)
            }
        }
        impl<$($upto_i,)+ $i_plus_1> Rem<&Incr<$i_plus_1>> for $n<$($upto_i,)+> {
            type Output = $n_plus_1<$($upto_i,)+ $i_plus_1>;
            fn rem(self, rhs: &Incr<$i_plus_1>) -> Self::Output {
                $n_plus_1(self.0, self.1, rhs.clone())
            }
        }
    };

    {
        [
            $n:ident<$($upto_i:ident),+>,
        ]
    } => {
    };
    {
        [
            $n:ident<$($upto_i:ident),+>,
            $n_plus_1:ident<.., $i_plus_1:ident>,
            $($rest:tt)*
        ]
    } => {
        map_builder!(@inner $n<$($upto_i),+> => $n_plus_1<.., $i_plus_1>);
        map_builder!([$n_plus_1<$($upto_i,)+ $i_plus_1>, $($rest)*]);
    };
}

map_builder!([
     MapBuilder2<I1, I2>,
     MapBuilder3<.., I3>,
     // MapBuilder4<.., I4>,
]);

impl<I1, I2> Rem<&Incr<I2>> for &Incr<I1> {
    type Output = MapBuilder2<I1, I2>;
    fn rem(self, rhs: &Incr<I2>) -> Self::Output {
        MapBuilder2(self.clone(), rhs.clone())
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

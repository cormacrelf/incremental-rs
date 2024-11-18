//! This module has some syntax helpers.
use std::ops::Rem;

use super::Incr;
use super::Value;

macro_rules! map_builder {
    (@def $(#[$attr:meta])* $n:ident<$vfirst:ident : $ifirst:ident, $($v:ident : $upto_i:ident),+ >::$map:ident) => {
        $(#[$attr])*
        pub struct $n<$ifirst, $($upto_i,)+>(Incr<$ifirst>, $(Incr<$upto_i>,)+);
        impl<$ifirst: Value, $($upto_i: Value,)+> $n<$ifirst, $($upto_i),+> {
            /// Maps the incrementals in the (i1 % i2 % ...) syntax all at once.
            pub fn map<R: Value>(&self, f: impl FnMut(&$ifirst, $(&$upto_i),+) -> R + 'static + crate::incrsan::NotObserver) -> Incr<R> {
                let Self($vfirst, $($v),+) = self;
                $vfirst.$map($($v),+, f)
            }
            /// Zips the incrementals in the (i1 % i2 % ...) syntax into an `Incr<(I1, I2, ...)>`.
            pub fn zip(&self) -> Incr<($ifirst, $($upto_i),+)> {
                let Self($vfirst, $($v),+) = self;
                $vfirst.$map($($v),+, |$vfirst, $($v),+| ($vfirst.clone(), $($v.clone()),+))
            }
        }
    };


    (@def_rem $n:ident<$($v:ident : $upto_i:ident),+ >
              => $(#[$attr:meta])* $n_plus_1:ident<.., $v_plus_1:ident: $i_plus_1:ident>::$map_n_plus_1:ident) => {
        map_builder!(@def $(#[$attr])* $n_plus_1<$($v: $upto_i,)+ $v_plus_1: $i_plus_1>::$map_n_plus_1);
        impl<$($upto_i,)+ $i_plus_1> Rem<Incr<$i_plus_1>> for $n<$($upto_i,)+> {
            type Output = $n_plus_1<$($upto_i,)+ $i_plus_1>;
            fn rem(self, rhs: Incr<$i_plus_1>) -> Self::Output {
                let Self($($v),+) = self;
                $n_plus_1($($v,)+ rhs)
            }
        }
        impl<$($upto_i,)+ $i_plus_1> Rem<&Incr<$i_plus_1>> for $n<$($upto_i,)+> {
            type Output = $n_plus_1<$($upto_i,)+ $i_plus_1>;
            fn rem(self, rhs: &Incr<$i_plus_1>) -> Self::Output {
                let Self($($v),+) = self;
                $n_plus_1($($v,)+ rhs.clone())
            }
        }
    };

    {
        [
            @rest
            $n:ident<$($v:ident: $upto_i:ident),+>::$map_n:ident,
        ]
    } => {
    };
    {
        [
            @rest
            $n:ident<$($v:ident: $upto_i:ident),+>::$map_n:ident,
            $(#[$attr:meta])* $n_plus_1:ident<.., $v_plus_1:ident: $i_plus_1:ident>::$map_n_plus_1:ident,
            $($rest:tt)*
        ]
    } => {
        map_builder!(@def_rem $n<$($v: $upto_i),+> => $(#[$attr])* $n_plus_1<.., $v_plus_1: $i_plus_1>::$map_n_plus_1);
        map_builder!([
             @rest
             $n_plus_1<$($v: $upto_i,)+ $v_plus_1: $i_plus_1>::$map_n_plus_1,
             $($rest)*
        ]);
    };
    {
        [
            $(#[$attr:meta])*
            $n:ident<$($v:ident: $upto_i:ident),+>::$map:ident,
            $($rest:tt)*
        ]
    } => {
        map_builder!(@def $(#[$attr])* $n<$($v: $upto_i),+>::$map);
        map_builder!([@rest $n<$($v: $upto_i),+>::$map, $($rest)*]);
    };
}

map_builder!([
    /// Produced by the syntax `i1 % i2` for two `Incr`s.
    MapBuilder2<i1: I1, i2: I2>::map2,
    /// Produced by the syntax `i1 % i2 % i3` for 3 `Incr`s.
    MapBuilder3<.., i3: I3>::map3,
    /// Produced by the syntax `i1 % i2 % i3 % i4` for 4 `Incr`s.
    MapBuilder4<.., i4: I4>::map4,
    /// Produced by the syntax `i1 % i2 % i3 % i4 % i5` for 5 `Incr`s.
    MapBuilder5<.., i5: I5>::map5,
    /// Produced by the syntax `i1 % i2 % i3 % i4 % i5 % i6` for 6 `Incr`s.
    MapBuilder6<.., i6: I6>::map6,
]);

// Base case

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

#[test]
fn test_syntax() {
    let incr = crate::IncrState::new();
    let i1 = incr.constant(5);
    let i2 = incr.constant(10);
    let i3 = incr.constant(9);
    let out = (&i1 % &i2 % &i3).map(|&a, &b, &c| a * b * c);
    let obs = out.observe();
    incr.stabilise();
    assert_eq!(obs.try_get_value(), Ok(450));

    let i4 = incr.constant(1);
    let i5 = incr.constant(1);
    let i6 = incr.constant(1);
    let _4 = (&i1 % &i2 % &i3 % &i4).map(|_, _, _, _| 0);
    let _5 = (&i1 % &i2 % &i3 % &i4 % &i5).map(|_, _, _, _, _| 0);
    let _6 = (&i1 % &i2 % &i3 % &i4 % &i5 % &i6).map(|_, _, _, _, _, _| 0);
    let _6_owned = (i1 % i2 % i3 % i4 % i5 % i6).map(|_, _, _, _, _, _| 0);
}

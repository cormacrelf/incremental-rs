#![feature(coerce_unsized)]
mod v1;
pub use v1::*;

#[test]
fn test_map() {
    let var = Var::create(5u32);
    let incr = var.read();
    let plus_five = incr.map(|x| x + 5);
    assert_eq!(plus_five.value(), 10);

    // var.set(10u32);
    // incr.stabilise();
    // assert_eq!(plus_five.value(), 15);
}

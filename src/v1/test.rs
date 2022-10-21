use super::*;

#[allow(dead_code)]
fn bind_if<T: Debug + Clone + 'static>(
    cond: Incr<bool>,
    thenn: Incr<T>,
    elsee: Incr<T>,
) -> Incr<T> {
    cond.bind(move |x| if x { thenn.clone() } else { elsee.clone() })
}

#[allow(dead_code)]
#[derive(Debug)]
struct IncrInt(u32);
impl Clone for IncrInt {
    fn clone(&self) -> Self {
        IncrInt(self.0 + 1)
    }
}

#[test]
fn test_clones() {
    let input = Var::create(IncrInt(0));
    let mapped = input.map(|x| IncrInt(x.0 + 1));
    println!("{:?}", mapped);
}

#[test]
fn test_hiding() {
    use std::collections::HashMap;
    let mut steve = HashMap::new();
    steve.insert("name", "steve");
    steve.insert("accoutrements", "personal");
    let person = Var::create(Rc::new(steve.clone()));
    let name = person.map(|x| *x.get("name").unwrap());
    println!("{:?}", name);
    steve.insert("name", "sharon");
    person.set(Rc::new(steve.clone()));
    person.stabilise();
    println!("{:?}", name);
    // TODO: test that setting an unrelated key stops propagation of stabilise().
    // steve.insert("name", "sharon");
    // person.set(Rc::new(steve.clone()));
    // person.stabilise();
    // println!("{:?}", name);
}

#[test]
fn test_map2() {
    let a = Var::create(10);
    let b = Var::create(20);
    let c = a.map2(&b, |a, b| a + b);
    assert_eq!(c.value(), 30);
    a.set(40);
    a.stabilise();
    assert_eq!(c.value(), 60);
}

#[test]
fn test_bind_if() {
    let cond = Var::create(true);
    let b = Var::create(10u32);
    let a_or_b = bind_if(cond.read(), Incr::new(5), b.read());
    println!("{:?}", a_or_b);
    assert_eq!(a_or_b.value(), 5);
    cond.set(false);
    cond.stabilise();
    println!("{:?}", a_or_b);
    assert_eq!(a_or_b.value(), 10);
    println!("setting b to 11");
    b.set(11);
    b.stabilise();
    println!("{:?}", a_or_b);
    assert_eq!(a_or_b.value(), 11);
    // TODO: what do you do when b is not connected, but it gets updated?
    // Do you dirty the bind_if node?
    // Does bind remember which is which somehow?
    println!("setting cond to true");
    cond.set(true);
    cond.stabilise();
    assert_eq!(a_or_b.value(), 5);
    println!("setting b to 50");
    b.set(50);
    b.stabilise();
    println!("setting cond to false");
    cond.set(false);
    cond.stabilise();
    assert_eq!(a_or_b.value(), 50);
    println!("setting b to 90");
    b.set(90);
    b.stabilise();
    assert_eq!(a_or_b.value(), 90);
}

#[test]
fn test_map() {
    let var = Var::create(5u32);
    let incr = var.read();
    let plus_five = incr.map(|x| x + 5);
    println!("{:?}", plus_five);
    assert_eq!(plus_five.value(), 10);

    var.set(10u32);
    println!("before stabilise {:?}", plus_five);
    incr.stabilise();
    println!("after stabilise {:?}", plus_five);
    assert_eq!(plus_five.value(), 15);
}

#[test]
fn multi_input() {
    let mut variables = vec![];
    for _ in 0..10 {
        variables.push(Var::create(1));
    }
    let incrs = variables.iter().map(|inp| inp.read()).collect();
    let list_all = Incr::list_all(incrs);
    println!("{:?}", list_all.value());
    for var in variables.iter_mut().take(5) {
        var.set(2);
    }
    for var in &variables {
        var.stabilise();
    }
    println!("{:?}", list_all.value());
    assert_eq!(&list_all.value(), &[2, 2, 2, 2, 2, 1, 1, 1, 1, 1]);
}

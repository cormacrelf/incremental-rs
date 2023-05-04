# incremental-rs

A port of Jane Street's [Incremental][jane] library.

- Pretty rigorous implementation of the basic Incr features, moderately well 
  tested and benchmarked, performs very similarly to the original.
- Partial implementation of incremental-map

[jane]: https://github.com/janestreet/incremental

Basic usage:

```rust
use incremental::IncrState;

let state = IncrState::new();
let variable = state.var(5);
let times_10 = variable.map(|num| num * 10);
let observer = times_10.observe();

// stabilise will propagate any changes
state.stabilise();
let value = observer.expect_value();
assert_eq!(value, 50);

// now mutate
variable.set(10);
state.stabilise();

// watch as var was propagated through the tree, and reached our observer
assert_eq!(observer.expect_value(), 100);
```

Subscriptions, and an illustration of how propagation stops when nodes produce
the same value as last time:

```rust
use incremental::{IncrState, Update};

// A little system to compute the absolute value of an input
// Note that the input could change (e.g. 5 to -5), but the
// output may stay the same (5 both times).
let state = IncrState::new();
let variable = state.var(5i32);
let absolute = variable.map(|num| num.abs());
let observer = absolute.observe();

// set up a subscription.
use std::{cell::RefCell, rc::Rc};
let latest = Rc::new(RefCell::new(None));
let latest_clone = latest.clone();
let subscription_token = observer.subscribe(move |update| {
     *latest_clone.borrow_mut() = Some(update.cloned());
});

// initial stabilisation
state.stabilise();
assert_eq!(observer.expect_value(), 5);
assert_eq!(latest.borrow().clone(), Some(Update::Initialised(5)));

// now mutate, but such that the output of abs() won't change
variable.set(-5);
state.stabilise();
// The subscription function was not called, because the `absolute` node
// produced the same value as last time we stabilised.
assert_eq!(latest.borrow().clone(), Some(Update::Initialised(5)));
assert_eq!(observer.expect_value(), 5);

// now mutate such that the output changes too
variable.set(-10);
state.stabilise();
// The observer did get a new value, and did call the subscription function
assert_eq!(latest.borrow().clone(), Some(Update::Changed(10)));
assert_eq!(observer.expect_value(), 10);

// now unsubscribe. this also implicitly happens if you drop the observer,
// but you can individually unsubscribe particular subscriptions if you wish.
observer.unsubscribe(subscription_token);
// dropping the observer also unloads any part of the computation graph
// that was only running for the purposes of this particular observer
drop(observer);

// now that the observer is dead, we can mutate the variable and nothing will
// happen, like, at all. The absolute value will not be computed.
variable.set(100000000);
let recomputed = state.stats().recomputed;
state.stabilise();
assert_eq!(recomputed, state.stats().recomputed);
```

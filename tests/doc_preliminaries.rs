fn stabilise_diff(incr: &incremental::IncrState, msg: &str) -> incremental::StatsDiff {
    let before = incr.stats();
    incr.stabilise();
    let delta = incr.stats() - before;
    println!("{msg} : {delta:#?}");
    delta
}

mod projections_and_cutoffs {
    use super::*;
    use std::rc::Rc;

    use incremental::{Incr, IncrState, StatsDiff};
    use test_log::test;

    #[derive(Debug, Clone, PartialEq)]
    struct Z {
        // Rc for cheap clones like they have in OCaml with GC.
        a: Rc<Vec<i32>>,
        b: (i32, i32),
    }

    /// ```
    ///             +---+   +--------+
    ///          .->| a |-->| a_prod |-.
    ///         /   +---+   +--------+  \
    ///   +---+/                         '->+--------+
    ///   | z |                             | result |
    ///   +---+\                         .->+--------+
    ///         \   +---+   +--------+  /
    ///          '->| b |-->| b_prod |-'
    ///             +---+   +--------+
    /// ```
    ///
    fn sumproduct_can_cutoff(z: Incr<Z>) -> Incr<i32> {
        let a_prod = {
            // clone the rc
            let a = z.map(|z| z.a.clone());
            a.map(|a| {
                println!("a_prod ran (expensive)");
                a.iter().fold(1, |acc, x| acc * x)
            })
        };
        let b_prod = {
            let b = z.map(|z| z.b);
            b.map(|&(b1, b2)| {
                println!("b_prod ran (cheap)");
                b1 * b2
            })
        };
        a_prod.map2(&b_prod, |a, b| a + b)
    }

    #[test]
    fn one() {
        let incr = IncrState::new();
        let z = incr.var(Z {
            a: Rc::new(vec![3, 2]),
            b: (1, 4),
        });
        let result = z.pipe(sumproduct_can_cutoff).observe();

        // This creates six nodes total.
        assert_eq!(incr.stats().created, 6);

        let diff = stabilise_diff(
            &incr,
            "initial stabilise, after observing sumproduct_can_cutoff",
        );
        assert!(matches!(
            diff,
            StatsDiff {
                changed: 6,
                recomputed: 6,
                necessary: 6, // and all 6 are participating.
                ..
            }
        ));
        assert_eq!(result.expect_value(), 10);

        // We don't have the problem with "newly allocated tuples" having
        // different pointer addresses. The default cutoff comparator in
        // incremental-rs is PartialEq.
        z.update(|z| z.b = (1, 4));

        let diff = stabilise_diff(&incr, "after updating z.b but with no actual change");
        assert!(matches!(
            diff,
            StatsDiff {
                changed: 0,
                // One for the z watch node. That's it. z.b gets cut off.
                recomputed: 1,
                ..
            }
        ));

        assert_eq!(result.expect_value(), 10);
        z.update(|z| z.b = (5, 6));

        let diff = stabilise_diff(&incr, "after updating z.b");
        assert!(matches!(
            diff,
            StatsDiff {
                changed: 4,    // all except a_prod
                recomputed: 5, // but a_prod didn't need to update at all.
                ..
            }
        ));
        assert_eq!(result.expect_value(), 36);
    }

    /// ```ignore
    ///             +--------+
    ///          .->| a_prod |-.
    ///         /   +--------+  \
    ///   +---+/                 '->+--------+
    ///   | z |                     | result |
    ///   +---+\                 .->+--------+
    ///         \   +--------+  /
    ///          '->| b_prod |-'
    ///             +--------+
    /// ```
    ///
    fn sumproduct_smaller_graph(z: Incr<Z>) -> Incr<i32> {
        let a_prod = z.map(|z| {
            println!("a_prod ran (expensive)");
            z.a.iter().fold(1, |acc, x| acc * x)
        });
        let b_prod = z.map(|&Z { b: (b1, b2), .. }| {
            println!("b_prod ran (cheap)");
            b1 * b2
        });
        a_prod.map2(&b_prod, |a, b| a + b)
    }

    #[test]
    fn two() {
        let incr = IncrState::new();
        let z = incr.var(Z {
            a: Rc::new(vec![3, 2]),
            b: (1, 4),
        });
        let result = z.pipe(sumproduct_smaller_graph).observe();

        let diff = stabilise_diff(&incr, "after observing sumproduct_smaller_graph");
        assert_eq!(incr.stats().created, 4);
        assert!(matches!(
            diff,
            StatsDiff {
                changed: 4,
                recomputed: 4,
                ..
            }
        ));

        z.update(|z| z.b = (5, 6));

        let diff = stabilise_diff(&incr, "after updating z.b");
        assert!(matches!(
            diff,
            StatsDiff {
                changed: 3,    // all except a_prod changed.
                recomputed: 4, // but all of them recomputed.
                ..
            }
        ));

        assert_eq!(result.expect_value(), 36);
    }
}

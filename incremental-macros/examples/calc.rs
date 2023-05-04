//! This example demonstrates how to memoize the execution of scripts which can
//! depend on other scripts---invalidating the result of a script's execution
//! only if a file it depends on changes.
//!
//! It originally appeared in the similar Rust computation caching tool `comemo`
//! at <https://github.com/typst/comemo/blob/9b107f89ad8bdd72776ebef9ac9f4cc9bb861c63/examples/calc.rs>,
//! used under the MIT license.

use incremental::{Incr, IncrState, Var};
use incremental_macros::{db, interned, memoized, InternedString};
use std::collections::HashMap;

db! {
    struct Db {
        /// We are using a HashMap for this basic example, but Incremental will take a clone each
        /// time you modify + stabilise, in order to help determine whether it changed or not and
        /// avoid spurious recomputation. So using an immutable shared-structure `im_rc::HashMap`
        /// is generally a better choice. Or `im_rc::OrdMap`, which can also be used with
        /// incremental-map.
        ///
        /// (note, could be good to have a version of Var that does not do this, same way MapRef
        /// does not do this.)
        files: Var<HashMap<Filename, String>>,
    } provides {
        EvalFile,
        InternedString,
    }
}

interned!(
    /// A newtype of [string_interner::DefaultSymbol].
    ///
    /// Can be used with a Db providing `incremental_macros::InternedString`.
    type Filename = String;
);

memoized! {
    fn eval_file(db: &Db, filename: Filename) -> Incr<i32> {
        let files = db.files.watch();

        // This is our caching layer. We are only interested in changes in the script stored
        // against `filename`.
        let script = files.map(move |files| {
            files.get(&filename).cloned().unwrap_or_default()
        });

        // using bind, each time the script for this filename is changed, we generate a new
        // computation graph. The shape of the computation will change depending on what we
        // write in the scripts.
        let db = db.clone();
        script.bind(move |script| eval_script(&db, script))
    }
}

fn eval_script(db: &Db, script: &str) -> Incr<i32> {
    let db = db.clone();

    // for "2 + 3 + eval file.calc", this will end up holding 5.
    let mut literal_sum = 0;

    // The rest of the components of the addition are Incr<i32>s, which we will sum together
    // using the `fold` method.
    let incr_components = script
        .split('+')
        .map(str::trim)
        .filter_map(|part| match part.strip_prefix("eval ") {
            Some(path) => {
                let filename = Filename::new(&db, path);
                Some(eval_file(&db, filename))
            }
            None => {
                literal_sum += part.parse::<i32>().unwrap();
                None
            }
        })
        .collect::<Vec<_>>();

    // we initialise the fold with `literal_sum`.
    // we could, alternatively, push an `db.incr.constant(literal_sum)` onto the array
    // and then initialise with `0`, but we like to avoid creating unnecessary Incrs.
    db.incr.fold(incr_components, literal_sum, |a, b| a + b)
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let incr = IncrState::new();
    let db = &Db::new(&incr, incr.var(HashMap::new()));

    let alpha_calc = Filename::new(db, "alpha.calc");
    let beta_calc = Filename::new(db, "beta.calc");
    let gamma_calc = Filename::new(db, "gamma.calc");

    db.files.modify(|fs| {
        fs.insert(alpha_calc, "2 + eval beta.calc".into());
        fs.insert(beta_calc, "2 + 3".into());
        fs.insert(gamma_calc, "8 + 3".into());
    });

    let alpha_incr = eval_file(db, alpha_calc);
    let alpha_obs = alpha_incr.observe();

    // The Incr<i32> is cached using the filename as a key. Since eval_file
    // is recursively defined, each dependency ("eval beta.calc") will use
    // the cached version if possible. Multiple files referencing the same
    // beta.calc will reuse the results of evaluating it.
    let alpha_2 = eval_file(db, alpha_calc);
    assert_eq!(alpha_2, alpha_incr);

    //
    // Dependencies
    //

    // This is our first stabilise call. The variable `files` is new, and
    // has never propagated its changes. Both alpha.calc and beta.calc
    // are new and will be computed for the first time..
    incr.stabilise();
    assert_eq!(alpha_obs.expect_value(), 7);

    // A cache hit.
    //
    // beta.calc was a dependency of alpha.calc, so if you grab its node
    // and slap an observer on it, you get the results for free.
    //
    // Nevertheless we must call incr.stabilise(), as observers can't give
    // you their value until they have been stabilised. If you remove the call,
    // you'll notice `beta.try_get_value()` returns `Err(ObserverError::NeverStabilised)`.
    let beta = eval_file(db, beta_calc).observe();
    incr.stabilise();
    assert_eq!(beta.try_get_value(), Ok(5));

    // Modify the gamma file. Nothing depends on gamma.calc. This will simply
    // propagate the new value of `files` to the caching nodes for alpha & beta,
    // and stop there.
    db.files.modify(|fs| {
        fs.insert(gamma_calc, "42".into());
    });
    incr.stabilise();
    assert_eq!(alpha_obs.expect_value(), 7);

    // Now beta.calc will depend on gamma.calc
    db.files.modify(|fs| {
        fs.insert(beta_calc, "4 + eval gamma.calc".into());
    });
    incr.stabilise();

    // Alpha and beta both have to recompute, because beta.calc was referenced by alpha.calc.
    // There is now a computation for gamma.calc in the mix, because it is referenced now.
    assert_eq!(alpha_obs.expect_value(), 48);

    // Observers and the computation graph

    // observe gamma.calc.
    let gamma = eval_file(db, gamma_calc)
        .with_graphviz_user_data("gamma observes this node")
        .observe();
    incr.save_dot_to_file("gamma-in-use.dot");

    // Drop the gamma.calc reference from beta.calc. Alpha does not depend on gamma any more.
    db.files.modify(|fs| {
        fs.insert(beta_calc, "43".into());
    });

    incr.stabilise();
    assert_eq!(alpha_obs.expect_value(), 45);
    incr.save_dot_to_file("gamma-hanging-around.dot");

    // The gamma observer is now the only thing hanging onto the evaluated gamma.calc computation.
    // Run with `RUST_LOG=trace cargo run --example calc` to see the computation being torn down.
    tracing::trace_span!("drop(gamma)").in_scope(|| {
        drop(gamma);
        incr.stabilise();
    });
    incr.save_dot_to_file("gamma-gone.dot");

    // If we add a cycle, stabilise will panic. There should be a better way to recover from such a
    // cycle.

    // db.files.modify(|fs| {
    //     fs.insert(alpha_calc, "10 + eval alpha.calc".into());
    // });
    // incr.stabilise();
}

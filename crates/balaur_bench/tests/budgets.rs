//! Performance budgets, kept apart from the feature tests on purpose.
//!
//! These assert orders of magnitude, not percentages. A CI runner is shared,
//! throttled and unpredictable, so a tight budget would fail for reasons that
//! have nothing to do with the commit — and a gate that cries wolf gets
//! ignored, which is worse than no gate.
//!
//! Every budget here is 10x the measured cost, and none of them is written
//! down twice: each test names the benchmark it gates and reads the ceiling
//! from budgets.toml, which `bench.py --record` writes from a real run. They
//! catch an accidental O(n^2), a lock held across a frame, or a compile moved
//! into the hot path. For real numbers, and to move a ceiling:
//!
//! ```text
//! python3 scripts/bench.py --quick            # the table
//! python3 scripts/bench.py --quick --record   # rewrite budgets.toml
//! ```

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use balaur_bench::{app, attach_many, Backend, Project};

/// One measurement at a time: four spinning tests on a four-vCPU runner
/// measure each other, not the engine.
fn alone() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Run `f` until it has taken at least `min`, and report the per-iteration
/// cost. Timing one pass of a fast operation measures the clock.
#[allow(
    clippy::disallowed_methods,
    reason = "this is the measurement, not simulation"
)]
fn per_iteration(min: Duration, mut f: impl FnMut()) -> Duration {
    // Warm up: first-touch page faults and lazy init are not what we measure.
    for _ in 0..3 {
        f();
    }
    let start = Instant::now();
    let mut runs = 0u32;
    while start.elapsed() < min {
        f();
        runs += 1;
    }
    start.elapsed() / runs.max(1)
}

/// The ceiling recorded for one benchmark id, from budgets.toml.
///
/// Panics rather than skipping: a gate that quietly passes when its number is
/// missing is the failure mode this whole file exists to avoid.
fn ceiling(id: &str) -> Duration {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("budgets.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let document: toml::Table = text.parse().expect("budgets.toml is not valid TOML");
    let ns = document
        .get("ceiling_ns")
        .and_then(toml::Value::as_table)
        .and_then(|t| t.get(id))
        .and_then(toml::Value::as_integer)
        .unwrap_or_else(|| {
            panic!("no ceiling for '{id}'; run `python3 scripts/bench.py --quick --record`")
        });
    Duration::from_nanos(ns.try_into().expect("a ceiling is positive"))
}

fn assert_under(id: &str, actual: Duration) {
    let budget = ceiling(id);
    assert!(
        actual <= budget,
        "{id} took {actual:?}, budget is {budget:?} — \
         run `python3 scripts/bench.py` to see what moved"
    );
}

const EMPTY: [(Backend, &str); 1] = [(
    Backend::Rune,
    "pub fn init(this) {}\npub fn update(this, dt) {}\n",
)];

#[test]
fn dispatch_over_a_thousand_nodes_stays_inside_a_frame() {
    let _alone = alone();
    for (backend, source) in EMPTY {
        let project = Project::new(backend, source).unwrap();
        let app = app(backend, &project).unwrap();
        attach_many(&app, backend, 1000).unwrap();
        let host = app.engine.script_host().unwrap();
        let each = per_iteration(Duration::from_millis(120), || host.update(1.0 / 60.0));
        assert_under("update_per_node/empty_rune/1000", each);
    }
}

#[test]
fn attaching_to_a_compiled_script_is_cheap_per_node() {
    let _alone = alone();
    for (backend, source) in EMPTY {
        let project = Project::new(backend, source).unwrap();
        let app = app(backend, &project).unwrap();
        attach_many(&app, backend, 1).unwrap(); // pay the compile first
                                                // A hundred at a time, because that is the batch the benchmark this
                                                // gates measures, and the ceiling comes from it.
        let each = per_iteration(Duration::from_millis(120), || {
            attach_many(&app, backend, 100).unwrap();
        });
        assert_under("attach_more/rune/100", each);
    }
}

#[test]
fn transform_propagation_stays_linear() {
    // Wide and shallow, the shape a real scene has: propagation recurses per
    // level, so one long chain overflows the stack instead of measuring.
    const DEPTH: usize = 50;
    let _alone = alone();
    let project = Project::new(Backend::Rune, EMPTY[0].1).unwrap();
    let app = app(Backend::Rune, &project).unwrap();
    let root = app.engine.root();
    let mut cost = Vec::new();
    for count in [500usize, 5000] {
        {
            let mut world = app.engine.world_mut();
            let mut parent = root;
            for i in 0..count {
                parent = balaur_core::scene::spawn_node(&mut world, &format!("n{i}"), parent);
                if i % DEPTH == DEPTH - 1 {
                    parent = root;
                }
            }
        }
        cost.push(per_iteration(Duration::from_millis(120), || {
            balaur_core::scene::propagate_transforms(&mut app.engine.world_mut(), root);
        }));
    }
    // 10x the nodes should be near 10x the time, not 100x. Allow 30x for noise
    // and cache effects; a quadratic walk would be far past it.
    let ratio = cost[1].as_secs_f64() / cost[0].as_secs_f64().max(f64::EPSILON);
    assert!(
        ratio < 30.0,
        "propagation went superlinear: 500 nodes {:?}, 5000 nodes {:?} ({ratio:.1}x)",
        cost[0],
        cost[1]
    );
}

#[test]
fn a_binding_call_stays_sub_microsecond() {
    let _alone = alone();
    for backend in Backend::ALL {
        let source = match backend {
            Backend::Rune => "pub fn init(this) {}\npub fn update(this, dt) { bench::noop(1); }\n",
        };
        let project = Project::new(backend, source).unwrap();
        let mut app = app(backend, &project).unwrap();
        {
            use balaur_script::Bindings;
            let mut m = app.script_module("bench").unwrap();
            m.function_raw("noop", Box::new(|_, _| Ok(balaur_script::Value::Nil)));
        }
        attach_many(&app, backend, 1).unwrap();
        let host = app.engine.script_host().unwrap();
        let each = per_iteration(Duration::from_millis(120), || host.update(1.0 / 60.0));
        assert_under("binding_call/rune", each);
    }
}

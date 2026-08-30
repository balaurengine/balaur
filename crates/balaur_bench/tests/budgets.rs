//! Performance budgets, kept apart from the feature tests on purpose.
//!
//! These assert orders of magnitude, not percentages. A CI runner is shared,
//! throttled and unpredictable, so a tight budget would fail for reasons that
//! have nothing to do with the commit — and a gate that cries wolf gets
//! ignored, which is worse than no gate.
//!
//! Every budget here is roughly 10x the measured cost on a developer machine.
//! They catch an accidental O(n^2), a lock held across a frame, or a compile
//! moved into the hot path. For real numbers use the benchmarks:
//!
//! ```text
//! python3 scripts/bench.py --quick
//! ```

use std::time::{Duration, Instant};

use balaur_bench::{app, attach_many, Backend, Project};

/// Run `f` until it has taken at least `min`, and report the per-iteration
/// cost. Timing one pass of a fast operation measures the clock.
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

fn assert_under(what: &str, actual: Duration, budget: Duration) {
    assert!(
        actual <= budget,
        "{what} took {actual:?}, budget is {budget:?} — \
         run `python3 scripts/bench.py` to see what moved"
    );
}

const EMPTY: [(Backend, &str); 2] = [
    (
        Backend::Luau,
        "local S = {}\nfunction S:init() end\nfunction S:update(dt) end\nreturn S\n",
    ),
    (
        Backend::Rune,
        "pub fn init(this) {}\npub fn update(this, dt) {}\n",
    ),
];

/// Dispatch across a thousand nodes is the shape of a real frame. Measured at
/// roughly 200-260 us; budget 4 ms, well inside one 16.6 ms frame.
#[test]
fn dispatch_over_a_thousand_nodes_stays_inside_a_frame() {
    for (backend, source) in EMPTY {
        let project = Project::new(backend, source).unwrap();
        let app = app(backend, &project).unwrap();
        attach_many(&app, backend, 1000).unwrap();
        let host = app.engine.scripts().unwrap();
        let each = per_iteration(Duration::from_millis(120), || host.update(1.0 / 60.0));
        assert_under(
            &format!("{}: update over 1000 nodes", backend.name()),
            each,
            Duration::from_millis(4),
        );
    }
}

/// Attaching to already-compiled scripts must stay per-node cheap. If a
/// compile ever creeps back into this path the number jumps by orders of
/// magnitude, which is exactly what this catches.
#[test]
fn attaching_to_a_compiled_script_is_cheap_per_node() {
    for (backend, source) in EMPTY {
        let project = Project::new(backend, source).unwrap();
        let app = app(backend, &project).unwrap();
        attach_many(&app, backend, 1).unwrap(); // pay the compile first
        let each = per_iteration(Duration::from_millis(120), || {
            attach_many(&app, backend, 20).unwrap();
        });
        assert_under(
            &format!("{}: attach 20 nodes", backend.name()),
            each,
            Duration::from_millis(3),
        );
    }
}

/// Propagation is linear in node count. A budget that scales with n catches a
/// quadratic walk, which is the failure this path is prone to.
#[test]
fn transform_propagation_stays_linear() {
    let project = Project::new(Backend::Luau, EMPTY[0].1).unwrap();
    let app = app(Backend::Luau, &project).unwrap();
    let root = app.engine.root();
    let mut cost = Vec::new();
    for count in [500usize, 5000] {
        {
            let mut world = app.engine.world_mut();
            let mut parent = root;
            for i in 0..count {
                parent = balaur_core::scene::spawn_node(&mut world, &format!("n{i}"), parent);
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

/// A binding call crossing the seam should stay in the hundreds of nanoseconds.
/// Budget 20 us catches a call that started allocating or locking.
#[test]
fn a_binding_call_stays_sub_microsecond() {
    for backend in Backend::ALL {
        let source = match backend {
            Backend::Luau => "local S = {}\nfunction S:init() end\nfunction S:update(dt) bench.noop(1) end\nreturn S\n",
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
        let host = app.engine.scripts().unwrap();
        let each = per_iteration(Duration::from_millis(120), || host.update(1.0 / 60.0));
        assert_under(
            &format!("{}: one binding call", backend.name()),
            each,
            Duration::from_micros(20),
        );
    }
}

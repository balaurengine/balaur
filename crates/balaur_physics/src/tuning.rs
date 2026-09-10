//! The knobs that change how the solver behaves, and the two things rapier
//! tells us about a step that went wrong.
//!
//! Every value here changes results. That makes it a determinism concern, not
//! a preference: a recording replays correctly only against the same numbers,
//! so `[physics]` in `project.toml` is the truth a game ships with, and the
//! script setters are for a game that tunes at run time and knows it.

use crate::rapier3d::dynamics::IntegrationParameters;
use balaur_core::hecs::Entity;
use balaur_core::{Engine, Stage};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, Value};

use crate::vocabulary::{Opts, keys as k, map};
use crate::{PhysicsState, PhysicsState2d};

/// One writer, two dimensions.
///
/// `IntegrationParameters` is the same struct field for field in rapier2d and
/// rapier3d, and two distinct types to the compiler. A macro is the honest way
/// to write it once (P4: share the vocabulary, not the calls).
macro_rules! write_parameters {
    ($p:expr_2021, $f:expr_2021, $boolean:expr_2021) => {{
        let p = $p;
        // Every knob is rapier's own scalar; the caller hands `f32` because
        // scenes and scripts do.
        let f = |key: &str, default: crate::scalar::Real| -> crate::scalar::Real {
            crate::scalar::real(($f)(key, crate::scalar::f32_of(default)))
        };
        let boolean = $boolean;
        p.num_solver_iterations =
            f(k::SOLVER_ITERATIONS, p.num_solver_iterations as _).max(1.0) as usize;
        p.num_internal_pgs_iterations =
            f(k::INTERNAL_ITERATIONS, p.num_internal_pgs_iterations as _).max(0.0) as usize;
        p.num_internal_stabilization_iterations = f(
            k::STABILIZATION_ITERATIONS,
            p.num_internal_stabilization_iterations as _,
        )
        .max(0.0) as usize;
        p.max_ccd_substeps = f(k::CCD_SUBSTEPS, p.max_ccd_substeps as _).max(0.0) as usize;
        p.min_ccd_dt = f(k::MIN_CCD_DT, p.min_ccd_dt);
        // The one knob a 2D game in pixels cannot do without: every tolerance
        // in the solver is scaled by it, and at 64 pixels per metre the
        // defaults are sixty-four times too loose.
        p.length_unit = f(k::LENGTH_UNIT, p.length_unit).max(1.0e-6);
        p.warmstart_coefficient = f(k::WARMSTART, p.warmstart_coefficient);
        p.warmstart_joints = boolean(k::WARMSTART_JOINTS, p.warmstart_joints);
        p.contact_clustering = boolean(k::CONTACT_CLUSTERING, p.contact_clustering);
        p.contact_recycling = boolean(k::CONTACT_RECYCLING, p.contact_recycling);
        p.friction_in_bias_pass = boolean(k::FRICTION_IN_BIAS_PASS, p.friction_in_bias_pass);
        p.normalized_allowed_linear_error =
            f(k::ALLOWED_LINEAR_ERROR, p.normalized_allowed_linear_error);
        p.normalized_max_corrective_velocity = f(
            k::MAX_CORRECTIVE_VELOCITY,
            p.normalized_max_corrective_velocity,
        );
        p.normalized_prediction_distance =
            f(k::PREDICTION_DISTANCE, p.normalized_prediction_distance);
        p.normalized_max_linear_velocity =
            f(k::MAX_LINEAR_VELOCITY, p.normalized_max_linear_velocity);
        p.contact_softness.natural_frequency =
            f(k::CONTACT_FREQUENCY, p.contact_softness.natural_frequency);
        p.contact_softness.damping_ratio = f(k::CONTACT_DAMPING, p.contact_softness.damping_ratio);
        p.static_contact_softness.natural_frequency = f(
            k::STATIC_CONTACT_FREQUENCY,
            p.static_contact_softness.natural_frequency,
        );
        p.static_contact_softness.damping_ratio = f(
            k::STATIC_CONTACT_DAMPING,
            p.static_contact_softness.damping_ratio,
        );
    }};
}

/// Applied once, after the project has loaded, from `[physics]` in
/// `project.toml`. A game with no such section keeps rapier's defaults.
pub(crate) fn build(reg: &mut Registry<'_>) {
    reg.insert_resource(SolverThreads::default());
    reg.insert_resource(ManifestTuning { applied: false });
    reg.add_system(Stage::First, manifest_tuning_system);
}

/// Whether the manifest's `[physics]` table has been read yet.
///
/// The plugin is built before the project is loaded, so the table cannot be
/// read at build time; this is the flag that makes the first tick do it.
struct ManifestTuning {
    applied: bool,
}

fn manifest_tuning_system(eng: &Engine, _dt: f32) {
    {
        let flag = eng.resource::<ManifestTuning>();
        if flag.borrow().applied {
            return;
        }
        if eng
            .try_resource::<balaur_core::project::ProjectManifest>()
            .is_none()
        {
            return;
        }
        flag.borrow_mut().applied = true;
        read_manifest_threads(eng);
        write_tuning_from_settings(eng);
    }
    // Last thing before the first step: everything that had a say has had it.
    build_pool(eng);
}

/// One `[physics]` key, as this platform resolves it: an override answers
/// here the same way it answers anywhere else.
///
/// `stated`, not `get`: rapier's own default is the fallback, and a schema
/// default answering for an absent key would overwrite it.
fn setting(eng: &Engine, key: &str) -> Option<toml::Value> {
    balaur_core::settings::stated(eng, &format!("physics/{key}"))
}

/// `[physics] threads`, which a script's own `set_threads` outranks: the
/// manifest is what a project usually wants and the call is what this run does.
fn read_manifest_threads(eng: &Engine) {
    let Some(count) = setting(eng, k::THREADS).as_ref().and_then(toml::Value::as_integer) else {
        return;
    };
    if eng.resource::<SolverThreads>().borrow().asked {
        return;
    }
    want_threads(eng, count.max(1) as usize, false);
}

/// The `[physics]` settings, onto both worlds.
fn write_tuning_from_settings(eng: &Engine) {
    let f = |key: &str, default: f32| {
        setting(eng, key)
            .as_ref()
            .and_then(balaur_core::components::as_f64)
            .map_or(default, |n| n as f32)
    };
    let boolean = |key: &str, default: bool| {
        setting(eng, key)
            .as_ref()
            .and_then(toml::Value::as_bool)
            .unwrap_or(default)
    };
    {
        let state = eng.resource::<PhysicsState>();
        let mut state = state.borrow_mut();
        write_parameters!(&mut state.world.integration_parameters, &f, &boolean);
    }
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    write_parameters!(&mut state.world.integration_parameters, &f, &boolean);
}

/// The parameters as a table, for the reader every setter owes (N8).
fn tuning_value(p: &IntegrationParameters) -> Value {
    map([
        (
            k::SOLVER_ITERATIONS,
            Value::Num(p.num_solver_iterations as f64),
        ),
        (
            k::INTERNAL_ITERATIONS,
            Value::Num(p.num_internal_pgs_iterations as f64),
        ),
        (
            k::STABILIZATION_ITERATIONS,
            Value::Num(p.num_internal_stabilization_iterations as f64),
        ),
        (k::CCD_SUBSTEPS, Value::Num(p.max_ccd_substeps as f64)),
        (k::MIN_CCD_DT, Value::Num(f64::from(p.min_ccd_dt))),
        (k::LENGTH_UNIT, Value::Num(f64::from(p.length_unit))),
        (k::WARMSTART, Value::Num(f64::from(p.warmstart_coefficient))),
        (k::WARMSTART_JOINTS, Value::Bool(p.warmstart_joints)),
        (k::CONTACT_CLUSTERING, Value::Bool(p.contact_clustering)),
        (k::CONTACT_RECYCLING, Value::Bool(p.contact_recycling)),
        (
            k::FRICTION_IN_BIAS_PASS,
            Value::Bool(p.friction_in_bias_pass),
        ),
        (
            k::ALLOWED_LINEAR_ERROR,
            Value::Num(f64::from(p.normalized_allowed_linear_error)),
        ),
        (
            k::MAX_CORRECTIVE_VELOCITY,
            Value::Num(f64::from(p.normalized_max_corrective_velocity)),
        ),
        (
            k::PREDICTION_DISTANCE,
            Value::Num(f64::from(p.normalized_prediction_distance)),
        ),
        (
            k::MAX_LINEAR_VELOCITY,
            Value::Num(f64::from(p.normalized_max_linear_velocity)),
        ),
        (
            k::CONTACT_FREQUENCY,
            Value::Num(f64::from(p.contact_softness.natural_frequency)),
        ),
        (
            k::CONTACT_DAMPING,
            Value::Num(f64::from(p.contact_softness.damping_ratio)),
        ),
        (
            k::STATIC_CONTACT_FREQUENCY,
            Value::Num(f64::from(p.static_contact_softness.natural_frequency)),
        ),
        (
            k::STATIC_CONTACT_DAMPING,
            Value::Num(f64::from(p.static_contact_softness.damping_ratio)),
        ),
    ])
}

pub(crate) fn install_tuning_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_tuning", &[], "(opts: table)", "Change how the solver behaves in both worlds: `solver_iterations`, `length_unit`, `ccd_substeps`, contact softness and the rest. Every value here changes results, so a recording only replays against the same numbers: prefer `[physics]` in project.toml."),
        ("tuning", &[], "()", "The solver settings both worlds are running with."),
        ("quarantined", &[], "()", "The nodes rapier disabled this step because their position or velocity stopped being a number. Empty is the normal answer."),
        ("counters", &[], "()", "What the last step spent its time on. The first call turns rapier's profiler on, so the numbers arrive from the step after it."),
        ("set_threads", &[], "(count: int)", "How many threads the solver may use, from a script's `init`: rayon's pool is built once, before the first step, and a call after that says so and changes nothing. `[physics] threads` in `project.toml` does the same and outranks nothing -- an `init` that asks wins. The default is one less than the machine reports, capped at eight."),
        ("threads", &[], "()", "How many threads the solver is using. One in a browser, unless the page is the threaded template and called `initThreadPool`."),
    ]);
    m.function("set_tuning", |eng: &Engine, opts: Value| {
        let opts = Opts(Some(&opts));
        let f = |key: &str, default: f32| opts.f32(key, default);
        let boolean = |key: &str, default: bool| opts.boolean(key, default);
        {
            let state = eng.resource::<PhysicsState>();
            let mut state = state.borrow_mut();
            write_parameters!(&mut state.world.integration_parameters, &f, &boolean);
        }
        let state = eng.resource::<PhysicsState2d>();
        let mut state = state.borrow_mut();
        write_parameters!(&mut state.world.integration_parameters, &f, &boolean);
        Ok(())
    });
    m.function("tuning", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        Ok(tuning_value(&state.world.integration_parameters))
    });
    // Rapier disables a body whose pose or velocity goes non-finite rather
    // than letting the whole world become NaN. Saying which node it was is the
    // difference between a bug report and a mystery.
    m.function("quarantined", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        let quarantine = state.world.quarantine();
        let mut nodes: Vec<Entity> = quarantine
            .bodies()
            .iter()
            .filter_map(|handle| {
                state
                    .bodies
                    .iter()
                    .find(|(_, h)| *h == handle)
                    .map(|(entity, _)| *entity)
            })
            .collect();
        nodes.sort_unstable_by_key(|e| e.to_bits());
        Ok(Value::List(
            nodes
                .into_iter()
                .map(|e| Value::Node(e.to_bits().get()))
                .collect(),
        ))
    });
    m.function("set_threads", |eng: &Engine, count: i64| {
        ask_for_threads(eng, count);
        Ok(Value::Nil)
    });
    m.function("threads", |_eng: &Engine, ()| {
        Ok(i64::try_from(threads()).unwrap_or(i64::MAX))
    });
    // Rapier's profiler is off until something asks for it, so the first call
    // turns it on and the numbers arrive from the step after this one.
    m.function("counters", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState>();
        let mut state = state.borrow_mut();
        state.world.physics_pipeline.counters.enable();
        let counters = &state.world.physics_pipeline.counters;
        Ok(map([
            (k::STEP_MS, Value::Num(counters.step_time_ms())),
            (
                k::BROAD_PHASE_MS,
                Value::Num(counters.cd.broad_phase_time.time_ms()),
            ),
            (
                k::NARROW_PHASE_MS,
                Value::Num(counters.cd.narrow_phase_time.time_ms()),
            ),
            (
                k::CONTACT_PAIRS,
                Value::Num(counters.cd.ncontact_pairs as f64),
            ),
        ]))
    });
}

/// Report anything rapier had to quarantine, once per step, so a game that
/// never calls `physics.quarantined()` still learns about it.
pub(crate) fn warn_about_quarantine(eng: &Engine) {
    let state = eng.resource::<PhysicsState>();
    let state = state.borrow();
    let quarantine = state.world.quarantine();
    if quarantine.is_empty() {
        return;
    }
    let world = eng.world();
    for handle in quarantine.bodies() {
        if let Some((entity, _)) = state.bodies.iter().find(|(_, h)| *h == handle) {
            tracing::warn!(
                node = %balaur_core::digest::node_label(&world, *entity),
                "physics disabled this body: its position or velocity stopped being a number"
            );
        }
    }
}

/// How many threads the solver should take, until the pool is built.
///
/// Rayon's pool is global and sized once per process, so the count has to be
/// settled before anything steps. It is held here rather than applied at
/// registration so `[physics] threads` and a script's `init` can still say.
pub(crate) struct SolverThreads {
    wanted: usize,
    /// A script asked, so the manifest does not overrule it.
    asked: bool,
    built: bool,
}

impl Default for SolverThreads {
    fn default() -> Self {
        Self {
            wanted: default_threads(),
            asked: false,
            built: false,
        }
    }
}

/// A script asking for `count` threads, and the reason it did not get them.
#[cfg(not(target_family = "wasm"))]
fn ask_for_threads(eng: &Engine, count: i64) {
    if !want_threads(eng, count.max(1) as usize, true) {
        tracing::warn!(
            "physics.set_threads({count}) came after the solver's pool was built; it keeps the \
             {} it has. Call it from a script's `init`, or set `[physics] threads`.",
            threads()
        );
    }
}

/// The page owns the pool in a browser, and it is sized before the engine runs.
#[cfg(target_family = "wasm")]
fn ask_for_threads(_eng: &Engine, count: i64) {
    tracing::warn!(
        "physics.set_threads({count}) is the page's to make: hand the count to `initThreadPool` \
         before starting the engine, on the threaded template."
    );
}

/// Ask for `count` threads, if the pool has not been built yet.
///
/// Answers whether the ask landed, so the caller can say why it did not.
fn want_threads(eng: &Engine, count: usize, asked: bool) -> bool {
    let held = eng.resource::<SolverThreads>();
    let mut held = held.borrow_mut();
    if held.built {
        return false;
    }
    held.wanted = count.max(1);
    held.asked = held.asked || asked;
    true
}

/// Build rayon's pool from what was asked for, once, before the first step.
///
/// The page owns the pool in a browser: it hands a count to `initThreadPool`
/// before the engine starts, and building one here would race that.
fn build_pool(eng: &Engine) {
    let held = eng.resource::<SolverThreads>();
    let count = {
        let mut held = held.borrow_mut();
        if held.built {
            return;
        }
        held.built = true;
        held.wanted
    };
    let _ = count;
    #[cfg(all(feature = "parallel", not(target_family = "wasm")))]
    if let Err(why) = rayon::ThreadPoolBuilder::new()
        .num_threads(count)
        .build_global()
    {
        tracing::warn!("the solver keeps the pool it already had: {why}");
    }
}

/// How many threads the solver is running on.
#[cfg(feature = "parallel")]
fn threads() -> usize {
    rayon::current_num_threads()
}

/// The one thread a build without the `parallel` feature runs the solver on.
#[cfg(not(feature = "parallel"))]
fn threads() -> usize {
    1
}

/// What the solver takes when a game says nothing.
///
/// One less than the machine reports, so the frame's own thread is not
/// competing with the solver, and capped because a step stops scaling long
/// before a large machine runs out of cores. `available_parallelism` answers 1
/// on wasm and follows a container's CPU limit, so both come out right.
///
/// Safe to vary per machine: rapier's solver is coloured, so the digest is the
/// same at one thread and at eight.
fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .saturating_sub(1)
        .clamp(1, 8)
}

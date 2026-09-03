//! The knobs that change how the solver behaves, and the two things rapier
//! tells us about a step that went wrong.
//!
//! Every value here changes results. That makes it a determinism concern, not
//! a preference: a recording replays correctly only against the same numbers,
//! so `[physics]` in `project.toml` is the truth a game ships with, and the
//! script setters are for a game that tunes at run time and knows it.

use crate::rapier3d::dynamics::IntegrationParameters;
use anyhow::Result;
use balaur_core::hecs::Entity;
use balaur_core::{App, Engine, Stage};
use balaur_script::{Bindings, BindingsExt, Value};

use crate::vocabulary::{map, Opts};
use crate::{PhysicsState, PhysicsState2d};

/// One writer, two dimensions.
///
/// `IntegrationParameters` is the same struct field for field in rapier2d and
/// rapier3d, and two distinct types to the compiler. A macro is the honest way
/// to write it once (P4: share the vocabulary, not the calls).
macro_rules! write_parameters {
    ($p:expr, $f:expr, $boolean:expr) => {{
        let p = $p;
        // Every knob is rapier's own scalar; the caller hands `f32` because
        // scenes and scripts do.
        let f = |key: &str, default: crate::scalar::Real| -> crate::scalar::Real {
            crate::scalar::real(($f)(key, crate::scalar::f32_of(default)))
        };
        let boolean = $boolean;
        p.num_solver_iterations =
            f("solver_iterations", p.num_solver_iterations as _).max(1.0) as usize;
        p.num_internal_pgs_iterations =
            f("internal_iterations", p.num_internal_pgs_iterations as _).max(0.0) as usize;
        p.num_internal_stabilization_iterations = f(
            "stabilization_iterations",
            p.num_internal_stabilization_iterations as _,
        )
        .max(0.0) as usize;
        p.max_ccd_substeps = f("ccd_substeps", p.max_ccd_substeps as _).max(0.0) as usize;
        p.min_ccd_dt = f("min_ccd_dt", p.min_ccd_dt);
        // The one knob a 2D game in pixels cannot do without: every tolerance
        // in the solver is scaled by it, and at 64 pixels per metre the
        // defaults are sixty-four times too loose.
        p.length_unit = f("length_unit", p.length_unit).max(1.0e-6);
        p.warmstart_coefficient = f("warmstart", p.warmstart_coefficient);
        p.warmstart_joints = boolean("warmstart_joints", p.warmstart_joints);
        p.contact_clustering = boolean("contact_clustering", p.contact_clustering);
        p.contact_recycling = boolean("contact_recycling", p.contact_recycling);
        p.friction_in_bias_pass = boolean("friction_in_bias_pass", p.friction_in_bias_pass);
        p.normalized_allowed_linear_error =
            f("allowed_linear_error", p.normalized_allowed_linear_error);
        p.normalized_max_corrective_velocity = f(
            "max_corrective_velocity",
            p.normalized_max_corrective_velocity,
        );
        p.normalized_prediction_distance =
            f("prediction_distance", p.normalized_prediction_distance);
        p.normalized_max_linear_velocity =
            f("max_linear_velocity", p.normalized_max_linear_velocity);
        p.contact_softness.natural_frequency =
            f("contact_frequency", p.contact_softness.natural_frequency);
        p.contact_softness.damping_ratio = f("contact_damping", p.contact_softness.damping_ratio);
        p.static_contact_softness.natural_frequency = f(
            "static_contact_frequency",
            p.static_contact_softness.natural_frequency,
        );
        p.static_contact_softness.damping_ratio = f(
            "static_contact_damping",
            p.static_contact_softness.damping_ratio,
        );
    }};
}

/// Applied once, after the project has loaded, from `[physics]` in
/// `project.toml`. A game with no such section keeps rapier's defaults.
pub(crate) fn build(app: &mut App) {
    app.engine
        .insert_resource(ManifestTuning { applied: false });
    app.add_system(Stage::First, manifest_tuning_system);
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
        let Some(source) = balaur_core::project::manifest_source(eng) else {
            return;
        };
        flag.borrow_mut().applied = true;
        let Ok(manifest) = source.parse::<toml::Value>() else {
            return;
        };
        let Some(section) = manifest.get("physics") else {
            return;
        };
        write_tuning_from_toml(eng, section);
    }
}

/// The `[physics]` table, onto both worlds.
fn write_tuning_from_toml(eng: &Engine, section: &toml::Value) {
    let f = |key: &str, default: f32| crate::vocabulary::f(section, key, default);
    let boolean = |key: &str, default: bool| crate::vocabulary::boolean(section, key, default);
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
            "solver_iterations",
            Value::Num(p.num_solver_iterations as f64),
        ),
        (
            "internal_iterations",
            Value::Num(p.num_internal_pgs_iterations as f64),
        ),
        (
            "stabilization_iterations",
            Value::Num(p.num_internal_stabilization_iterations as f64),
        ),
        ("ccd_substeps", Value::Num(p.max_ccd_substeps as f64)),
        ("min_ccd_dt", Value::Num(f64::from(p.min_ccd_dt))),
        ("length_unit", Value::Num(f64::from(p.length_unit))),
        ("warmstart", Value::Num(f64::from(p.warmstart_coefficient))),
        ("warmstart_joints", Value::Bool(p.warmstart_joints)),
        ("contact_clustering", Value::Bool(p.contact_clustering)),
        ("contact_recycling", Value::Bool(p.contact_recycling)),
        (
            "contact_frequency",
            Value::Num(f64::from(p.contact_softness.natural_frequency)),
        ),
        (
            "contact_damping",
            Value::Num(f64::from(p.contact_softness.damping_ratio)),
        ),
    ])
}

pub(crate) fn install_tuning_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_tuning", &[], "(opts: table)", "Change how the solver behaves in both worlds: `solver_iterations`, `length_unit`, `ccd_substeps`, contact softness and the rest. Every value here changes results, so a recording only replays against the same numbers — prefer `[physics]` in project.toml."),
        ("tuning", &[], "()", "The solver settings both worlds are running with."),
        ("quarantined", &[], "()", "The nodes rapier disabled this step because their position or velocity stopped being a number. Empty is the normal answer."),
        ("counters", &[], "()", "What the last step spent its time on. Zeroes unless the engine was built with rapier's profiler."),
        ("set_threads", &[], "(count: int)", "How many threads rapier's solver may use. Only a build with the `parallel` feature has any; on a serial build this says so and changes nothing."),
        ("threads", &[], "()", "How many threads the solver is using; 1 on a serial build."),
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
    m.function("set_threads", |_eng: &Engine, count: i64| {
        set_threads(count.max(1) as usize)
    });
    m.function("threads", |_eng: &Engine, ()| {
        Ok(i64::try_from(threads()).unwrap_or(i64::MAX))
    });
    m.function("counters", |eng: &Engine, ()| {
        let state = eng.resource::<PhysicsState>();
        let state = state.borrow();
        let counters = &state.world.physics_pipeline.counters;
        Ok(map([
            ("step_ms", Value::Num(counters.step_time_ms())),
            (
                "broad_phase_ms",
                Value::Num(counters.cd.broad_phase_time.time_ms()),
            ),
            (
                "narrow_phase_ms",
                Value::Num(counters.cd.narrow_phase_time.time_ms()),
            ),
            (
                "contact_pairs",
                Value::Num(counters.cd.ncontact_pairs as f64),
            ),
        ]))
    });
}

/// The thread count rapier's parallel work runs on.
///
/// With `unsync-callbacks` on — which is what lets a hook call a script —
/// rapier's own per-pipeline pool is compiled out, so the pool is rayon's
/// global one. It can be sized once per process, which is why setting it twice
/// is not an error but does nothing the second time.
#[cfg(feature = "parallel")]
fn set_threads(count: usize) -> Result<()> {
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(count.max(1))
        .build_global();
    Ok(())
}

#[cfg(not(feature = "parallel"))]
fn set_threads(_count: usize) -> Result<()> {
    Err(anyhow::anyhow!(
        "this build steps physics on one thread; build with the `parallel` feature for more"
    ))
}

#[cfg(feature = "parallel")]
fn threads() -> usize {
    rayon::current_num_threads()
}

#[cfg(not(feature = "parallel"))]
const fn threads() -> usize {
    1
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

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

use crate::vocabulary::{Opts, keys as k, map, words as w};
use crate::{PhysicsState2d, PhysicsState3d};

mod soft;

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
        p.min_ccd_dt = f(k::MIN_CCD_SECONDS, p.min_ccd_dt);
        // The one knob a 2D game in pixels cannot do without: every tolerance
        // in the solver is scaled by it, and at 64 pixels per metre the
        // defaults are sixty-four times too loose.
        p.length_unit = f(k::LENGTH_UNIT, p.length_unit).max(1.0e-6);
        p.warmstart_coefficient = f(k::WARMSTART, p.warmstart_coefficient);
        p.warmstart_joints = boolean(k::WARMSTART_JOINTS, p.warmstart_joints);
        p.contact_clustering = boolean(k::CONTACT_CLUSTERING, p.contact_clustering);
        p.contact_recycling = boolean(k::CONTACT_RECYCLING, p.contact_recycling);
        p.normalized_contact_recycle_distance = f(
            k::CONTACT_RECYCLE_DISTANCE,
            p.normalized_contact_recycle_distance,
        )
        .max(0.0);
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
        p.contact_softness.natural_frequency = f(
            k::CONTACT_FREQUENCY_HZ,
            p.contact_softness.natural_frequency,
        );
        p.contact_softness.damping_ratio = f(k::CONTACT_DAMPING, p.contact_softness.damping_ratio);
        p.static_contact_softness.natural_frequency = f(
            k::STATIC_CONTACT_FREQUENCY_HZ,
            p.static_contact_softness.natural_frequency,
        );
        p.static_contact_softness.damping_ratio = f(
            k::STATIC_CONTACT_DAMPING,
            p.static_contact_softness.damping_ratio,
        );
    }};
}

/// What one world's profiler counted in its last step. Turns the profiler on,
/// so the numbers arrive from the step after the first call.
macro_rules! counters_value {
    ($state:expr_2021) => {{
        let mut state = $state;
        let pipeline = &mut state.world.physics_pipeline;
        pipeline.counters.enable();
        let c = &pipeline.counters;
        let (stages, cd, solver, ccd) = (&c.stages, &c.cd, &c.solver, &c.ccd);
        let count = |n: usize| Value::Num(n as f64);
        map([
            (k::STEP_MS, Value::Num(c.step_time_ms())),
            (k::UPDATE_MS, Value::Num(stages.update_time.time_ms())),
            (
                k::USER_CHANGES_MS,
                Value::Num(stages.user_changes.time_ms()),
            ),
            (
                k::COLLISION_DETECTION_MS,
                Value::Num(stages.collision_detection_time.time_ms()),
            ),
            (
                k::ISLAND_CONSTRUCTION_MS,
                Value::Num(stages.island_construction_time.time_ms()),
            ),
            (
                k::ISLAND_CONSTRAINTS_MS,
                Value::Num(stages.island_constraints_collection_time.time_ms()),
            ),
            (k::SOLVER_MS, Value::Num(stages.solver_time.time_ms())),
            (k::CCD_MS, Value::Num(stages.ccd_time.time_ms())),
            (k::BROAD_PHASE_MS, Value::Num(cd.broad_phase_time.time_ms())),
            (
                k::FINAL_BROAD_PHASE_MS,
                Value::Num(cd.final_broad_phase_time.time_ms()),
            ),
            (
                k::NARROW_PHASE_MS,
                Value::Num(cd.narrow_phase_time.time_ms()),
            ),
            (k::CONTACT_PAIR_COUNT, count(cd.ncontact_pairs)),
            (k::CONSTRAINT_COUNT, count(solver.nconstraints)),
            (k::CONTACT_COUNT, count(solver.ncontacts)),
            (
                k::VELOCITY_RESOLUTION_MS,
                Value::Num(solver.velocity_resolution_time.time_ms()),
            ),
            (
                k::VELOCITY_ASSEMBLY_MS,
                Value::Num(solver.velocity_assembly_time.time_ms()),
            ),
            (
                k::VELOCITY_ASSEMBLY_BODIES_MS,
                Value::Num(solver.velocity_assembly_time_solver_bodies.time_ms()),
            ),
            (
                k::VELOCITY_ASSEMBLY_CONSTRAINTS_MS,
                Value::Num(solver.velocity_assembly_time_constraints_init.time_ms()),
            ),
            (
                k::VELOCITY_UPDATE_MS,
                Value::Num(solver.velocity_update_time.time_ms()),
            ),
            (
                k::VELOCITY_WRITEBACK_MS,
                Value::Num(solver.velocity_writeback_time.time_ms()),
            ),
            (k::CCD_SUBSTEP_COUNT, count(ccd.num_substeps)),
            (
                k::CCD_TIME_OF_IMPACT_MS,
                Value::Num(ccd.toi_computation_time.time_ms()),
            ),
            (k::CCD_SOLVER_MS, Value::Num(ccd.solver_time.time_ms())),
            (
                k::CCD_BROAD_PHASE_MS,
                Value::Num(ccd.broad_phase_time.time_ms()),
            ),
            (
                k::CCD_NARROW_PHASE_MS,
                Value::Num(ccd.narrow_phase_time.time_ms()),
            ),
        ])
    }};
}

/// The nodes behind what one world quarantined in its last step: its bodies,
/// its colliders and its soft bodies, in that order.
macro_rules! quarantined_nodes {
    ($state:expr_2021) => {{
        let state = $state;
        let quarantine = state.world.quarantine();
        let bodies: Vec<Entity> = quarantine
            .bodies()
            .iter()
            .filter_map(|handle| {
                let found = state.bodies.iter().find(|(_, h)| *h == handle);
                found.map(|(entity, _)| *entity)
            })
            .collect();
        let colliders: Vec<Entity> = quarantine
            .colliders()
            .iter()
            .filter_map(|handle| {
                let collider = state.world.colliders.get(*handle)?;
                Entity::from_bits(collider.user_data as u64)
            })
            .collect();
        let soft_bodies: Vec<Entity> = quarantine
            .soft_bodies()
            .iter()
            .filter_map(|handle| {
                let body = state.world.soft_bodies.get(*handle)?;
                Entity::from_bits(body.user_data as u64)
            })
            .collect();
        [bodies, colliders, soft_bodies]
    }};
}

/// One world's quarantine as `#{ bodies, colliders, soft_bodies }`, each a
/// list of nodes in stable-id order.
fn quarantine_value(
    eng: &Engine,
    [bodies, colliders, soft_bodies]: [Vec<Entity>; 3],
) -> anyhow::Result<Value> {
    let world = eng.world();
    let list = |mut nodes: Vec<Entity>| -> anyhow::Result<Value> {
        nodes.retain(|e| world.contains(*e));
        balaur_core::ids::sort_by_id(&world, &mut nodes)?;
        nodes.dedup();
        Ok(Value::List(
            nodes
                .into_iter()
                .map(|e| Value::Node(e.to_bits().get()))
                .collect(),
        ))
    };
    Ok(map([
        (k::BODIES, list(bodies)?),
        (k::COLLIDERS, list(colliders)?),
        (k::SOFT_BODIES, list(soft_bodies)?),
    ]))
}

/// Applied once, after the project has loaded, from `[physics]` in
/// `project.toml`. A game with no such section keeps rapier's defaults.
pub(crate) fn build(reg: &mut Registry<'_>) {
    declare_settings(reg.engine());
    reg.insert_resource(SolverThreads::default());
    reg.insert_resource(ManifestTuning {
        applied: false,
        recorded: false,
    });
    reg.add_system(Stage::First, manifest_tuning_system);
    reg.add_replay_setup(WORLD_SETUP, capture_world, restore_world);
}

/// The `[physics]` rows each world reads from its own `[physics.2d]` or
/// `[physics.3d]` table before the shared one; `friction_model` is 3D's alone.
const WORLD_KEYS: &[&str] = &[
    k::SOLVER_ITERATIONS,
    k::INTERNAL_ITERATIONS,
    k::STABILIZATION_ITERATIONS,
    k::CCD_SUBSTEPS,
    k::MIN_CCD_SECONDS,
    k::LENGTH_UNIT,
    k::WARMSTART,
    k::WARMSTART_JOINTS,
    k::CONTACT_CLUSTERING,
    k::CONTACT_RECYCLING,
    k::CONTACT_RECYCLE_DISTANCE,
    k::FRICTION_IN_BIAS_PASS,
    k::ALLOWED_LINEAR_ERROR,
    k::MAX_CORRECTIVE_VELOCITY,
    k::PREDICTION_DISTANCE,
    k::MAX_LINEAR_VELOCITY,
    k::CONTACT_FREQUENCY_HZ,
    k::CONTACT_DAMPING,
    k::STATIC_CONTACT_FREQUENCY_HZ,
    k::STATIC_CONTACT_DAMPING,
    k::SOFT_RESWEEP_STRAIN,
    k::SOFT_MAX_EXTRA_SUBSTEPS,
    k::SOFT_CONTACT_STIFFENING,
    k::SOFT_LINEAR_TOLERANCE,
    k::SOFT_MAX_LINEAR_ITERATIONS,
    k::SOFT_MAX_DENSE_DOFS,
];

/// The soft-body rows, `[physics.soft_recovery]`, and each world's own copy
/// of every row it reads over the shared table: a key there wins for that
/// world, and a key in `[physics]` alone reaches both.
fn declare_settings(eng: &Engine) {
    use balaur_core::settings::{Scope, SettingDef};
    let define = |prefix: &str, schema: &str| {
        balaur_core::settings::define_group(
            eng,
            prefix,
            Scope::Project,
            &balaur_core::ComponentDef::parse_schema("settings.physics", schema),
        );
    };
    define("physics", &soft::schema());
    define(
        &format!("physics/{}", k::SOFT_RECOVERY),
        &soft::recovery_schema(),
    );
    let recovery_prefix = format!("physics/{}/", k::SOFT_RECOVERY);
    let shared: Vec<SettingDef> = balaur_core::settings::all(eng)
        .borrow()
        .0
        .iter()
        .filter(|def| {
            let key = def.path.strip_prefix("physics/").unwrap_or_default();
            WORLD_KEYS.contains(&key)
                || key == k::FRICTION_MODEL
                || def.path.starts_with(&recovery_prefix)
        })
        .cloned()
        .collect();
    for world in [k::WORLD_3D, k::WORLD_2D] {
        for def in &shared {
            let key = def.path.strip_prefix("physics/").unwrap_or_default();
            if key == k::FRICTION_MODEL && world == k::WORLD_2D {
                continue;
            }
            balaur_core::settings::define(
                eng,
                SettingDef {
                    path: format!("physics/{world}/{key}"),
                    ..def.clone()
                },
            );
        }
    }
}

/// What `set_tuning` asks of one world: its own `3d` or `2d` table's keys,
/// else the shared ones, the recovery rows under `soft_recovery` in each.
struct World<'a> {
    shared: &'a Opts<'a>,
    own: Opts<'a>,
}

impl<'a> World<'a> {
    fn of(shared: &'a Opts<'a>, world: &str) -> Self {
        Self {
            shared,
            own: Opts(shared.get(world)),
        }
    }

    fn f32(&self, key: &str, default: f32) -> f32 {
        self.own.f32(key, self.shared.f32(key, default))
    }

    fn boolean(&self, key: &str, default: bool) -> bool {
        self.own.boolean(key, self.shared.boolean(key, default))
    }

    fn text(&self, key: &str) -> Option<String> {
        self.own
            .text(key)
            .or_else(|| self.shared.text(key))
            .map(str::to_string)
    }

    fn recovery(&self) -> (Opts<'a>, Opts<'a>) {
        (
            Opts(self.own.get(k::SOFT_RECOVERY)),
            Opts(self.shared.get(k::SOFT_RECOVERY)),
        )
    }

    fn recovery_f32(&self, key: &str, default: f32) -> f32 {
        let (own, shared) = self.recovery();
        own.f32(key, shared.f32(key, default))
    }

    fn recovery_boolean(&self, key: &str, default: bool) -> bool {
        let (own, shared) = self.recovery();
        own.boolean(key, shared.boolean(key, default))
    }

    fn recovery_text(&self, key: &str) -> Option<String> {
        let (own, shared) = self.recovery();
        own.text(key)
            .or_else(|| shared.text(key))
            .map(str::to_string)
    }
}

/// One world's `[physics]` key: its own table's, else the shared one's.
fn world_setting(eng: &Engine, world: &str, key: &str) -> Option<toml::Value> {
    setting(eng, &format!("{world}/{key}")).or_else(|| setting(eng, key))
}

/// Whether the manifest's `[physics]` table has been read yet.
///
/// The plugin is built before the project is loaded, so the table cannot be
/// read at build time; this is the flag that makes the first tick do it.
struct ManifestTuning {
    applied: bool,
    /// A replay put a recording's own tuning back, which the manifest of the
    /// machine replaying it does not overwrite.
    recorded: bool,
}

/// The recording header's name for both worlds' tuning and gravity.
const WORLD_SETUP: &str = "physics_world";

/// Both worlds' tuning and gravity as the session starts, the manifest's
/// already written so a recording started before the first tick holds it.
fn capture_world(eng: &Engine) -> serde_json::Value {
    let pending = !eng.resource::<ManifestTuning>().borrow().applied
        && eng
            .try_resource::<balaur_core::project::ProjectManifest>()
            .is_some();
    if pending {
        write_tuning_from_settings(eng);
    }
    let gravity_3d = eng
        .resource::<PhysicsState3d>()
        .borrow()
        .world
        .gravity
        .to_array();
    let gravity_2d = eng
        .resource::<PhysicsState2d>()
        .borrow()
        .world
        .gravity
        .to_array();
    serde_json::json!({
        "tuning": balaur_core::engine_api::to_json(&tuning(eng)).unwrap_or_default(),
        "gravity_3d": gravity_3d,
        "gravity_2d": gravity_2d,
    })
}

fn restore_world(eng: &Engine, value: &serde_json::Value) {
    if let Some(tuning) = value
        .get("tuning")
        .and_then(|json| balaur_core::engine_api::from_json(json).ok())
    {
        eng.resource::<ManifestTuning>().borrow_mut().recorded = true;
        set_tuning(eng, &tuning);
    }
    let vector = |key: &str| -> Option<Vec<f32>> {
        value
            .get(key)?
            .as_array()?
            .iter()
            .map(|n| n.as_f64().map(|n| n as f32))
            .collect()
    };
    if let Some([x, y, z]) = vector("gravity_3d")
        .as_deref()
        .and_then(|v| <[f32; 3]>::try_from(v).ok())
    {
        eng.resource::<PhysicsState3d>().borrow_mut().world.gravity = crate::scalar::v3(x, y, z);
    }
    if let Some([x, y]) = vector("gravity_2d")
        .as_deref()
        .and_then(|v| <[f32; 2]>::try_from(v).ok())
    {
        eng.resource::<PhysicsState2d>().borrow_mut().world.gravity = crate::scalar::v2(x, y);
    }
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
        if !flag.borrow().recorded {
            write_tuning_from_settings(eng);
        }
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
    let Some(count) = setting(eng, k::THREADS)
        .as_ref()
        .and_then(toml::Value::as_integer)
    else {
        return;
    };
    if eng.resource::<SolverThreads>().borrow().asked {
        return;
    }
    want_threads(eng, count.max(1) as usize, false);
}

/// The `[physics]` settings, onto both worlds, each reading its own
/// `[physics.3d]` or `[physics.2d]` table first.
fn write_tuning_from_settings(eng: &Engine) {
    let numbers = |key: &str| {
        setting(eng, key).and_then(|value| {
            let list = value
                .as_array()?
                .iter()
                .map(balaur_core::components::as_f64);
            list.map(|n| n.map(|n| crate::scalar::real(n as f32)))
                .collect::<Option<Vec<_>>>()
        })
    };
    let readers = |world: &'static str| {
        let f = move |key: &str, default: f32| {
            world_setting(eng, world, key)
                .as_ref()
                .and_then(balaur_core::components::as_f64)
                .map_or(default, |n| n as f32)
        };
        let boolean = move |key: &str, default: bool| {
            world_setting(eng, world, key)
                .as_ref()
                .and_then(toml::Value::as_bool)
                .unwrap_or(default)
        };
        let word = move |key: &str| {
            world_setting(eng, world, key).and_then(|value| value.as_str().map(str::to_string))
        };
        (f, boolean, word)
    };
    let recovery = |key: &str| format!("{}/{key}", k::SOFT_RECOVERY);
    {
        let (f, boolean, word) = readers(k::WORLD_3D);
        let state = eng.resource::<PhysicsState3d>();
        let mut state = state.borrow_mut();
        let p = &mut state.world.integration_parameters;
        write_parameters!(&mut *p, &f, &boolean);
        soft::write_soft!(
            rapier3d,
            p,
            f,
            |key: &str, default: f32| f(&recovery(key), default),
            |key: &str, default: bool| boolean(&recovery(key), default),
            |key: &str| word(&recovery(key))
        );
        write_friction_model(p, word(k::FRICTION_MODEL).as_deref());
        // The value `physics3d.set_gravity` writes, under its file spelling.
        if let Some([x, y, z]) = numbers(k::GRAVITY_3D).as_deref() {
            state.world.gravity = crate::rapier3d::math::Vector::new(*x, *y, *z);
        }
    }
    let (f, boolean, word) = readers(k::WORLD_2D);
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    let p = &mut state.world.integration_parameters;
    write_parameters!(&mut *p, &f, &boolean);
    soft::write_soft!(
        rapier2d,
        p,
        f,
        |key: &str, default: f32| f(&recovery(key), default),
        |key: &str, default: bool| boolean(&recovery(key), default),
        |key: &str| word(&recovery(key))
    );
    if let Some([x, y]) = numbers(k::GRAVITY_2D).as_deref() {
        state.world.gravity = crate::rapier2d::math::Vector::new(*x, *y);
    }
}

/// `friction_model`, which only the 3D solver has; an unknown word changes
/// nothing.
fn write_friction_model(p: &mut IntegrationParameters, word: Option<&str>) {
    use crate::rapier3d::dynamics::FrictionModel;
    match word {
        Some(w::PER_CONTACT) => p.friction_model = FrictionModel::Coulomb,
        Some(w::SIMPLIFIED) => p.friction_model = FrictionModel::Simplified,
        _ => {}
    }
}

fn friction_model_name(p: &IntegrationParameters) -> &'static str {
    match p.friction_model {
        crate::rapier3d::dynamics::FrictionModel::Coulomb => w::PER_CONTACT,
        crate::rapier3d::dynamics::FrictionModel::Simplified => w::SIMPLIFIED,
    }
}

/// One world's parameters as the rows the reader every setter owes (N8)
/// answers.
macro_rules! tuning_rows {
    ($p:expr_2021) => {{
        let p = $p;
        vec![
            (
                k::SOLVER_ITERATIONS,
                Value::Int(i64::try_from(p.num_solver_iterations).unwrap_or(i64::MAX)),
            ),
            (
                k::INTERNAL_ITERATIONS,
                Value::Int(i64::try_from(p.num_internal_pgs_iterations).unwrap_or(i64::MAX)),
            ),
            (
                k::STABILIZATION_ITERATIONS,
                Value::Int(
                    i64::try_from(p.num_internal_stabilization_iterations).unwrap_or(i64::MAX),
                ),
            ),
            (
                k::CCD_SUBSTEPS,
                Value::Int(i64::try_from(p.max_ccd_substeps).unwrap_or(i64::MAX)),
            ),
            (k::MIN_CCD_SECONDS, Value::Num(f64::from(p.min_ccd_dt))),
            (k::LENGTH_UNIT, Value::Num(f64::from(p.length_unit))),
            (k::WARMSTART, Value::Num(f64::from(p.warmstart_coefficient))),
            (k::WARMSTART_JOINTS, Value::Bool(p.warmstart_joints)),
            (k::CONTACT_CLUSTERING, Value::Bool(p.contact_clustering)),
            (k::CONTACT_RECYCLING, Value::Bool(p.contact_recycling)),
            (
                k::CONTACT_RECYCLE_DISTANCE,
                Value::Num(f64::from(p.normalized_contact_recycle_distance)),
            ),
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
                k::CONTACT_FREQUENCY_HZ,
                Value::Num(f64::from(p.contact_softness.natural_frequency)),
            ),
            (
                k::CONTACT_DAMPING,
                Value::Num(f64::from(p.contact_softness.damping_ratio)),
            ),
            (
                k::STATIC_CONTACT_FREQUENCY_HZ,
                Value::Num(f64::from(p.static_contact_softness.natural_frequency)),
            ),
            (
                k::STATIC_CONTACT_DAMPING,
                Value::Num(f64::from(p.static_contact_softness.damping_ratio)),
            ),
        ]
    }};
}

fn tuning_rows(p: &IntegrationParameters) -> Vec<(&'static str, Value)> {
    tuning_rows!(p)
}

fn tuning_rows_2d(
    p: &crate::rapier2d::dynamics::IntegrationParameters,
) -> Vec<(&'static str, Value)> {
    tuning_rows!(p)
}

/// What `physics.set_tuning` writes: the shared keys onto both worlds, each
/// world's own table over them.
fn set_tuning(eng: &Engine, opts: &Value) {
    let shared = Opts(Some(opts));
    {
        let own = World::of(&shared, k::WORLD_3D);
        let state = eng.resource::<PhysicsState3d>();
        let mut state = state.borrow_mut();
        let p = &mut state.world.integration_parameters;
        write_parameters!(
            &mut *p,
            &|key: &str, d: f32| own.f32(key, d),
            &|key: &str, d: bool| own.boolean(key, d)
        );
        soft::write_soft!(
            rapier3d,
            p,
            |key: &str, d: f32| own.f32(key, d),
            |key: &str, d: f32| own.recovery_f32(key, d),
            |key: &str, d: bool| own.recovery_boolean(key, d),
            |key: &str| own.recovery_text(key)
        );
        write_friction_model(p, own.text(k::FRICTION_MODEL).as_deref());
    }
    let own = World::of(&shared, k::WORLD_2D);
    let state = eng.resource::<PhysicsState2d>();
    let mut state = state.borrow_mut();
    let p = &mut state.world.integration_parameters;
    write_parameters!(
        &mut *p,
        &|key: &str, d: f32| own.f32(key, d),
        &|key: &str, d: bool| own.boolean(key, d)
    );
    soft::write_soft!(
        rapier2d,
        p,
        |key: &str, d: f32| own.f32(key, d),
        |key: &str, d: f32| own.recovery_f32(key, d),
        |key: &str, d: bool| own.recovery_boolean(key, d),
        |key: &str| own.recovery_text(key)
    );
}

/// What `physics.tuning` reads back: every key, per world.
fn tuning(eng: &Engine) -> Value {
    let dim3 = {
        let state = eng.resource::<PhysicsState3d>();
        let state = state.borrow();
        let p = &state.world.integration_parameters;
        let mut rows = tuning_rows(p);
        rows.push((
            k::FRICTION_MODEL,
            Value::Str(friction_model_name(p).to_string()),
        ));
        rows.extend(soft::soft_value!(rapier3d, p));
        rows
    };
    let state = eng.resource::<PhysicsState2d>();
    let state = state.borrow();
    let p = &state.world.integration_parameters;
    let mut dim2 = tuning_rows_2d(p);
    dim2.extend(soft::soft_value!(rapier2d, p));
    let table = |rows: Vec<(&str, Value)>| {
        Value::Map(
            rows.into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    };
    map([(k::WORLD_3D, table(dim3)), (k::WORLD_2D, table(dim2))])
}

pub(crate) fn install_tuning_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_tuning", &[], "(opts: table)", "Change how the solver behaves: `solver_iterations`, `length_unit`, `ccd_substeps`, contact softness, the `soft_*` rows and a `soft_recovery` table, the keys `[physics]` takes. A key at the top reaches both worlds; a `3d` or `2d` table holds keys for that world alone, which win over the top. Every value here changes results, so a recording only replays against the same numbers: prefer `[physics]`, `[physics.3d]` and `[physics.2d]` in project.toml."),
        ("tuning", &[], "()", "The solver settings each world is running with, as `#{ \"3d\", \"2d\" }`, each holding every key `set_tuning` takes, `soft_recovery` included."),
        ("quarantined", &[], "()", "What rapier disabled in each world's last step because a position, velocity or shape stopped being a number, as `#{ physics3d, physics2d }`, each `#{ bodies, colliders, soft_bodies }` lists of nodes. Empty lists are the normal answer."),
        ("counters", &[], "()", "What each world's last step spent its time on, as `#{ physics3d, physics2d }`, each with every timer rapier keeps, in milliseconds (`step_ms`; the stages `update_ms`, `user_changes_ms`, `collision_detection_ms`, `island_construction_ms`, `island_constraints_ms`, `solver_ms`, `ccd_ms`; collision detection's `broad_phase_ms`, `final_broad_phase_ms`, `narrow_phase_ms`; the solver's `velocity_resolution_ms`, `velocity_assembly_ms`, `velocity_assembly_bodies_ms`, `velocity_assembly_constraints_ms`, `velocity_update_ms`, `velocity_writeback_ms`; continuous collision's `ccd_time_of_impact_ms`, `ccd_solver_ms`, `ccd_broad_phase_ms`, `ccd_narrow_phase_ms`) and every count (`contact_pair_count`, `constraint_count`, `contact_count`, `ccd_substep_count`). The first call turns rapier's profilers on, so the numbers arrive from the step after it."),
        ("set_threads", &[], "(count: int)", "How many threads the solver may use, from a script's `init`: rayon's pool is built once, before the first step, and a call after that says so and changes nothing. `[physics] threads` in `project.toml` does the same and outranks nothing -- an `init` that asks wins. The default is one less than the machine reports, capped at eight."),
        ("threads", &[], "()", "How many threads the solver is using. One in a browser, unless the page is the threaded template and called `initThreadPool`."),
    ]);
    m.function("set_tuning", |eng: &Engine, opts: Value| {
        set_tuning(eng, &opts);
        Ok(())
    });
    m.function("tuning", |eng: &Engine, ()| Ok(tuning(eng)));
    // Rapier disables a body whose pose or velocity goes non-finite rather
    // than letting the whole world become NaN. Saying which node it was is the
    // difference between a bug report and a mystery.
    m.function("quarantined", |eng: &Engine, ()| {
        let (state3, state2) = (
            eng.resource::<PhysicsState3d>(),
            eng.resource::<PhysicsState2d>(),
        );
        let dim3 = quarantined_nodes!(&*state3.borrow());
        let dim2 = quarantined_nodes!(&*state2.borrow());
        Ok(map([
            (k::PHYSICS_3D, quarantine_value(eng, dim3)?),
            (k::PHYSICS_2D, quarantine_value(eng, dim2)?),
        ]))
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
        let (state3, state2) = (
            eng.resource::<PhysicsState3d>(),
            eng.resource::<PhysicsState2d>(),
        );
        let dim3 = counters_value!(state3.borrow_mut());
        let dim2 = counters_value!(state2.borrow_mut());
        Ok(map([(k::PHYSICS_3D, dim3), (k::PHYSICS_2D, dim2)]))
    });
}

/// Report anything the 3D world had to quarantine, once per step, so a game
/// that never calls `physics.quarantined()` still learns about it.
pub(crate) fn warn_about_quarantine(eng: &Engine) {
    let state = eng.resource::<PhysicsState3d>();
    let lists = quarantined_nodes!(&*state.borrow());
    warn_about(eng, lists);
}

/// The same, after the 2D world's step.
pub(crate) fn warn_about_quarantine_2d(eng: &Engine) {
    let state = eng.resource::<PhysicsState2d>();
    let lists = quarantined_nodes!(&*state.borrow());
    warn_about(eng, lists);
}

fn warn_about(eng: &Engine, [bodies, colliders, soft_bodies]: [Vec<Entity>; 3]) {
    let world = eng.world();
    let label = |entity: &Entity| balaur_core::digest::node_label(&world, *entity);
    for entity in &bodies {
        tracing::warn!(
            node = %label(entity),
            "physics disabled this body: its position or velocity stopped being a number"
        );
    }
    for entity in &colliders {
        tracing::warn!(
            node = %label(entity),
            "physics disabled this collider: its shape or pose stopped being a number"
        );
    }
    for entity in &soft_bodies {
        tracing::warn!(
            node = %label(entity),
            "physics disabled this soft body: a particle's position or velocity stopped being a number"
        );
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

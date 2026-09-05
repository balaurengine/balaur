//! Rapier draws its own world, into the engine's line buffers.
//!
//! Every phase after this one is debugged by eye through it: a collider whose
//! offset is wrong, a joint anchored to the wrong end, a contact that is not
//! where the artwork says it is. Rendering is an observer (ARCHITECTURE.md),
//! and so is this: the lines are produced after the step from state the step
//! already wrote, and nothing here can reach the tick.

use crate::rapier3d::pipeline::{
    DebugRenderBackend, DebugRenderMode, DebugRenderObject, DebugRenderPipeline,
};
use balaur_core::debug_lines::{DebugLineBuffer, DebugLineBuffer2d};
use balaur_core::{Engine, Stage};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, Value};

use crate::vocabulary::Opts;
use crate::{PhysicsState, PhysicsState2d};

/// What the physics debug draw shows. Written by scripts and the editor, read
/// by this module's `draw_system` every frame.
///
/// `mode` is rapier's own flag set, so a mode rapier adds needs a name here
/// and nothing else.
pub struct PhysicsDebugConfig {
    pub enabled: bool,
    pub mode: DebugRenderMode,
}

impl Default for PhysicsDebugConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            // What an author means by "show me the physics": the shapes.
            mode: DebugRenderMode::COLLIDER_SHAPES,
        }
    }
}

/// The pipeline is state (it holds the style), so it lives beside the config
/// rather than being rebuilt every frame.
#[derive(Default)]
pub struct PhysicsDebugState {
    pipeline: DebugRenderPipeline,
    pipeline_2d: crate::rapier2d::pipeline::DebugRenderPipeline,
}

/// The mode flags a script can name, in the vocabulary the engine uses for
/// them rather than rapier's constant names (N14).
pub const DEBUG_MODES: &[(&str, DebugRenderMode)] = &[
    ("colliders", DebugRenderMode::COLLIDER_SHAPES),
    ("aabbs", DebugRenderMode::COLLIDER_AABBS),
    ("axes", DebugRenderMode::RIGID_BODY_AXES),
    ("joints", DebugRenderMode::JOINTS),
    ("contacts", DebugRenderMode::CONTACTS),
    ("solver_contacts", DebugRenderMode::SOLVER_CONTACTS),
];

pub(crate) fn build(reg: &mut Registry<'_>) {
    reg.insert_resource(PhysicsDebugConfig::default());
    reg.insert_resource(PhysicsDebugState::default());
    // Before Stage::Render, which is where a windowed backend draws the
    // buffer and a headless one clears it.
    reg.add_system(Stage::SceneSync, draw_system);
}

/// Collects rapier's lines into a `DebugLineBuffer`.
struct Lines3d<'a> {
    out: &'a mut DebugLineBuffer,
}

impl DebugRenderBackend for Lines3d<'_> {
    fn draw_line(
        &mut self,
        _object: DebugRenderObject<'_>,
        a: crate::rapier3d::math::Vector,
        b: crate::rapier3d::math::Vector,
        color: [f32; 4],
    ) {
        self.out
            .push(crate::scalar::a3(a), crate::scalar::a3(b), hsl_rgb(color));
    }
}

struct Lines2d<'a> {
    out: &'a mut DebugLineBuffer2d,
}

impl crate::rapier2d::pipeline::DebugRenderBackend for Lines2d<'_> {
    fn draw_line(
        &mut self,
        _object: crate::rapier2d::pipeline::DebugRenderObject<'_>,
        a: crate::rapier2d::math::Vector,
        b: crate::rapier2d::math::Vector,
        color: [f32; 4],
    ) {
        self.out
            .push(crate::scalar::a2(a), crate::scalar::a2(b), hsl_rgb(color));
    }
}

/// Rapier hands colours as HSLA; the line buffers hold RGB. Arithmetic only,
/// so it stays on the right side of the platform-float rule even though a
/// debug colour could never reach the tick.
fn hsl_rgb([h, s, l, _a]: [f32; 4]) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = (h / 60.0).rem_euclid(6.0);
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r + m, g + m, b + m]
}

fn draw_system(eng: &Engine, _dt: f32) {
    let config = eng.resource::<PhysicsDebugConfig>();
    let config = config.borrow();
    if !config.enabled || config.mode.is_empty() {
        return;
    }
    let debug = eng.resource::<PhysicsDebugState>();
    let mut debug = debug.borrow_mut();
    let debug = &mut *debug;
    debug.pipeline.mode = config.mode;
    debug.pipeline_2d.mode =
        crate::rapier2d::pipeline::DebugRenderMode::from_bits_truncate(config.mode.bits());
    if let Some(buffer) = eng.try_resource::<DebugLineBuffer>() {
        let state = eng.resource::<PhysicsState>();
        state.borrow().world.debug_render(
            &mut debug.pipeline,
            &mut Lines3d {
                out: &mut buffer.borrow_mut(),
            },
        );
    }
    if let Some(buffer) = eng.try_resource::<DebugLineBuffer2d>() {
        let state = eng.resource::<PhysicsState2d>();
        state.borrow().world.debug_render(
            &mut debug.pipeline_2d,
            &mut Lines2d {
                out: &mut buffer.borrow_mut(),
            },
        );
    }
}

/// `physics.set_debug_draw` / `physics.debug_draw`: world-spanning, so they
/// live in `physics` rather than in either dimension (D5).
pub(crate) fn install_debug_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_debug_draw", &[], "", "Draw the physics world over the scene: `true` for the usual shapes, or a table naming modes (`#{ colliders = true, joints = true }`)."),
        ("debug_draw", &[], "", "What the debug renderer is drawing now, as a table of `enabled` and one flag per mode."),
    ]);
    // Takes `true`/`false` for "the usual thing", or a table naming the modes
    // (`#{ colliders = true, joints = true }`) when a caller wants more than
    // shapes. One entry point rather than a setter per flag.
    m.function("set_debug_draw", |eng: &Engine, value: Value| {
        let config = eng.resource::<PhysicsDebugConfig>();
        let mut config = config.borrow_mut();
        match value {
            Value::Bool(on) => config.enabled = on,
            Value::Map(_) => {
                let opts = Opts(Some(&value));
                let mut mode = DebugRenderMode::empty();
                for (name, flag) in DEBUG_MODES {
                    if opts.boolean(name, false) {
                        mode |= *flag;
                    }
                }
                config.mode = mode;
                config.enabled = opts.boolean("enabled", !mode.is_empty());
            }
            other => {
                return Err(anyhow::anyhow!(
                    "set_debug_draw takes true, false or a table of modes, not {}",
                    other.type_name()
                ));
            }
        }
        Ok(())
    });
    // Reads back what the setter wrote, `enabled` included (N8).
    m.function("debug_draw", |eng: &Engine, ()| {
        let config = eng.resource::<PhysicsDebugConfig>();
        let config = config.borrow();
        let mut out = vec![("enabled".to_string(), Value::Bool(config.enabled))];
        for (name, flag) in DEBUG_MODES {
            out.push((
                (*name).to_string(),
                Value::Bool(config.mode.contains(*flag)),
            ));
        }
        Ok(Value::Map(out))
    });
}

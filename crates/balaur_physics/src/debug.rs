//! Rapier draws its own world, into the engine's line buffers.
//!
//! Every phase after this one is debugged by eye through it: a collider whose
//! offset is wrong, a joint anchored to the wrong end, a contact that is not
//! where the artwork says it is. Rendering is an observer (ARCHITECTURE.md),
//! and so is this: the lines are produced after the step from state the step
//! already wrote, and nothing here can reach the tick.

use crate::rapier3d::pipeline::{
    DebugRenderBackend, DebugRenderMode, DebugRenderObject, DebugRenderPipeline, DebugRenderStyle,
};
use balaur_core::debug_lines::{DebugLineBuffer2d, DebugLineBuffer3d};
use balaur_core::{Engine, Stage};
use balaur_plugin::Registry;
use balaur_script::{Bindings, BindingsExt, Value};

use crate::vocabulary::{Opts, keys as k, words as w};
use crate::{PhysicsState2d, PhysicsState3d};

/// What the physics debug draw shows. Written by scripts and the editor, read
/// by this module's `draw_system` every frame.
///
/// `mode` is rapier's own flag set, so a mode rapier adds needs a name here
/// and nothing else.
pub struct PhysicsDebugConfig {
    pub enabled: bool,
    pub mode: DebugRenderMode,
    /// The nodes whose bodies, colliders, joints, contacts and soft bodies are
    /// drawn; empty draws all of them.
    pub nodes: Vec<u64>,
}

impl Default for PhysicsDebugConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            // What an author means by "show me the physics": the shapes.
            mode: DebugRenderMode::COLLIDER_SHAPES,
            nodes: Vec::new(),
        }
    }
}

/// The pipeline is state (it holds the style), so it lives beside the config
/// rather than being rebuilt every frame.
#[derive(Default)]
pub struct PhysicsDebugState {
    pipeline: DebugRenderPipeline,
    pipeline_2d: crate::rapier2d::pipeline::DebugRenderPipeline,
    /// The settings revision the style was last read at.
    styled_at: Option<u64>,
}

/// The mode flags a script can name, in the vocabulary the engine uses for
/// them rather than rapier's constant names (N14).
pub const DEBUG_MODES: &[(&str, DebugRenderMode)] = &[
    (w::DRAW_COLLIDERS, DebugRenderMode::COLLIDER_SHAPES),
    (w::DRAW_AABBS, DebugRenderMode::COLLIDER_AABBS),
    (w::DRAW_AXES, DebugRenderMode::RIGID_BODY_AXES),
    (w::DRAW_IMPULSE_JOINTS, DebugRenderMode::IMPULSE_JOINTS),
    (w::DRAW_MULTIBODY_JOINTS, DebugRenderMode::MULTIBODY_JOINTS),
    (w::DRAW_CONTACTS, DebugRenderMode::CONTACTS),
    (w::DRAW_SOLVER_CONTACTS, DebugRenderMode::SOLVER_CONTACTS),
    (w::DRAW_SOFT_BODIES, DebugRenderMode::SOFT_BODIES),
    (w::DRAW_PSEUDO_NORMALS, DebugRenderMode::PSEUDO_NORMALS),
    (
        w::DRAW_SOFT_VOLUME_CONTACTS,
        DebugRenderMode::SOFT_VOLUME_CONTACTS,
    ),
    (w::DRAW_SOFT_STRESS, DebugRenderMode::SOFT_BODY_STRESS),
];

/// Where the `[physics.debug]` keys live.
const STYLE_PREFIX: &str = "physics/debug";

/// One `[physics.debug]` key: how it reads rapier's style and writes it back.
enum StyleField {
    Count(
        &'static str,
        fn(&mut DebugRenderStyle) -> &mut u32,
        &'static str,
    ),
    Length(
        &'static str,
        fn(&mut DebugRenderStyle) -> &mut f32,
        &'static str,
    ),
    Color(
        &'static str,
        fn(&mut DebugRenderStyle) -> &mut [f32; 4],
        &'static str,
    ),
    Tint(
        &'static str,
        fn(&mut DebugRenderStyle) -> &mut [f32; 4],
        &'static str,
    ),
}

/// Every `DebugRenderStyle` field, in Balaur's words. A colour is red, green,
/// blue and alpha; a tint multiplies a colour's hue, saturation, lightness
/// and alpha, as rapier does.
const STYLE: &[StyleField] = &[
    StyleField::Count(
        k::SUBDIVISIONS,
        |s| &mut s.subdivisions,
        "How many segments a curved face is drawn with",
    ),
    StyleField::Count(
        k::BORDER_SUBDIVISIONS,
        |s| &mut s.border_subdivisions,
        "How many segments a round border is drawn with",
    ),
    StyleField::Color(
        k::DYNAMIC_COLOR,
        |s| &mut s.collider_dynamic_color,
        "A collider on a dynamic body",
    ),
    StyleField::Color(
        k::STATIC_COLOR,
        |s| &mut s.collider_fixed_color,
        "A collider on a static body",
    ),
    StyleField::Color(
        k::KINEMATIC_COLOR,
        |s| &mut s.collider_kinematic_color,
        "A collider on a kinematic body",
    ),
    StyleField::Color(
        k::STANDALONE_COLOR,
        |s| &mut s.collider_parentless_color,
        "A collider on no body",
    ),
    StyleField::Color(
        k::JOINT_ANCHOR_COLOR,
        |s| &mut s.impulse_joint_anchor_color,
        "The line from a body's centre of mass to its joint's anchor",
    ),
    StyleField::Color(
        k::JOINT_SEPARATION_COLOR,
        |s| &mut s.impulse_joint_separation_color,
        "The line between a joint's two anchors",
    ),
    StyleField::Color(
        k::ARTICULATION_ANCHOR_COLOR,
        |s| &mut s.multibody_joint_anchor_color,
        "The same anchor line for an articulation",
    ),
    StyleField::Color(
        k::ARTICULATION_SEPARATION_COLOR,
        |s| &mut s.multibody_joint_separation_color,
        "The same separation line for an articulation",
    ),
    StyleField::Tint(
        k::SLEEPING_TINT,
        |s| &mut s.sleep_color_multiplier,
        "What a sleeping body's lines are multiplied by, as hue, saturation, lightness and alpha factors",
    ),
    StyleField::Tint(
        k::SLEEP_READY_TINT,
        |s| &mut s.sleep_eligible_color_multiplier,
        "The same for an awake body still enough to sleep",
    ),
    StyleField::Tint(
        k::DISABLED_TINT,
        |s| &mut s.disabled_color_multiplier,
        "The same for a disabled body",
    ),
    StyleField::Length(
        k::AXES_LENGTH,
        |s| &mut s.rigid_body_axes_length,
        "How long a body's drawn axes are",
    ),
    StyleField::Color(
        k::CONTACT_DEPTH_COLOR,
        |s| &mut s.contact_depth_color,
        "The line joining a contact's two points",
    ),
    StyleField::Color(
        k::CONTACT_NORMAL_COLOR,
        |s| &mut s.contact_normal_color,
        "A contact's normal",
    ),
    StyleField::Length(
        k::CONTACT_NORMAL_LENGTH,
        |s| &mut s.contact_normal_length,
        "How long a contact's normal is drawn",
    ),
    StyleField::Color(
        k::SOFT_BODY_COLOR,
        |s| &mut s.soft_body_element_color,
        "A soft body's edges",
    ),
    StyleField::Color(
        k::SOFT_SLACK_COLOR,
        |s| &mut s.soft_body_slack_color,
        "An unloaded soft-body edge, drawn by load under soft_stress",
    ),
    StyleField::Color(
        k::SOFT_LOADED_COLOR,
        |s| &mut s.soft_body_loaded_color,
        "A soft-body edge at its tear threshold, under soft_stress",
    ),
    StyleField::Color(
        k::SOFT_FRAME_COLOR,
        |s| &mut s.soft_body_frame_color,
        "A soft body's region frames",
    ),
    StyleField::Color(
        k::AABB_COLOR,
        |s| &mut s.collider_aabb_color,
        "A collider's bounding box",
    ),
    StyleField::Color(
        k::VERTEX_NORMAL_COLOR,
        |s| &mut s.vertex_pseudo_normal_color,
        "A mesh or polyline vertex's pseudo-normal",
    ),
    StyleField::Color(
        k::EDGE_NORMAL_COLOR,
        |s| &mut s.edge_pseudo_normal_color,
        "A mesh edge's pseudo-normal, 3D only",
    ),
    StyleField::Length(
        k::NORMAL_LENGTH,
        |s| &mut s.pseudo_normal_length,
        "How long a pseudo-normal is drawn",
    ),
    StyleField::Color(
        k::VOLUME_NORMAL_COLOR,
        |s| &mut s.volume_contact_normal_color,
        "A soft body's volume contact normal",
    ),
    StyleField::Color(
        k::VOLUME_GRADIENT_COLOR,
        |s| &mut s.volume_gradient_color,
        "The volume gradient at each particle of a volume contact",
    ),
];

/// The `[physics.debug]` schema, each default rapier's own.
fn style_schema() -> String {
    let mut style = DebugRenderStyle::default();
    let numbers = |values: &[f32]| {
        values
            .iter()
            .map(|n| format!("{n:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    STYLE
        .iter()
        .map(|field| match field {
            StyleField::Count(key, get, help) => format!(
                r#"{key} = {{ type = "int", default = {}, min = 1, max = 256, help = "{help}." }}"#,
                get(&mut style)
            ),
            StyleField::Length(key, get, help) => format!(
                r#"{key} = {{ type = "float", default = {:?}, min = 0.0, help = "{help}, in length units." }}"#,
                get(&mut style)
            ),
            StyleField::Color(key, get, help) => format!(
                r#"{key} = {{ type = "color", default = [{}], help = "{help}." }}"#,
                numbers(&hsl_rgba(*get(&mut style)).map(|c| c.clamp(0.0, 1.0)))
            ),
            StyleField::Tint(key, get, help) => format!(
                r#"{key} = {{ type = "vec4", default = [{}], help = "{help}." }}"#,
                numbers(get(&mut style))
            ),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The style the `[physics.debug]` keys spell, over rapier's defaults.
fn read_style(eng: &Engine) -> DebugRenderStyle {
    let mut style = DebugRenderStyle::default();
    let setting = |key: &str| balaur_core::settings::get(eng, &format!("{STYLE_PREFIX}/{key}"));
    for field in STYLE {
        match field {
            StyleField::Count(key, get, _) => {
                if let Some(n) = setting(key).as_ref().and_then(toml::Value::as_integer) {
                    *get(&mut style) = u32::try_from(n.max(1)).unwrap_or(u32::MAX);
                }
            }
            StyleField::Length(key, get, _) => {
                if let Some(n) = setting(key)
                    .as_ref()
                    .and_then(balaur_core::components::as_f64)
                {
                    *get(&mut style) = crate::scalar::real(n as f32);
                }
            }
            StyleField::Color(key, get, _) => {
                if let Some(rgba) = setting(key).as_ref().and_then(four) {
                    *get(&mut style) = rgba_hsl(rgba);
                }
            }
            StyleField::Tint(key, get, _) => {
                if let Some(factors) = setting(key).as_ref().and_then(four) {
                    *get(&mut style) = factors;
                }
            }
        }
    }
    style
}

/// Four numbers, or a `#rrggbb` / `#rrggbbaa` colour.
fn four(value: &toml::Value) -> Option<[f32; 4]> {
    match value {
        toml::Value::Array(items) => {
            let mut out = [0.0, 0.0, 0.0, 1.0];
            for (slot, item) in out.iter_mut().zip(items) {
                *slot = balaur_core::components::as_f64(item)? as f32;
            }
            (items.len() >= 3).then_some(out)
        }
        toml::Value::String(text) => {
            let hex = text.strip_prefix('#')?;
            let channel = |i: usize| {
                let pair = hex.get(i * 2..i * 2 + 2)?;
                u8::from_str_radix(pair, 16)
                    .ok()
                    .map(|n| f32::from(n) / 255.0)
            };
            let alpha = if hex.len() == 8 { channel(3)? } else { 1.0 };
            (hex.len() == 6 || hex.len() == 8).then_some([
                channel(0)?,
                channel(1)?,
                channel(2)?,
                alpha,
            ])
        }
        _ => None,
    }
}

pub(crate) fn build(reg: &mut Registry<'_>) {
    balaur_core::settings::define_group(
        reg.engine(),
        STYLE_PREFIX,
        balaur_core::settings::Scope::Project,
        &balaur_core::ComponentDef::parse_schema("settings.physics.debug", &style_schema()),
    );
    reg.insert_resource(PhysicsDebugConfig::default());
    reg.insert_resource(PhysicsDebugState::default());
    // Before Stage::Render, which is where a windowed backend draws the
    // buffer and a headless one clears it.
    reg.add_system(Stage::SceneSync, draw_system);
}

/// Whether a node the draw is limited to owns `data`, a body's or a
/// collider's `user_data`.
fn listed(nodes: &[u64], data: u128) -> bool {
    nodes.contains(&(data as u64))
}

/// Collects rapier's lines into a `DebugLineBuffer3d`.
struct Lines3d<'a> {
    out: &'a mut DebugLineBuffer3d,
    nodes: &'a [u64],
    bodies: &'a crate::rapier3d::prelude::RigidBodySet,
}

impl Lines3d<'_> {
    fn body_listed(&self, handle: crate::rapier3d::prelude::RigidBodyHandle) -> bool {
        self.bodies
            .get(handle)
            .is_some_and(|body| listed(self.nodes, body.user_data))
    }

    fn collider_listed(&self, collider: &crate::rapier3d::prelude::Collider) -> bool {
        listed(self.nodes, collider.user_data)
            || collider.parent().is_some_and(|body| self.body_listed(body))
    }
}

impl DebugRenderBackend for Lines3d<'_> {
    fn filter_object(&self, object: DebugRenderObject<'_>) -> bool {
        if self.nodes.is_empty() {
            return true;
        }
        match object {
            DebugRenderObject::RigidBody(_, body) => listed(self.nodes, body.user_data),
            DebugRenderObject::Collider(_, collider)
            | DebugRenderObject::ColliderAabb(_, collider, _) => self.collider_listed(collider),
            DebugRenderObject::ImpulseJoint(_, joint) => {
                self.body_listed(joint.body1()) || self.body_listed(joint.body2())
            }
            DebugRenderObject::MultibodyJoint(_, _, link) => {
                self.body_listed(link.rigid_body_handle())
            }
            DebugRenderObject::ContactPair(_, a, b) => {
                self.collider_listed(a) || self.collider_listed(b)
            }
            DebugRenderObject::SoftBody(_, body) => listed(self.nodes, body.user_data),
        }
    }

    fn draw_line(
        &mut self,
        _object: DebugRenderObject<'_>,
        a: crate::rapier3d::math::Vector,
        b: crate::rapier3d::math::Vector,
        color: [f32; 4],
    ) {
        self.out
            .push(crate::scalar::a3(a), crate::scalar::a3(b), hsl_rgba(color));
    }
}

struct Lines2d<'a> {
    out: &'a mut DebugLineBuffer2d,
    nodes: &'a [u64],
    bodies: &'a crate::rapier2d::prelude::RigidBodySet,
}

impl Lines2d<'_> {
    fn body_listed(&self, handle: crate::rapier2d::prelude::RigidBodyHandle) -> bool {
        self.bodies
            .get(handle)
            .is_some_and(|body| listed(self.nodes, body.user_data))
    }

    fn collider_listed(&self, collider: &crate::rapier2d::prelude::Collider) -> bool {
        listed(self.nodes, collider.user_data)
            || collider.parent().is_some_and(|body| self.body_listed(body))
    }
}

impl crate::rapier2d::pipeline::DebugRenderBackend for Lines2d<'_> {
    fn filter_object(&self, object: crate::rapier2d::pipeline::DebugRenderObject<'_>) -> bool {
        use crate::rapier2d::pipeline::DebugRenderObject as Object;
        if self.nodes.is_empty() {
            return true;
        }
        match object {
            Object::RigidBody(_, body) => listed(self.nodes, body.user_data),
            Object::Collider(_, collider) | Object::ColliderAabb(_, collider, _) => {
                self.collider_listed(collider)
            }
            Object::ImpulseJoint(_, joint) => {
                self.body_listed(joint.body1()) || self.body_listed(joint.body2())
            }
            Object::MultibodyJoint(_, _, link) => self.body_listed(link.rigid_body_handle()),
            Object::ContactPair(_, a, b) => self.collider_listed(a) || self.collider_listed(b),
            Object::SoftBody(_, body) => listed(self.nodes, body.user_data),
        }
    }

    fn draw_line(
        &mut self,
        _object: crate::rapier2d::pipeline::DebugRenderObject<'_>,
        a: crate::rapier2d::math::Vector,
        b: crate::rapier2d::math::Vector,
        color: [f32; 4],
    ) {
        self.out
            .push(crate::scalar::a2(a), crate::scalar::a2(b), hsl_rgba(color));
    }
}

/// Rapier hands colours as HSLA and the line buffers hold RGBA. Arithmetic
/// only, so it stays on the right side of the platform-float rule even though
/// a debug colour could never reach the tick.
fn hsl_rgba([h, s, l, a]: [f32; 4]) -> [f32; 4] {
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
    [r + m, g + m, b + m, a]
}

/// The inverse of [`hsl_rgba`], for a colour a project wrote in RGBA.
fn rgba_hsl([r, g, b, a]: [f32; 4]) -> [f32; 4] {
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = f32::midpoint(max, min);
    let d = max - min;
    if d <= 0.0 {
        return [0.0, 0.0, l, a];
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    #[allow(
        clippy::float_cmp,
        reason = "max is one of the three channels, by value"
    )]
    let sector = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [sector * 60.0, s, l, a]
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
    let revision = balaur_core::settings::revision(eng);
    if debug.styled_at != Some(revision) {
        let style = read_style(eng);
        debug.pipeline.style = style;
        debug.pipeline_2d.style = two_dimensional(&style);
        debug.styled_at = Some(revision);
    }
    debug.pipeline.mode = config.mode;
    debug.pipeline_2d.mode =
        crate::rapier2d::pipeline::DebugRenderMode::from_bits_truncate(config.mode.bits());
    if let Some(buffer) = eng.try_resource::<DebugLineBuffer3d>() {
        let state = eng.resource::<PhysicsState3d>();
        let state = state.borrow();
        state.world.debug_render(
            &mut debug.pipeline,
            &mut Lines3d {
                out: &mut buffer.borrow_mut(),
                nodes: &config.nodes,
                bodies: &state.world.bodies,
            },
        );
    }
    if let Some(buffer) = eng.try_resource::<DebugLineBuffer2d>() {
        let state = eng.resource::<PhysicsState2d>();
        let state = state.borrow();
        state.world.debug_render(
            &mut debug.pipeline_2d,
            &mut Lines2d {
                out: &mut buffer.borrow_mut(),
                nodes: &config.nodes,
                bodies: &state.world.bodies,
            },
        );
    }
}

/// The same style for rapier2d, whose struct is a different type of the same
/// fields.
fn two_dimensional(s: &DebugRenderStyle) -> crate::rapier2d::pipeline::DebugRenderStyle {
    crate::rapier2d::pipeline::DebugRenderStyle {
        subdivisions: s.subdivisions,
        border_subdivisions: s.border_subdivisions,
        collider_dynamic_color: s.collider_dynamic_color,
        collider_fixed_color: s.collider_fixed_color,
        collider_kinematic_color: s.collider_kinematic_color,
        collider_parentless_color: s.collider_parentless_color,
        impulse_joint_anchor_color: s.impulse_joint_anchor_color,
        impulse_joint_separation_color: s.impulse_joint_separation_color,
        multibody_joint_anchor_color: s.multibody_joint_anchor_color,
        multibody_joint_separation_color: s.multibody_joint_separation_color,
        sleep_color_multiplier: s.sleep_color_multiplier,
        sleep_eligible_color_multiplier: s.sleep_eligible_color_multiplier,
        disabled_color_multiplier: s.disabled_color_multiplier,
        rigid_body_axes_length: s.rigid_body_axes_length,
        contact_depth_color: s.contact_depth_color,
        contact_normal_color: s.contact_normal_color,
        contact_normal_length: s.contact_normal_length,
        soft_body_element_color: s.soft_body_element_color,
        soft_body_slack_color: s.soft_body_slack_color,
        soft_body_loaded_color: s.soft_body_loaded_color,
        soft_body_frame_color: s.soft_body_frame_color,
        collider_aabb_color: s.collider_aabb_color,
        vertex_pseudo_normal_color: s.vertex_pseudo_normal_color,
        edge_pseudo_normal_color: s.edge_pseudo_normal_color,
        pseudo_normal_length: s.pseudo_normal_length,
        volume_contact_normal_color: s.volume_contact_normal_color,
        volume_gradient_color: s.volume_gradient_color,
    }
}

/// `physics.set_debug_draw` / `physics.debug_draw`: world-spanning, so they
/// live in `physics` rather than in either dimension (D5).
pub(crate) fn install_debug_api(m: &mut dyn Bindings<Engine>) {
    m.describe(&[
        ("set_debug_draw", &[], "", "Draw the physics world over the scene: `true` for the usual shapes, or a table naming modes (`#{ colliders = true, impulse_joints = true }`): `colliders`, `aabbs`, `axes`, `impulse_joints`, `multibody_joints`, `contacts`, `solver_contacts`, `soft_bodies`, `pseudo_normals`, `soft_volume_contacts`, `soft_stress` (soft-body edges coloured by load), with `nodes`, a list of nodes whose bodies, colliders, joints, contacts and soft bodies alone are drawn. `[physics.debug]` holds the colours and lengths."),
        ("debug_draw", &[], "", "What the debug renderer is drawing now, as a table of `enabled`, one flag per mode and `nodes`."),
    ]);
    // Takes `true`/`false` for "the usual thing", or a table naming the modes
    // when a caller wants more than shapes. One entry point rather than a
    // setter per flag.
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
                config.enabled = opts.boolean(k::ENABLED, !mode.is_empty());
                config.nodes = opts
                    .list(k::NODES)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|item| match item {
                        Value::Node(bits) => Some(*bits),
                        _ => None,
                    })
                    .collect();
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
    // Reads back what the setter wrote, `enabled` and `nodes` included (N8).
    m.function("debug_draw", |eng: &Engine, ()| {
        let config = eng.resource::<PhysicsDebugConfig>();
        let config = config.borrow();
        let mut out = vec![(k::ENABLED.to_string(), Value::Bool(config.enabled))];
        for (name, flag) in DEBUG_MODES {
            out.push((name.to_string(), Value::Bool(config.mode.contains(*flag))));
        }
        let nodes = config.nodes.iter().map(|bits| Value::Node(*bits)).collect();
        out.push((k::NODES.to_string(), Value::List(nodes)));
        Ok(Value::Map(out))
    });
}

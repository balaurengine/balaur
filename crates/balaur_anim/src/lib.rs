//! Animation as a Balaur plugin: clips, a pure sampler, and playback.
//!
//! Seven pieces, in dependency order:
//!
//! 1. [`clip`] — the `animation_clip` asset type, registered through
//!    `App::register_asset_type`. Core never learns what a clip is; a clip is
//!    shared by every node that names it, and immutable once parsed.
//! 2. [`ease`] — twelve transitions in four modes, on `libm`, with Godot's
//!    names and Godot's shapes.
//! 3. [`sampler`] — `(clip, time) -> pose`, pure, so a crossfade composes
//!    samples without the data model changing under them.
//! 4. [`player`] — one `Playback` per node: the playhead, the queue, the
//!    clips a script defined at run time, and the Rust API over them.
//! 5. [`tween`] — a clip generated on the spot from a list of steps, with its
//!    start values read off the node. One sampler, two authoring front-ends;
//!    there is no second interpolation path in this crate.
//! 6. `system` (crate-private) — the fixed-step advance, the pose write, and
//!    delivery of what a step passed over, for players and tweens alike.
//! 7. This module — the `animation` component (the scene key an editor-saved
//!    `[nodes.animation]` writes) and the plugin that wires it all together,
//!    including the `animation` script module.
//!
//! Playback advances on its own 1/60 accumulator, never on `engine.time()`:
//! a variable dt in a simulation path is a different result on every machine.
//! The system runs in `Stage::Update`, after the script tick, so a script's
//! `animation.play()` lands the same frame — and before `Stage::PostUpdate`,
//! so physics reads an animated kinematic body's transform in the frame it
//! was animated.
//!
//! A track drives a transform (`position`, `rotation_euler`, `scale`), a
//! registered component's property (`color/rgba`, `shape/radius`,
//! `widget/x`), or nothing at all — a method track is a list of moments at
//! which to call a script method. Component properties go through the
//! component registry and `balaur_core::components::patch`, which is why this
//! crate animates render, UI and third-party components while depending on
//! none of them.

pub mod bindings;
pub mod clip;
pub mod ease;
mod gizmo;
pub mod machine;
pub mod modifier;
mod modifier_solve;
pub mod player;
pub mod retarget;
pub mod sampler;
mod snapshot;
mod system;
pub mod tween;

/// The component key, as the registry and every `describe` entry spell it.
pub(crate) const COMPONENT: &str = "animation";

/// Every key a component, an asset, a tween or a `play` option table spells,
/// for the schemas, the parsers, the readers and the importer alike.
pub mod keys {
    pub const ACTIVE: &str = "active";
    pub const ADVANCE: &str = "advance";
    pub const ANGLE_LIMIT: &str = "angle_limit";
    pub const AUTOPLAY: &str = "autoplay";
    pub const BONE: &str = "bone";
    pub const BONES: &str = "bones";
    pub const BREAK_LOOP: &str = "break_loop";
    pub const BY: &str = "by";
    pub const CALL: &str = "call";
    pub const CHAIN: &str = "chain";
    pub const CHECK: &str = "check";
    pub const CHECK_NODE: &str = "check_node";
    pub const CONDITION: &str = "condition";
    pub const DAMPING: &str = "damping";
    pub const DELAY: &str = "delay";
    pub const DURATION: &str = "duration";
    pub const EASE: &str = "ease";
    pub const ENABLED: &str = "enabled";
    pub const FADE: &str = "fade";
    pub const FADE_CURVE: &str = "fade_curve";
    pub const FLIP: &str = "flip";
    pub const FROM: &str = "from";
    pub const FROM_START: &str = "from_start";
    pub const GRAVITY: &str = "gravity";
    pub const INTERP: &str = "interp";
    pub const INTERVAL: &str = "interval";
    pub const ITERATIONS: &str = "iterations";
    pub const KEYS: &str = "keys";
    pub const KIND: &str = "kind";
    pub const LAG: &str = "lag";
    pub const LENGTH: &str = "length";
    pub const LIBRARY: &str = "library";
    pub const LOOP: &str = "loop";
    pub const LOOPS: &str = "loops";
    pub const MACHINE: &str = "machine";
    pub const MASS: &str = "mass";
    pub const NAME: &str = "name";
    pub const OFFSET: &str = "offset";
    pub const PARALLEL: &str = "parallel";
    pub const PLAYER: &str = "player";
    pub const PRIORITY: &str = "priority";
    pub const PROFILE: &str = "profile";
    pub const PROPERTY: &str = "property";
    pub const RESET: &str = "reset";
    pub const REST_POSITION: &str = "rest_position";
    pub const REST_ROTATION: &str = "rest_rotation";
    pub const RETARGET: &str = "retarget";
    pub const ROOT: &str = "root";
    pub const SPEED: &str = "speed";
    pub const START: &str = "start";
    pub const STATES: &str = "states";
    pub const STEPS: &str = "steps";
    pub const STIFFNESS: &str = "stiffness";
    pub const SWITCH: &str = "switch";
    pub const T: &str = "t";
    pub const TARGET: &str = "target";
    pub const THEN: &str = "then";
    pub const TO: &str = "to";
    pub const TOLERANCE: &str = "tolerance";
    pub const TRACKS: &str = "tracks";
    pub const TRANSITIONS: &str = "transitions";
    pub const USE_GRAVITY: &str = "use_gravity";
    pub const VALUE: &str = "value";
}

/// The closed sets of words a clip, a machine, a modifier and a script spell,
/// written once so a parser, a schema and a read-back cannot disagree.
pub mod words {
    /// A clip's `loop`.
    pub const NONE: &str = "none";
    pub const LOOP: &str = "loop";
    pub const PINGPONG: &str = "pingpong";

    /// A track's `interp`.
    pub const STEP: &str = "step";
    pub const LINEAR: &str = "linear";
    pub const CUBIC: &str = "cubic";

    /// The transform and appearance properties a track drives by name.
    pub const POSITION: &str = "position";
    pub const ROTATION_EULER: &str = "rotation_euler";
    pub const ROTATION: &str = "rotation";
    pub const SCALE: &str = "scale";
    pub const VISIBLE: &str = "visible";
    pub const TINT: &str = "tint";

    /// A transition's `advance`: never on its own, only by travel, or on its
    /// own too.
    pub const DISABLED: &str = "disabled";
    pub const ENABLED: &str = "enabled";
    pub const AUTO: &str = "auto";

    /// A transition's `switch`: cut now, cut keeping the playhead, or wait
    /// for the clip's end.
    pub const IMMEDIATE: &str = "immediate";
    pub const SYNC: &str = "sync";
    pub const AT_END: &str = "at_end";

    /// The state a transition reaches to stop the machine: Godot's `End`.
    pub const END: &str = "end";

    /// A modifier's `kind`.
    pub const LOOK_AT: &str = "look_at";
    pub const TWO_BONE_IK: &str = "two_bone_ik";
    pub const FABRIK: &str = "fabrik";
    pub const CCDIK: &str = "ccdik";
    pub const JIGGLE: &str = "jiggle";
    pub const FOLLOW: &str = "follow";
    pub const MODIFIER_KINDS: &[&str] = &[LOOK_AT, TWO_BONE_IK, FABRIK, CCDIK, JIGGLE, FOLLOW];
}

use crate::words as w;

/// A clip's `loop` modes, as `animation::LOOP_PINGPONG` and the rest.
pub const LOOP_MODES: &[(&str, &str)] = &[
    ("LOOP_NONE", w::NONE),
    ("LOOP_LOOP", w::LOOP),
    ("LOOP_PINGPONG", w::PINGPONG),
];

/// A track's `interp` modes.
pub const INTERPS: &[(&str, &str)] = &[
    ("INTERP_STEP", w::STEP),
    ("INTERP_LINEAR", w::LINEAR),
    ("INTERP_CUBIC", w::CUBIC),
];

/// The properties a track drives by name; a component's is `component/property`.
pub const PROPERTIES: &[(&str, &str)] = &[
    ("PROPERTY_POSITION", w::POSITION),
    ("PROPERTY_ROTATION_EULER", w::ROTATION_EULER),
    ("PROPERTY_ROTATION", w::ROTATION),
    ("PROPERTY_SCALE", w::SCALE),
    ("PROPERTY_VISIBLE", w::VISIBLE),
    ("PROPERTY_TINT", w::TINT),
    ("PROPERTY_DEFORM", clip::DEFORM),
];

/// A state machine transition's `advance` modes.
pub const ADVANCE_MODES: &[(&str, &str)] = &[
    ("ADVANCE_DISABLED", w::DISABLED),
    ("ADVANCE_ENABLED", w::ENABLED),
    ("ADVANCE_AUTO", w::AUTO),
];

/// A state machine transition's `switch` modes.
pub const SWITCH_MODES: &[(&str, &str)] = &[
    ("SWITCH_IMMEDIATE", w::IMMEDIATE),
    ("SWITCH_SYNC", w::SYNC),
    ("SWITCH_AT_END", w::AT_END),
];

/// The state a transition names to stop its machine.
pub const MACHINE_STATES: &[(&str, &str)] = &[("STATE_END", w::END)];

/// `modifier2d` and `modifier3d` kinds.
pub const MODIFIER_KINDS: &[(&str, &str)] = &[
    ("MODIFIER_LOOK_AT", w::LOOK_AT),
    ("MODIFIER_TWO_BONE_IK", w::TWO_BONE_IK),
    ("MODIFIER_FABRIK", w::FABRIK),
    ("MODIFIER_CCDIK", w::CCDIK),
    ("MODIFIER_JIGGLE", w::JIGGLE),
    ("MODIFIER_FOLLOW", w::FOLLOW),
];

/// The events a player and a machine emit from their node.
pub const EVENTS: &[(&str, &str)] = &[
    ("EVENT_ANIMATION_FINISHED", system::FINISHED_EVENT),
    ("EVENT_STATE_STARTED", machine::STATE_STARTED_EVENT),
    ("EVENT_STATE_FINISHED", machine::STATE_FINISHED_EVENT),
];

/// Every table above, installed on the `animation` module.
pub const CONSTANTS: &[&[(&str, &str)]] = &[
    LOOP_MODES,
    INTERPS,
    PROPERTIES,
    ADVANCE_MODES,
    SWITCH_MODES,
    MACHINE_STATES,
    MODIFIER_KINDS,
    EVENTS,
];

/// Every curve name as its constant: `in_out_sine` is `EASE_IN_OUT_SINE`.
#[must_use]
pub fn ease_constants() -> Vec<(String, &'static str)> {
    ease::names()
        .into_iter()
        .map(|name| (format!("EASE_{}", name.to_ascii_uppercase()), name))
        .collect()
}

use balaur_plugin::Registry;
use std::any::Any;
use std::rc::Rc;

use crate::keys as k;
use anyhow::Result;
use balaur_core::components::ComponentDef;
use balaur_core::hecs::Entity;
use balaur_core::{Engine, Stage};

pub use crate::bindings::install_animation_api;
pub use crate::machine::{STATE_FINISHED_EVENT, STATE_STARTED_EVENT};
pub use crate::player::{
    AnimationState, CLIP_ASSET_TYPE, Playback, current, define, is_playing, just_finished, pause,
    play, play_from, queue, resume, seek, set_retarget, set_speed, stop, time,
};
pub use crate::retarget::{BONE_MAP_ASSET_TYPE, BoneMap, PROFILE_ASSET_TYPE, SkeletonProfile};
pub use crate::system::FINISHED_EVENT;
pub use crate::tween::{Tween, TweenId};

pub struct AnimationPlugin {
    manifest: balaur_plugin::Manifest,
}

impl Default for AnimationPlugin {
    fn default() -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("animation", env!("CARGO_PKG_VERSION")),
        }
    }
}

/// What a definition table holds, for the generated reference.
const CLIP_ASSET_DOC: &str = r#"A clip keys node properties over time. `loop` is `none`, `loop` or `pingpong`; each track names a `target`, a `property`, an `interp` and its `keys`.

```toml
type = "animation_clip"

[clips.patrol]           # one clip per file, or several, addressed as file.toml#patrol
length = 4.0             # seconds; left out, the clip ends at its last key
loop = "pingpong"        # none, loop or pingpong

[[clips.patrol.tracks]]
target = ""              # node path relative to the playing node; empty is that node
property = "position"    # rotation_euler, rotation, scale, visible, tint or <component>/<property>
interp = "linear"        # step, linear or cubic
keys = [
  { t = 0.0, value = [-2.5, 0.25, -2.0] },
  { t = 4.0, value = [-2.5, 0.25, 2.0], ease = "in_out_sine" },
]

[[clips.patrol.tracks]]  # no property: a method track, each key a call on the node's script
keys = [{ t = 2.0, call = "on_halfway" }]
```"#;

impl balaur_plugin::Plugin for AnimationPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(AnimationState::default());
        reg.add_system(Stage::Update, system::advance_system);
        snapshot::register(reg);
        // After the clip has posed the rig, so a modifier has the last word.
        reg.add_system(Stage::Update, modifier::modify_system);
        modifier::register_modifier2d_component(reg);
        modifier::register_modifier3d_component(reg);
        reg.register_asset_type(CLIP_ASSET_TYPE, "animations", CLIP_ASSET_DOC, |value| {
            Ok(Rc::new(clip::parse(value)?) as Rc<dyn Any>)
        });
        reg.register_asset_type(
            retarget::BONE_MAP_ASSET_TYPE,
            "animations",
            retarget::MAP_ASSET_DOC,
            |value| Ok(Rc::new(retarget::parse_map(value)?) as Rc<dyn Any>),
        );
        reg.register_asset_type(
            retarget::PROFILE_ASSET_TYPE,
            "animations",
            retarget::PROFILE_ASSET_DOC,
            |value| Ok(Rc::new(retarget::parse_profile(value)?) as Rc<dyn Any>),
        );
        reg.register_asset_type(
            machine::MACHINE_ASSET_TYPE,
            "animations",
            machine::MACHINE_ASSET_DOC,
            |value| Ok(Rc::new(machine::parse(value)?) as Rc<dyn Any>),
        );
        register_animation_component(reg);
        register_machine_component(reg);
        let mut m = reg.script_module("animation")?;
        install_animation_api(&mut *m);
        Ok(())
    }
}

/// The `animation` scene key — the one an editor-saved `[nodes.animation]`
/// writes, which until now had no handler.
///
/// It backs no component of its own: what it writes is a [`Playback`] in
/// [`AnimationState`], keyed by entity, because the clip is shared between
/// nodes and the playhead is not.
fn register_animation_component(reg: &mut Registry<'_>) {
    reg.register_component(
        COMPONENT,
        ComponentDef {
            doc: "Plays animation clips on the node. `library` is the clip asset, `autoplay` the clip started on load, `speed` the rate; the `animation` module drives playback.",
            schema: ComponentDef::parse_schema(
                "animation",
                &balaur_core::components::ComponentDef::schema(&[
                    (k::LIBRARY, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "The clip library this node plays from" }}"#, crate::player::CLIP_ASSET_TYPE)),
                    (k::AUTOPLAY, r#"{ type = "string", default = "", description = "Clip to start when the scene loads; empty starts nothing" }"#),
                    (k::SPEED, r#"{ type = "float", default = 1.0, description = "Playback rate for every clip on this node" }"#),
                    (k::ROOT, r#"{ type = "string", default = "", description = "Node path the clip's tracks resolve against; empty means this node" }"#),
                ]),
            ),
            tags: &[balaur_core::components::tag::ANIMATION],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                apply_animation(eng, entity, params);
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                remove_animation(eng, entity);
                Ok(())
            }),
            get: Box::new(animation_of),
        },
    );
}

/// The `state_machine` scene key: a machine asset run against a player.
fn register_machine_component(reg: &mut Registry<'_>) {
    reg.register_component(
        machine::COMPONENT,
        ComponentDef {
            doc: "Runs the `state_machine` asset in `machine` over the `player` node's clips. `auto` transitions fire when their conditions come on; `animation.travel` moves to a state.",
            schema: ComponentDef::parse_schema(
                machine::COMPONENT,
                &balaur_core::components::ComponentDef::schema(&[
                    (k::MACHINE, &format!(r#"{{ type = "asset", asset = "{}", default = "", description = "The state machine to run" }}"#, machine::MACHINE_ASSET_TYPE)),
                    (k::PLAYER, r#"{ type = "string", default = "", description = "Node path to the `animation` player it drives; empty means this node" }"#),
                    (k::ACTIVE, r#"{ type = "bool", default = true, description = "Whether the machine is running" }"#),
                    (k::CHECK_NODE, r#"{ type = "string", default = "", description = "Node path whose script answers the transitions' `check` methods; empty means this node" }"#),
                ]),
            ),
            tags: &[balaur_core::components::tag::ANIMATION],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                machine::apply(eng, entity, params);
                Ok(())
            }),
            remove: Box::new(|eng, entity| {
                machine::remove(eng, entity);
                Ok(())
            }),
            get: Box::new(machine::get),
        },
    );
}

fn apply_animation(eng: &Engine, entity: Entity, params: &toml::Value) {
    let text = |key: &str| {
        params
            .get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let autoplay = text(k::AUTOPLAY);
    let speed = balaur_core::components::prop_f32(params, k::SPEED);
    let running = {
        let state = eng.resource::<AnimationState>();
        let mut state = state.borrow_mut();
        let playback = state.players.entry(entity).or_default();
        playback.library = text(k::LIBRARY);
        playback.root = text(k::ROOT);
        playback.autoplay.clone_from(&autoplay);
        playback.speed = speed;
        playback.active()
    };
    // Re-applying the component must not restart a running clip, and a clip
    // that will not load only warns — one bad reference must not kill the scene.
    if !autoplay.is_empty()
        && !running
        && let Err(why) = play(eng, entity, &autoplay)
    {
        tracing::warn!("autoplay '{autoplay}': {why:#}");
    }
}

fn remove_animation(eng: &Engine, entity: Entity) {
    if let Some(state) = eng.try_resource::<AnimationState>() {
        state.borrow_mut().players.shift_remove(&entity);
    }
}

fn animation_of(eng: &Engine, entity: Entity) -> Option<toml::Value> {
    let state = eng.try_resource::<AnimationState>()?;
    let state = state.borrow();
    let playback = state.players.get(&entity)?;
    let mut out = toml::map::Map::new();
    out.insert(k::LIBRARY.into(), playback.library.clone().into());
    out.insert(k::AUTOPLAY.into(), playback.autoplay.clone().into());
    out.insert(k::SPEED.into(), f64::from(playback.speed).into());
    out.insert(k::ROOT.into(), playback.root.clone().into());
    Some(toml::Value::Table(out))
}

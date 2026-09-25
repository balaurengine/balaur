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
pub mod vocabulary;

pub(crate) use vocabulary::COMPONENT;
pub use vocabulary::{
    ADVANCE_MODES, CONSTANTS, EVENTS, INTERPS, LOOP_MODES, MACHINE_STATES, MODIFIER_KINDS,
    PROPERTIES, SWITCH_MODES, ease_constants, keys, words,
};

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
            warnings: None,
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
            warnings: None,
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

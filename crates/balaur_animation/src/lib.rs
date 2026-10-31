//! Animation as a Balaur plugin: clips, a pure sampler, and playback.
//!
//! Seven pieces, in dependency order:
//!
//! 1. [`clip`] — the `animation_library` asset type, registered through
//!    `App::register_asset_type`. Core never learns what a clip is; a clip is
//!    shared by every node that names it, and immutable once parsed.
//! 2. [`ease`] — eleven transitions in four modes, on `libm`, with Godot's
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
    ADVANCE_MODES, CONSTANTS, EVENTS, INTERPOLATIONS, LOOP_MODES, MACHINE_STATES, MODIFIER_KINDS,
    PROPERTIES, SWITCH_MODES, ease_constants, keys, words,
};

use balaur_plugin::Registry;
use std::any::Any;
use std::rc::Rc;

use anyhow::Result;
use balaur_core::components::ComponentDef;
use balaur_core::components::derived::{Component, Written};
use balaur_core::hecs::Entity;
use balaur_core::{Engine, Stage};

pub use crate::bindings::install_animation_api;
pub use crate::machine::{STATE_FINISHED_EVENT, STATE_STARTED_EVENT, StateMachine};
pub use crate::player::{
    AnimationState, LIBRARY_ASSET_TYPE, Playback, add_clip, current_clip, is_playing,
    just_finished, pause, play, play_blended, play_from, queue, resume, seek, set_retarget,
    set_speed_scale, stop, time,
};
pub use crate::retarget::{BONE_MAP_ASSET_TYPE, BoneMap, PROFILE_ASSET_TYPE, SkeletonProfile};
pub use crate::system::{
    CHANGED_EVENT, FINISHED_EVENT, LOOPED_EVENT, STARTED_EVENT, TWEEN_FINISHED_EVENT,
    TWEEN_LOOPED_EVENT, TWEEN_STEP_EVENT,
};
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
const LIBRARY_ASSET_DOC: &str = r#"A library holds clips, and a clip keys node properties over time. `loop_mode` is `none`, `linear` or `pingpong`; each track names a `target`, a `property`, an `interpolation` and its `keys`.

```toml
type = "animation_library"

[clips.patrol]           # one clip per file, or several, addressed as file.toml#patrol
length = 4.0             # seconds; left out, the clip ends at its last key
loop_mode = "pingpong"   # none, linear or pingpong

[[clips.patrol.tracks]]
target = ""              # node path relative to the playing node; empty is that node
property = "position"    # rotation_euler, rotation, scale, visible, tint or <component>/<property>
interpolation = "linear" # step, linear or cubic
keys = [
  { time = 0.0, value = [-2.5, 0.25, -2.0] },
  { time = 4.0, value = [-2.5, 0.25, 2.0], ease = "in_out_sine" },
]

[[clips.patrol.tracks]]  # no property: a method track, each key a call on the node's script
keys = [{ time = 2.0, call = "on_halfway" }]
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
        reg.register_asset_type(
            LIBRARY_ASSET_TYPE,
            "animations",
            LIBRARY_ASSET_DOC,
            |value| Ok(Rc::new(clip::parse(value)?) as Rc<dyn Any>),
        );
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

/// Plays animation clips on the node. `library` is the clip asset, `autoplay` the clip started on load, `speed_scale` the rate; the `animation` module drives playback.
#[derive(Clone, Debug, Component)]
#[component(name = "animation")]
pub struct Animation {
    /// The clip library this node plays from
    #[property(asset = "animation_library")]
    pub library: String,
    /// Clip to start when the scene loads; empty starts nothing
    pub autoplay: String,
    /// Playback rate for every clip on this node
    #[property(default = 1.0)]
    pub speed_scale: f32,
    /// The node the clip's tracks resolve against; empty means this node
    #[property(node)]
    pub root_node: String,
}

/// The `animation` scene key — the one an editor-saved `[nodes.animation]`
/// writes.
///
/// The node holds what was asked, an [`Animation`]; the playhead is a
/// [`Playback`] in [`AnimationState`], keyed by entity, because the clip is
/// shared between nodes and the playhead is not.
fn register_animation_component(reg: &mut Registry<'_>) {
    reg.register_component(
        COMPONENT,
        ComponentDef {
            events: &[
                (system::STARTED_EVENT, "the clip's name"),
                (system::CHANGED_EVENT, "`#{ from, to }`, the clips' names"),
                (system::LOOPED_EVENT, "the clip's name"),
                (system::FINISHED_EVENT, "the clip's name"),
            ],
            tags: &[balaur_core::components::tag::ANIMATION],
            remove: Box::new(|eng, entity| {
                let _ = eng.remove_one::<Animation>(entity);
                if let Some(state) = eng.try_resource::<AnimationState>() {
                    state.borrow_mut().players.shift_remove(&entity);
                }
                Ok(())
            }),
            ..ComponentDef::of::<Animation>(Some(animation_written))
        },
    );
}

/// The `state_machine` scene key: a machine asset run against a player.
fn register_machine_component(reg: &mut Registry<'_>) {
    reg.register_component(
        machine::COMPONENT,
        ComponentDef {
            events: &[
                (machine::STATE_STARTED_EVENT, "the state entered"),
                (machine::STATE_FINISHED_EVENT, "the state left"),
            ],
            tags: &[balaur_core::components::tag::ANIMATION],
            remove: Box::new(|eng, entity| {
                let _ = eng.remove_one::<StateMachine>(entity);
                if let Some(state) = eng.try_resource::<AnimationState>() {
                    state.borrow_mut().machines.shift_remove(&entity);
                }
                Ok(())
            }),
            ..ComponentDef::of::<StateMachine>(Some(machine::written))
        },
    );
}

/// The playhead takes what the node now asks, and the `autoplay` clip starts.
///
/// Re-writing the component must not restart a running clip, and a clip that
/// will not load only warns — one bad reference must not kill the scene.
#[allow(
    clippy::unnecessary_wraps,
    reason = "an after-write hook may refuse a write; this one never does"
)]
fn animation_written(eng: &Engine, entity: Entity, _: Written) -> Result<()> {
    let Some(asked) = eng
        .world()
        .get::<&Animation>(entity)
        .ok()
        .map(|held| (*held).clone())
    else {
        return Ok(());
    };
    let running = {
        let state = eng.resource::<AnimationState>();
        let mut state = state.borrow_mut();
        let playback = state.players.entry(entity).or_default();
        playback.library.clone_from(&asked.library);
        playback.root_node.clone_from(&asked.root_node);
        playback.autoplay.clone_from(&asked.autoplay);
        playback.speed_scale = asked.speed_scale;
        playback.active()
    };
    if !asked.autoplay.is_empty()
        && !running
        && let Err(why) = play(eng, entity, &asked.autoplay)
    {
        tracing::warn!("autoplay '{}': {why:#}", asked.autoplay);
    }
    Ok(())
}

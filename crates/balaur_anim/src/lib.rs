//! Animation as a Balaur plugin: clips, a pure sampler, and playback.
//!
//! Seven pieces, in dependency order:
//!
//! 1. [`clip`] — the `animation_clip` asset type, registered through
//!    `App::register_asset_type`. Core never learns what a clip is; a clip is
//!    shared by every node that names it, and immutable once parsed.
//! 2. [`ease`] — twelve transitions in four modes, on `libm`, with Godot's
//!    names and Godot's shapes.
//! 3. [`sampler`] — `(clip, time) -> pose`, pure, so blend trees can compose
//!    samples later without the data model changing under them.
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
pub mod modifier;
pub mod player;
pub mod sampler;
mod system;
pub mod tween;

use std::any::Any;
use std::rc::Rc;

use anyhow::Result;
use balaur_core::components::{as_f64, ComponentDef};
use balaur_core::hecs::Entity;
use balaur_core::{App, Engine, Plugin, Stage};

pub use crate::bindings::install_animation_api;
pub use crate::player::{
    current, define, is_playing, just_finished, pause, play, play_from, queue, resume, seek,
    set_speed, stop, time, AnimationState, Playback, CLIP_ASSET_TYPE,
};
pub use crate::tween::{Tween, TweenId};

pub struct AnimationPlugin;

/// What a definition table holds, for the generated reference.
const CLIP_ASSET_DOC: &str = r#"A clip keys node properties over time. `length` is in seconds and may be
left out to end at the last key; `loop` is `none` (hold the last key),
`loop` or `pingpong`. Each track names a `target` node path relative to the
playing node (empty means that node), a `property` (`position`,
`rotation_euler`, `rotation`, `scale` or `<component>/<property>`), an
`interp` (`step`, `linear`, `cubic`) and its `keys`, each `{ t, value }` with
an optional `ease`. A track with no `property` is a method track whose keys
call the node's script. A file holds one clip, or several under
`[clips.<name>]`, addressed as `file.toml#name`.

```toml
type = "animation_clip"

[clips.patrol]
length = 4.0
loop = "pingpong"

[[clips.patrol.tracks]]
property = "position"
interp = "linear"
keys = [
  { t = 0.0, value = [-2.5, 0.25, -2.0] },
  { t = 4.0, value = [-2.5, 0.25, 2.0], ease = "in_out_sine" },
]
```"#;

impl Plugin for AnimationPlugin {
    fn name(&self) -> &'static str {
        "animation"
    }

    fn build(&mut self, app: &mut App) -> Result<()> {
        app.engine.insert_resource(AnimationState::default());
        app.add_system(Stage::Update, system::advance_system);
        // After the clip has posed the rig, so a modifier has the last word.
        app.add_system(Stage::Update, modifier::modify_system);
        modifier::register_modifier2d_component(app);
        app.register_asset_type(CLIP_ASSET_TYPE, "animations", CLIP_ASSET_DOC, |value| {
            Ok(Rc::new(clip::parse(value)?) as Rc<dyn Any>)
        });
        register_animation_component(app);
        let mut m = app.script_module("animation")?;
        m.drives(&["animation"]);
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
fn register_animation_component(app: &mut App) {
    app.register_component(
        "animation",
        ComponentDef {
            schema: ComponentDef::parse_schema(
                "animation",
                r#"library = { type = "asset", asset = "animation_clip", default = "", description = "The clip library this node plays from" }
autoplay = { type = "string", default = "", description = "Clip to start when the scene loads; empty starts nothing" }
speed = { type = "float", default = 1.0, description = "Playback rate for every clip on this node" }
root = { type = "string", default = "", description = "Node path the clip's tracks resolve against; empty means this node" }"#,
            ),
            tags: &["animation"],
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

fn apply_animation(eng: &Engine, entity: Entity, params: &toml::Value) {
    let text = |key: &str| {
        params
            .get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let autoplay = text("autoplay");
    let speed = params.get("speed").and_then(as_f64).unwrap_or(1.0) as f32;
    let running = {
        let state = eng.resource::<AnimationState>();
        let mut state = state.borrow_mut();
        let playback = state.players.entry(entity).or_default();
        playback.library = text("library");
        playback.root = text("root");
        playback.autoplay.clone_from(&autoplay);
        playback.speed = speed;
        playback.active()
    };
    // Re-applying the component must not restart a running clip, and a clip
    // that will not load only warns — one bad reference must not kill the scene.
    if !autoplay.is_empty() && !running {
        if let Err(why) = play(eng, entity, &autoplay) {
            tracing::warn!("autoplay '{autoplay}': {why:#}");
        }
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
    out.insert("library".into(), playback.library.clone().into());
    out.insert("autoplay".into(), playback.autoplay.clone().into());
    out.insert("speed".into(), f64::from(playback.speed).into());
    out.insert("root".into(), playback.root.clone().into());
    Some(toml::Value::Table(out))
}

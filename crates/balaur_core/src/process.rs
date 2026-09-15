//! `process`: which nodes keep ticking while the game is paused.
//!
//! One key on a node, inherited by its subtree, the way Godot's process mode
//! is. `engine.set_paused(true)` holds the tree; a node set to `always` keeps
//! its subtree running through the hold, which is what a pause menu is.
//!
//! Read by every subsystem that ticks per node — scripts, animation, the
//! `timer` component — so one key stops all of them together. Physics is one
//! world and cannot be held per node: a paused game holds every body, as
//! Godot's does.

use hecs::{Entity, World};

use crate::engine::Engine;
use crate::scene::Parent;

/// The scene key and the node op both spell it this way.
pub const KEY: &str = "process";

/// When a node and its subtree tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ProcessMode {
    /// Take the nearest ancestor's answer; the root's is [`Self::Pausable`].
    #[default]
    Inherit,
    /// Ticks while the game runs and stops while it is paused.
    Pausable,
    /// Ticks only while the game is paused: a menu that drives itself.
    WhenPaused,
    /// Ticks either way. What a pause menu, a loading screen and the music
    /// that outlives a pause are set to.
    Always,
    /// Never ticks, paused or not.
    Disabled,
}

impl ProcessMode {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::Pausable => "pausable",
            Self::WhenPaused => "when_paused",
            Self::Always => "always",
            Self::Disabled => "disabled",
        }
    }

    /// The mode a scene key or a script named, or `None` for a word that is
    /// not one of them.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "inherit" => Some(Self::Inherit),
            "pausable" => Some(Self::Pausable),
            "when_paused" => Some(Self::WhenPaused),
            "always" => Some(Self::Always),
            "disabled" => Some(Self::Disabled),
            _ => None,
        }
    }

    /// Every mode a node may be set to, for a schema and an editor's list.
    pub const ALL: [Self; 5] = [
        Self::Inherit,
        Self::Pausable,
        Self::WhenPaused,
        Self::Always,
        Self::Disabled,
    ];

    /// Whether a node resolved to this mode ticks, given the game's state.
    #[must_use]
    const fn runs(self, paused: bool) -> bool {
        match self {
            Self::Disabled => false,
            Self::WhenPaused => paused,
            Self::Always => true,
            // Inherit cannot reach here: `resolved` stops at the root.
            Self::Inherit | Self::Pausable => !paused,
        }
    }
}

/// What the node was set to itself, [`ProcessMode::Inherit`] when nothing.
#[must_use]
pub fn own(world: &World, entity: Entity) -> ProcessMode {
    world
        .get::<&ProcessMode>(entity)
        .map_or(ProcessMode::Inherit, |m| *m)
}

/// Set a node's own mode. [`ProcessMode::Inherit`] takes the key back off, so
/// a node that was never set and a node set back to inherit are one thing.
pub fn set(world: &mut World, entity: Entity, mode: ProcessMode) {
    if mode == ProcessMode::Inherit {
        let _ = world.remove_one::<ProcessMode>(entity);
    } else {
        let _ = world.insert_one(entity, mode);
    }
}

/// The mode that governs `entity`: its own, else the nearest ancestor's that
/// is not [`ProcessMode::Inherit`], else [`ProcessMode::Pausable`].
#[must_use]
pub fn resolved(world: &World, entity: Entity) -> ProcessMode {
    let mut current = entity;
    loop {
        let mode = own(world, current);
        if mode != ProcessMode::Inherit {
            return mode;
        }
        match world.get::<&Parent>(current) {
            Ok(parent) => current = parent.0,
            Err(_) => return ProcessMode::Pausable,
        }
    }
}

/// The pause as a node-by-node question: whether it is on, and which subtree
/// it covers.
///
/// Read once per frame with [`pause`] and asked of each node, so the
/// `is_within` walk only happens while something is actually held.
#[derive(Clone, Copy, Default)]
pub struct Pause {
    on: bool,
    /// The subtree the pause reaches. Inside an editor the game is a subtree
    /// and the shell around it must keep running; `None` is the whole tree,
    /// which is what a shipped game is.
    scope: Option<Entity>,
    /// Whether any node carries a mode at all. False is the common case, and
    /// it turns the walk to the root into one compare.
    modes: bool,
}

impl Pause {
    #[must_use]
    fn covers(self, world: &World, entity: Entity) -> bool {
        self.on
            && self
                .scope
                .is_none_or(|root| crate::scene::is_within(world, entity, root))
    }
}

/// What the engine's pause holds right now.
#[must_use]
pub fn pause(eng: &Engine) -> Pause {
    Pause {
        on: eng.paused(),
        scope: eng.debug_scope(),
        modes: eng.world().query::<&ProcessMode>().iter().next().is_some(),
    }
}

/// Whether `entity` ticks this frame, pause and process mode together.
#[must_use]
pub fn ticks(world: &World, entity: Entity, pause: Pause) -> bool {
    // With no mode anywhere the answer is the pause alone, and `covers`
    // answers that without touching the node.
    if !pause.modes {
        return !pause.covers(world, entity);
    }
    resolved(world, entity).runs(pause.covers(world, entity))
}

/// [`ticks`], reading the engine's own pause. The call a subsystem asking
/// about one node makes.
#[must_use]
pub fn ticking(eng: &Engine, entity: Entity) -> bool {
    ticks(&eng.world(), entity, pause(eng))
}

/// Tell every script the game paused or resumed, once per change.
///
/// Announced to instances the pause itself holds: `on_paused(true)` is how a
/// script learns it has stopped, so filtering it by the pause would be the
/// one hook nobody ever receives.
pub(crate) fn announce_pause_system(eng: &Engine, _: f32) {
    let Some(paused) = eng.take_pause_change() else {
        return;
    };
    if let Some(host) = eng.script_host() {
        host.announce(
            crate::hooks::ON_PAUSED,
            &[balaur_script::Value::Bool(paused)],
        );
    }
}

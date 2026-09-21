//! `multiplayer.*`: host, join and play a rollback match from a script.
//!
//! A host listens and takes slot 0; a joiner dials it, says hello, and is
//! given a slot. When the lobby is full, or the host says `start`, every
//! machine loads the match scene and the joiners restore the host's world
//! as it stood after loading, so the first tick starts from one world.
//! From there the engine steps the match itself: each tick carries every
//! slot's input, late inputs roll the world back, and the host relays each
//! player's inputs to the others.
//!
//! Scripts never send. They set their own input with `set_input` and read
//! everyone's with `rollback.input(slot)`. Events reach every script's
//! `on_multiplayer_event` as a map with a `kind`; they report, and a change
//! to the world made from one is not something the other machines make.

use std::collections::BTreeMap;

use anyhow::Result;
use balaur_core::netsession::{LinkStats, NetSession};
use balaur_core::rollback::{Input, PlayerId};
use balaur_core::{Engine, Stage};
use balaur_script::Value;

mod api;
mod links;
mod lobby;
mod options;
mod play;
mod vocabulary;
mod wire;

pub use options::Options;
pub use vocabulary::{EventKind, HOOK, Role, State, Status, TransportKind};
pub use wire::{Member, PROTOCOL};

/// Where this machine is, and everything it holds in that phase.
pub(crate) enum Phase {
    Idle,
    Hosting(lobby::Lobby),
    Joining(lobby::Joiner),
    /// Told to start; the next frame loads the scene.
    Starting(lobby::Starting),
    Playing,
}

/// One machine's match, from `host` or `join` to `leave`.
pub struct MultiplayerState {
    pub(crate) phase: Phase,
    pub(crate) role: Option<Role>,
    /// This machine's own slot.
    pub(crate) local: Option<PlayerId>,
    pub(crate) roster: Vec<Member>,
    pub(crate) options: Option<Options>,
    /// Events waiting for the next frame's delivery.
    pub(crate) events: Vec<Value>,
    /// What scripts set for the next tick, per slot this machine plays.
    pub(crate) pending: BTreeMap<PlayerId, Input>,
    /// The session, while playing; out of here while the driver ticks it.
    pub(crate) net: Option<NetSession>,
    /// Frame time owed to ticks not run yet.
    pub(crate) owed: f32,
    /// Slots that left, with the first tick each has no input for.
    pub(crate) absent: BTreeMap<PlayerId, u64>,
    /// Each slot's link, as of the last tick.
    pub(crate) stats: BTreeMap<PlayerId, LinkStats>,
    /// Which slot is at the far end of each session link, in link order.
    pub(crate) link_slots: Vec<PlayerId>,
    pub(crate) desync_told: bool,
    /// A script asked to leave while the driver held the session.
    pub(crate) leave_asked: bool,
    /// When play began; a link never heard from is timed from here.
    pub(crate) began: Option<balaur_core::time::Instant>,
}

impl Default for MultiplayerState {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            role: None,
            local: None,
            roster: Vec::new(),
            options: None,
            events: Vec::new(),
            pending: BTreeMap::new(),
            net: None,
            owed: 0.0,
            absent: BTreeMap::new(),
            stats: BTreeMap::new(),
            link_slots: Vec::new(),
            desync_told: false,
            leave_asked: false,
            began: None,
        }
    }
}

impl MultiplayerState {
    /// Where this machine is.
    #[must_use]
    pub const fn state(&self) -> State {
        match self.phase {
            Phase::Idle => State::Idle,
            Phase::Joining(ref joiner) if !joiner.welcomed => State::Connecting,
            Phase::Hosting(_) | Phase::Joining(_) => State::Lobby,
            Phase::Starting(_) | Phase::Playing => State::Playing,
        }
    }

    /// Where a hosting lobby listens: the URL, and the certificate hash a
    /// joiner pins.
    #[must_use]
    pub fn address(&self) -> Option<(String, Option<String>)> {
        match &self.phase {
            Phase::Hosting(lobby) => Some(lobby.address()),
            _ => None,
        }
    }

    /// The running session, between ticks.
    #[must_use]
    pub const fn session(&self) -> Option<&NetSession> {
        self.net.as_ref()
    }

    /// This machine's own slot.
    #[must_use]
    pub const fn local(&self) -> Option<PlayerId> {
        self.local
    }

    /// Queue an event for every script's `on_multiplayer_event`.
    pub(crate) fn tell(&mut self, kind: EventKind, fields: Vec<(&str, Value)>) {
        let mut map = vec![("kind".to_string(), Value::Str(kind.name().into()))];
        map.extend(
            fields
                .into_iter()
                .map(|(key, value)| (key.to_string(), value)),
        );
        self.events.push(Value::Map(map));
    }

    /// The name a slot plays under, as the roster has it.
    pub(crate) fn name_of(&self, slot: PlayerId) -> String {
        self.roster
            .iter()
            .find(|member| member.slot == slot)
            .map(|member| member.name.clone())
            .unwrap_or_default()
    }

    /// Back to idle, forgetting the match.
    pub(crate) fn reset(&mut self) {
        let events = std::mem::take(&mut self.events);
        *self = Self {
            events,
            ..Self::default()
        };
    }
}

/// A slot as a script number.
pub(crate) fn slot_value(slot: PlayerId) -> Value {
    Value::Int(i64::from(slot))
}

pub struct MultiplayerPlugin {
    manifest: balaur_plugin::Manifest,
}

impl Default for MultiplayerPlugin {
    fn default() -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("multiplayer", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl balaur_plugin::Plugin for MultiplayerPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(MultiplayerState::default());
        reg.insert_resource(balaur_core::app::FrameDriver(Some(Box::new(play::drive))));
        reg.add_system(Stage::First, poll_system);
        reg.add_system(Stage::First, deliver_system);
        balaur_core::settings::define_group(
            reg.engine(),
            options::CATEGORY,
            balaur_core::settings::Scope::Project,
            &balaur_core::ComponentDef::parse_schema("settings.multiplayer.match", options::SCHEMA),
        );
        let mut m = reg.script_module("multiplayer")?;
        api::install(&mut *m);
        Ok(())
    }
}

/// Run the lobby: accept, greet, and notice who came and went.
fn poll_system(eng: &Engine, _dt: f32) {
    let state = eng.resource::<MultiplayerState>();
    let mut state = state.borrow_mut();
    match state.phase {
        Phase::Hosting(_) => lobby::poll_host(&mut state),
        Phase::Joining(_) => lobby::poll_join(&mut state),
        _ => {}
    }
}

/// Hand queued events to every script, once, outside a re-run tick.
fn deliver_system(eng: &Engine, _dt: f32) {
    if balaur_core::rollback::is_resimulating(eng) {
        return;
    }
    let events = std::mem::take(&mut eng.resource::<MultiplayerState>().borrow_mut().events);
    let Some(host) = eng.script_host() else {
        return;
    };
    for event in events {
        host.call_all_with(HOOK, &[event]);
    }
}

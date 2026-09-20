//! The words `multiplayer.*` answers in. Each is a script constant, so a
//! script compares against `multiplayer::ROLE_HOST` rather than a string.

use balaur_script::{Bindings, Value};
use smol_str::SmolStr;

/// The method every script is called with a match's events.
pub const HOOK: &str = "on_multiplayer_event";

/// Which end of the match this machine is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Listening, and playing slot 0.
    Host,
    /// Linked to a host.
    Client,
}

impl Role {
    pub const ALL: [Self; 2] = [Self::Host, Self::Client];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Client => "client",
        }
    }
}

/// Where this machine is in a match's life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    /// Dialled a host and waiting to be let in.
    Connecting,
    /// In, waiting for the host to start.
    Lobby,
    Playing,
}

impl State {
    pub const ALL: [Self; 4] = [Self::Idle, Self::Connecting, Self::Lobby, Self::Playing];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Connecting => "connecting",
            Self::Lobby => "lobby",
            Self::Playing => "playing",
        }
    }
}

/// What an `on_multiplayer_event` map's `kind` says happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// This machine is in a lobby, with a slot.
    Connected,
    /// A join was refused or timed out.
    Failed,
    /// Another player came.
    Joined,
    /// Another player went.
    Left,
    /// The match scene is loaded and the first tick is next.
    Started,
    /// This machine and a peer disagree about a tick.
    Desync,
    /// This machine's match or lobby ended.
    Closed,
}

impl EventKind {
    pub const ALL: [Self; 7] = [
        Self::Connected,
        Self::Failed,
        Self::Joined,
        Self::Left,
        Self::Started,
        Self::Desync,
        Self::Closed,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Connected => "connected",
            Self::Failed => "failed",
            Self::Joined => "joined",
            Self::Left => "left",
            Self::Started => "started",
            Self::Desync => "desync",
            Self::Closed => "closed",
        }
    }
}

/// Whether a slot is still played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Present,
    /// Left; the slot's input is nil from then on.
    Absent,
}

impl Status {
    pub const ALL: [Self; 2] = [Self::Present, Self::Absent];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Absent => "absent",
        }
    }
}

/// What a host listens with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportKind {
    /// QUIC datagrams; native only.
    Webtransport,
    /// A websocket, where UDP is blocked; a "datagram" here is reliable.
    Websocket,
}

impl TransportKind {
    pub const ALL: [Self; 2] = [Self::Webtransport, Self::Websocket];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Webtransport => "webtransport",
            Self::Websocket => "websocket",
        }
    }

    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

/// Every word above as a `PREFIX_NAME` constant, and the digest's local tag.
pub(crate) fn install_constants(m: &mut dyn Bindings<balaur_core::Engine>) {
    let mut word = |prefix: &str, name: &str| {
        m.constant(
            &format!("{prefix}_{}", name.to_uppercase()),
            Value::Str(SmolStr::new(name)),
        );
    };
    for role in Role::ALL {
        word("ROLE", role.name());
    }
    for state in State::ALL {
        word("STATE", state.name());
    }
    for kind in EventKind::ALL {
        word("EVENT", kind.name());
    }
    for status in Status::ALL {
        word("STATUS", status.name());
    }
    for kind in TransportKind::ALL {
        word("TRANSPORT", kind.name());
    }
    word("TAG", balaur_core::digest::TAG_LOCAL);
}

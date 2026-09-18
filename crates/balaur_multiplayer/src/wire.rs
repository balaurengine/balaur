//! What a host and its players say besides inputs: the lobby handshake, the
//! start, and who left. Carried reliably, as a session's control messages.

use balaur_core::netsession;
use balaur_core::rollback::{Input, PlayerId};
use serde::{Deserialize, Serialize};

/// Bumped whenever a message below changes shape.
pub const PROTOCOL: u32 = 1;

/// One slot in the lobby, as every machine is told it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub slot: PlayerId,
    pub name: String,
    /// Played by the host's script rather than by a machine of its own.
    #[serde(default)]
    pub bot: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Control {
    /// A joiner introducing itself.
    Hello {
        protocol: u32,
        name: String,
        #[serde(default)]
        token: String,
    },
    /// The host letting a joiner in, as `slot`.
    Welcome {
        slot: PlayerId,
        players: u32,
        roster: Vec<Member>,
    },
    Refuse {
        reason: String,
    },
    /// Who is in the lobby now.
    Roster {
        roster: Vec<Member>,
    },
    /// The match begins from the host's world as it stood after loading
    /// `scene`.
    Start {
        scene: String,
        depth: usize,
        roster: Vec<Member>,
        snapshot: serde_json::Value,
    },
    /// `slot` plays no tick from `from`; `values` are its last inputs, one
    /// per tick from `first`, for a machine the relay had not reached yet.
    Absent {
        slot: PlayerId,
        from: u64,
        first: u64,
        values: Vec<Input>,
    },
    /// The sender is leaving.
    Bye,
}

impl Control {
    #[must_use]
    pub(crate) fn to_value(&self) -> netsession::Control {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    #[must_use]
    pub(crate) fn from_value(value: netsession::Control) -> Option<Self> {
        serde_json::from_value(value).ok()
    }

    /// The bytes for a link no session holds yet.
    #[must_use]
    pub(crate) fn bytes(&self) -> Vec<u8> {
        netsession::encode_control(&self.to_value())
    }

    #[must_use]
    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        Self::from_value(netsession::decode_control(bytes)?)
    }
}

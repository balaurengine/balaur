//! A rollback [`Session`] with the other players on the far end of a
//! [`Transport`].
//!
//! Only inputs cross the wire. Each peer sends its own input for the tick it
//! is about to run and predicts everyone else's; when the real one lands the
//! session rolls back and re-runs. That is the whole protocol on the hot
//! path, and it is a datagram because an input that arrives late is worth
//! less than the one behind it.
//!
//! The cold path is the desync check. Peers exchange the digest of a tick a
//! few behind the one they are running — far enough back that rollbacks have
//! settled — and compare it against their own. A mismatch is not something to
//! recover from: two simulations that disagree about one tick disagree about
//! every tick after it. The session records which tick it was and stops
//! claiming to be in sync, so a game can say so rather than drift.
//!
//! Digests travel reliably, inputs do not. Losing an input costs a
//! misprediction; losing a digest would mean never noticing a desync.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::digest::Digest;
use crate::rollback::{Input, PlayerId, Session};
use crate::transport::{Delivery, Transport};

/// How far behind the running tick the digest exchange sits.
///
/// Far enough that a rollback has already rewritten the number, so peers are
/// comparing settled ticks rather than racing each other's corrections.
const CONFIRM_LAG: u64 = 4;

/// What one peer says to another. JSON because it is small, self-describing
/// and easy to look at when a session misbehaves; a compact encoding is worth
/// doing when the wire is the bottleneck, and it is not yet.
#[derive(Serialize, Deserialize)]
enum Message {
    Input {
        tick: u64,
        player: PlayerId,
        value: Input,
    },
    Digest {
        tick: u64,
        digest: u64,
    },
}

/// Two peers disagreeing about one tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Desync {
    pub tick: u64,
    pub ours: Digest,
    pub theirs: Digest,
}

/// One player's view of a networked rollback session.
pub struct NetSession {
    local: PlayerId,
    session: Session,
    peers: Vec<Box<dyn Transport>>,
    /// This player's input for the tick about to run.
    pending: Input,
    /// Digests peers reported, by tick, until ours catches up to compare.
    claimed: BTreeMap<u64, Digest>,
    desync: Option<Desync>,
}

impl NetSession {
    /// A session where `local` is this machine's player.
    #[must_use]
    pub fn new(local: PlayerId, players: &[PlayerId], depth: usize) -> Self {
        Self {
            local,
            session: Session::new(players, depth),
            peers: Vec::new(),
            pending: Input::Nil,
            claimed: BTreeMap::new(),
            desync: None,
        }
    }

    /// Add a peer, whichever end of the link this is.
    pub fn add_peer(&mut self, peer: Box<dyn Transport>) {
        self.peers.push(peer);
    }

    /// What this player is doing on the next tick.
    ///
    /// Held until [`NetSession::advance`] runs, which is what stamps it with
    /// a tick number and puts it on the wire.
    pub fn set_input(&mut self, value: Input) {
        self.pending = value;
    }

    /// The tick that runs next.
    #[must_use]
    pub const fn tick(&self) -> u64 {
        self.session.tick()
    }

    /// The tick two peers first disagreed on, once one has been found.
    ///
    /// Terminal: nothing clears it, because nothing after a desync is
    /// trustworthy.
    #[must_use]
    pub const fn desync(&self) -> Option<Desync> {
        self.desync
    }

    /// Inputs that arrived too late to answer; see [`Session::stale_inputs`].
    #[must_use]
    pub const fn stale_inputs(&self) -> u64 {
        self.session.stale_inputs()
    }

    /// The underlying local session, for a caller that wants its digests.
    #[must_use]
    pub const fn session(&self) -> &Session {
        &self.session
    }

    /// Read the peers, send this tick's input, run the tick, then exchange
    /// digests.
    ///
    /// Reading happens here rather than inside a system, on purpose: a
    /// re-simulated tick must not re-read the wire. The journal is what a
    /// re-run replays from, and it is already full by the time the tick runs.
    pub fn advance(&mut self, app: &mut App) {
        self.read_peers();
        let tick = self.session.tick();
        let mine = self.pending.clone();
        self.session.submit(self.local, tick, mine.clone());
        self.broadcast_input(tick, &mine);
        self.session.advance(app);
        self.exchange_digests();
    }

    fn read_peers(&mut self) {
        let mut messages = Vec::new();
        for peer in &mut self.peers {
            for received in peer.receive() {
                match serde_json::from_slice::<Message>(&received.bytes) {
                    Ok(message) => messages.push(message),
                    Err(e) => tracing::warn!(error = %e, "a peer sent something unreadable"),
                }
            }
        }
        for message in messages {
            match message {
                Message::Input {
                    tick,
                    player,
                    value,
                } => self.session.submit(player, tick, value),
                Message::Digest { tick, digest } => {
                    self.claimed.insert(tick, Digest(digest));
                }
            }
        }
    }

    fn broadcast_input(&mut self, tick: u64, value: &Input) {
        let message = Message::Input {
            tick,
            player: self.local,
            value: value.clone(),
        };
        self.send(&message, Delivery::Datagram);
    }

    /// Publish the digest of a settled tick, and check anything a peer has
    /// already claimed about one.
    fn exchange_digests(&mut self) {
        let Some(settled) = self.session.tick().checked_sub(CONFIRM_LAG + 1) else {
            return;
        };
        if let Some(ours) = self.session.digest_at(settled) {
            self.send(
                &Message::Digest {
                    tick: settled,
                    digest: ours.0,
                },
                Delivery::Reliable,
            );
        }
        let ticks: Vec<u64> = self.claimed.keys().copied().collect();
        for tick in ticks {
            let (Some(theirs), Some(ours)) = (
                self.claimed.get(&tick).copied(),
                self.session.digest_at(tick),
            ) else {
                continue;
            };
            self.claimed.remove(&tick);
            if theirs != ours && self.desync.is_none() {
                tracing::error!(%tick, %ours, %theirs, "peers disagree; the session has desynced");
                self.desync = Some(Desync { tick, ours, theirs });
            }
        }
    }

    fn send(&mut self, message: &Message, delivery: Delivery) {
        let Ok(bytes) = serde_json::to_vec(message) else {
            return;
        };
        for peer in &mut self.peers {
            let sent = match delivery {
                Delivery::Reliable => peer.send_reliable(&bytes),
                Delivery::Datagram => peer.send_datagram(&bytes),
            };
            if let Err(e) = sent {
                tracing::warn!(error = %e, "a peer send failed");
            }
        }
    }
}

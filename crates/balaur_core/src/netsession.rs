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
use crate::engine::Engine;
use crate::replay::ExternalIo;
use crate::rollback::{Input, PlayerId, Session};
use crate::transport::{Delivery, Received, Transport};

/// How far behind the running tick the digest exchange sits.
///
/// A floor, not the rule: what actually settles a tick is every player's
/// input having arrived for it, which [`Session::confirmed`] answers. The lag
/// only keeps the exchange off the tick currently being corrected.
const CONFIRM_LAG: u64 = 4;

/// How often a peer is pinged, in ticks. Twice a second at 60 Hz: often
/// enough to track a route changing, rare enough to be free.
const PING_EVERY: u64 = 30;

/// How many ticks of this player's input ride in every datagram.
///
/// Inputs are sent unreliably and never retransmitted, so a dropped one would
/// otherwise be lost for good — and a tick simulated on a prediction nobody
/// ever corrects is a permanent divergence, not a recoverable one. Repeating
/// the last few costs a few bytes and means a single packet getting through
/// repairs every gap behind it. At one datagram in twenty lost, twelve in a
/// row is around one run in 2^52.
const INPUT_WINDOW: u64 = 12;

/// What one peer says to another. JSON because it is small, self-describing
/// and easy to look at when a session misbehaves; a compact encoding is worth
/// doing when the wire is the bottleneck, and it is not yet.
#[derive(Serialize, Deserialize)]
enum Message {
    /// One player's input for a run of consecutive ticks, newest last.
    ///
    /// A run rather than a single tick, because this travels unreliably: see
    /// [`INPUT_WINDOW`].
    Inputs {
        player: PlayerId,
        /// One per datagram sent, so the gaps count what was lost.
        seq: u64,
        /// The tick `values[0]` belongs to; the rest follow one per tick.
        from: u64,
        values: Vec<Input>,
    },
    Digest {
        tick: u64,
        digest: u64,
    },
    /// A round trip, measured rather than guessed. Unreliable on purpose: a
    /// ping that had to be retransmitted would measure the retransmit.
    Ping {
        id: u64,
    },
    Pong {
        id: u64,
    },
}

/// What one peer's link is doing.
///
/// An observer, exactly as `engine.timings()` is: nothing here may reach the
/// simulation. Wall time is not reproducible, so a tick that branched on a
/// round-trip time would desync — and would be caught by the digest, since
/// none of this is recorded, replayed or hashed.
#[derive(Clone, Copy, Debug, Default)]
pub struct LinkStats {
    /// The last measured round trip, in milliseconds.
    pub rtt_ms: f32,
    /// The fraction of this peer's input datagrams that never arrived,
    /// counted from the gaps in their sequence numbers.
    pub loss: f32,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

/// One link's measurements, and the running counts behind them.
#[derive(Default)]
struct Link {
    stats: LinkStats,
    /// The highest input sequence seen, and how many arrived, which is all
    /// loss needs: sequences start at one, so the highest is how many were
    /// sent.
    highest_seq: u64,
    seen: u64,
}

impl Link {
    /// Fold one arrival's sequence number into the loss estimate.
    #[allow(
        clippy::cast_precision_loss,
        reason = "a ratio of counts, not an exact quantity"
    )]
    fn saw(&mut self, seq: u64) {
        self.seen += 1;
        self.highest_seq = self.highest_seq.max(seq);
        if self.highest_seq > 0 {
            self.stats.loss = 1.0 - (self.seen as f32 / self.highest_seq as f32).min(1.0);
        }
    }
}

/// Every peer's [`LinkStats`], in the order the peers were added.
///
/// Published once a tick for a profiler dock or a game's own connection
/// meter to read.
#[derive(Default)]
pub struct SessionStats(pub Vec<LinkStats>);

/// Two peers disagreeing about one tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Desync {
    pub tick: u64,
    pub ours: Digest,
    pub theirs: Digest,
}

/// What the peers delivered this tick, before anything decoded it.
///
/// A resource rather than a field of the session, because that is what a
/// replay source can reach. Recording the payloads verbatim is what makes a
/// networked desync reproducible from a file: the same bytes arrive on the
/// same ticks, and `ExternalIo` is already the rule that a replay reads them
/// from the file instead of from a socket.
#[derive(Default)]
pub struct PeerTraffic(ExternalIo<Received>);

/// Register the session's recording source. Called once by `App`.
pub(crate) fn build_session_source(app: &mut App) {
    app.add_replay_source(
        "session",
        |eng| eng.resource::<PeerTraffic>().borrow().0.capture(),
        |eng, value| eng.resource::<PeerTraffic>().borrow().0.restore(value),
    );
}

/// One player's view of a networked rollback session.
pub struct NetSession {
    local: PlayerId,
    session: Session,
    peers: Vec<Box<dyn Transport>>,
    /// This player's input for the tick about to run.
    pending: Input,
    /// What this player has sent, per tick, so a datagram can repeat the
    /// recent past. Trimmed to the window it feeds.
    mine: BTreeMap<u64, Input>,
    /// Digests peers reported, by tick, until ours catches up to compare.
    claimed: BTreeMap<u64, Digest>,
    /// What each link is doing, and the counts behind it.
    links: Vec<Link>,
    /// Datagrams sent, which is the sequence peers count gaps in.
    sent: u64,
    /// Pings in flight, by id, with when they went out.
    pinged: BTreeMap<u64, std::time::Instant>,
    /// The newest tick this peer has published a digest for, so a tick that
    /// takes a while to confirm is still published once it does.
    published: u64,
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
            mine: BTreeMap::new(),
            claimed: BTreeMap::new(),
            links: Vec::new(),
            sent: 0,
            pinged: BTreeMap::new(),
            published: 0,
            desync: None,
        }
    }

    /// Add a peer, whichever end of the link this is.
    pub fn add_peer(&mut self, peer: Box<dyn Transport>) {
        self.peers.push(peer);
        self.links.push(Link::default());
    }

    /// What every link is doing, in the order the peers were added.
    #[must_use]
    pub fn stats(&self) -> Vec<LinkStats> {
        self.links.iter().map(|link| link.stats).collect()
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
        self.read_peers(&app.engine);
        let tick = self.session.tick();
        let mine = self.pending.clone();
        self.session.submit(self.local, tick, mine.clone());
        self.broadcast_input(tick, &mine);
        self.session.advance(app);
        self.exchange_digests();
        self.measure(tick);
        app.engine.resource::<SessionStats>().borrow_mut().0 = self.stats();
    }

    /// Drain the peers into the tick's traffic, then decode what is there.
    ///
    /// Reading goes through `ExternalIo::start`, so a replay never touches a
    /// transport: the recorded payloads are already in the channel, and the
    /// decode below cannot tell the difference.
    fn read_peers(&mut self, eng: &Engine) {
        let traffic = eng.resource::<PeerTraffic>();
        // Measured here rather than after the drain, because the traffic
        // channel merges every peer and a replay reads it with no peer at
        // all: a link's numbers describe a live link or they describe
        // nothing.
        let peers = &mut self.peers;
        let links = &mut self.links;
        traffic.borrow().0.start(eng, |report| {
            for (index, peer) in peers.iter_mut().enumerate() {
                for received in peer.receive() {
                    if let Some(link) = links.get_mut(index) {
                        link.stats.bytes_in += received.bytes.len() as u64;
                        // A second decode, only to attribute the sequence to
                        // the link it came in on. Cheap next to the round
                        // trip it is measuring.
                        if let Ok(Message::Inputs { seq, .. }) =
                            serde_json::from_slice::<Message>(&received.bytes)
                        {
                            link.saw(seq);
                        }
                    }
                    let _ = report.send(received);
                }
            }
        });
        let mut messages = Vec::new();
        for received in traffic.borrow_mut().0.drain() {
            match serde_json::from_slice::<Message>(&received.bytes) {
                Ok(message) => messages.push(message),
                Err(e) => tracing::warn!(error = %e, "a peer sent something unreadable"),
            }
        }
        for message in messages {
            match message {
                Message::Inputs {
                    player,
                    from,
                    values,
                    seq: _,
                } => {
                    for (at, value) in values.into_iter().enumerate() {
                        // A repeat of something already known costs nothing:
                        // `submit` compares against what the tick actually
                        // ran with, so only a correction rolls anything back.
                        self.session.submit(player, from + at as u64, value);
                    }
                }
                Message::Digest { tick, digest } => {
                    self.claimed.insert(tick, Digest(digest));
                }
                Message::Ping { id } => self.send(&Message::Pong { id }, Delivery::Datagram),
                Message::Pong { id } => {
                    if let Some(sent) = self.pinged.remove(&id) {
                        let rtt = sent.elapsed().as_secs_f32() * 1000.0;
                        for link in &mut self.links {
                            link.stats.rtt_ms = rtt;
                        }
                    }
                }
            }
        }
    }

    /// Ping every so often, and forget any that never came back.
    fn measure(&mut self, tick: u64) {
        if tick % PING_EVERY != 0 {
            return;
        }
        self.pinged.insert(tick, std::time::Instant::now());
        // A ping older than a few seconds is not coming back; keeping it
        // would leak and would never resolve.
        let oldest = tick.saturating_sub(PING_EVERY * 8);
        self.pinged.retain(|at, _| *at >= oldest);
        self.send(&Message::Ping { id: tick }, Delivery::Datagram);
    }

    /// Send this tick's input, and the last few again behind it.
    fn broadcast_input(&mut self, tick: u64, value: &Input) {
        self.mine.insert(tick, value.clone());
        let from = tick.saturating_sub(INPUT_WINDOW - 1).max(1);
        self.mine.retain(|at, _| *at >= from);
        let values: Vec<Input> = (from..=tick)
            .map(|at| self.mine.get(&at).cloned().unwrap_or(Input::Nil))
            .collect();
        self.sent += 1;
        let message = Message::Inputs {
            player: self.local,
            seq: self.sent,
            from,
            values,
        };
        self.send(&message, Delivery::Datagram);
    }

    /// Publish the digest of every settled tick not published yet, and check
    /// anything a peer has claimed about one this peer has settled too.
    ///
    /// Settled means confirmed: a tick still resting on a prediction of
    /// somebody's input has a digest that a late arrival will rewrite, and
    /// publishing that races the correction. On a slow link the confirmation
    /// can trail `CONFIRM_LAG` by several ticks, which is why this walks
    /// forward from the last one published rather than looking at one tick.
    fn exchange_digests(&mut self) {
        let Some(settled) = self.session.tick().checked_sub(CONFIRM_LAG + 1) else {
            return;
        };
        while self.published < settled {
            let tick = self.published + 1;
            if !self.session.confirmed(tick) {
                // Dropped from the ring: the input that would settle it can
                // never arrive now, so publishing stops waiting for it rather
                // than going quiet on every tick behind it too.
                if self.session.earliest().is_some_and(|first| tick < first) {
                    self.published = tick;
                    continue;
                }
                break;
            }
            if let Some(ours) = self.session.digest_at(tick) {
                self.send(
                    &Message::Digest {
                        tick,
                        digest: ours.0,
                    },
                    Delivery::Reliable,
                );
            }
            self.published = tick;
        }
        let ticks: Vec<u64> = self.claimed.keys().copied().collect();
        for tick in ticks {
            // In tick order, stopping at the first one not settled here yet:
            // inputs are datagrams, so a later tick can confirm first, and
            // comparing it first would name the wrong tick as the desync.
            if !self.session.confirmed(tick) {
                // One the ring has dropped never will confirm. Blocking on it
                // would hide every tick behind it; `stale_inputs` is where
                // that loss is already counted.
                if self.session.earliest().is_some_and(|first| tick < first) {
                    self.claimed.remove(&tick);
                    continue;
                }
                break;
            }
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
        for (index, peer) in self.peers.iter_mut().enumerate() {
            let sent = match delivery {
                Delivery::Reliable => peer.send_reliable(&bytes),
                Delivery::Datagram => peer.send_datagram(&bytes),
            };
            match sent {
                Ok(()) => {
                    if let Some(link) = self.links.get_mut(index) {
                        link.stats.bytes_out += bytes.len() as u64;
                    }
                }
                Err(e) => tracing::warn!(error = %e, "a peer send failed"),
            }
        }
    }
}

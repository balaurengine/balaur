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
//!
//! A link may be bound to the players it speaks for, so one peer cannot
//! send another's input, and a relaying session forwards every input it
//! accepts to its other links: the star a host sits in the middle of.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::digest::Digest;
use crate::engine::Engine;
use crate::replay::ExternalIo;
use crate::rollback::{Input, PlayerId, Session};
use crate::transport::{Delivery, LinkState, Received, Transport};

/// How far behind the running tick the digest exchange sits.
///
/// A floor, not the rule: what actually settles a tick is every player's
/// input having arrived for it, which [`Session::confirmed`] answers. The lag
/// only keeps the exchange off the tick currently being corrected.
const CONFIRM_LAG: u64 = 4;

/// Where a fault-injected link's dice start. Fixed, so a session that
/// misbehaved once misbehaves identically on the next run.
const FAULT_SEED: u64 = 0x5eed_face;

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

/// How many ticks further ahead than a peer this one may run before it waits.
const LEAD_SLACK: i64 = 2;

/// The replay source peer traffic is recorded under.
pub const SOURCE: &str = "multiplayer";

/// What the layer above a session says between peers. Opaque here: the
/// session carries it on the reliable channel and hands it back.
pub type Control = serde_json::Value;

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
        /// One per datagram sent for this player, so the gaps count what was
        /// lost.
        seq: u64,
        /// The tick `values[0]` belongs to; the rest follow one per tick.
        from: u64,
        values: Vec<Input>,
        /// How far the sender ran ahead of the inputs it had, for pacing.
        #[serde(default)]
        lead: i64,
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
    Control(Control),
}

/// A control message's bytes, for a link no session holds yet.
#[must_use]
pub fn encode_control(control: &Control) -> Vec<u8> {
    serde_json::to_vec(&Message::Control(control.clone())).unwrap_or_default()
}

/// The control message `bytes` carry; `None` for anything else.
#[must_use]
pub fn decode_control(bytes: &[u8]) -> Option<Control> {
    match serde_json::from_slice::<Message>(bytes) {
        Ok(Message::Control(control)) => Some(control),
        _ => None,
    }
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

/// One link's measurements, the counts behind them, and what it may say.
#[derive(Default)]
struct Link {
    stats: LinkStats,
    /// The highest input sequence seen, and how many arrived, which is all
    /// loss needs: sequences start at one, so the highest is how many were
    /// sent.
    highest_seq: u64,
    seen: u64,
    /// The player at the far end, whose own datagrams this link measures.
    direct: Option<PlayerId>,
    /// The players this link may send inputs for; `None` trusts it with any.
    speaks_for: Option<Vec<PlayerId>>,
    /// The far end's last reported lead.
    their_lead: i64,
    closed: bool,
    /// When anything last arrived; wall time, for noticing a silent peer.
    heard: Option<crate::time::Instant>,
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

/// One payload, and which link it came in on.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Arrival {
    pub link: usize,
    pub received: Received,
}

/// What the peers delivered this tick, before anything decoded it.
///
/// A resource rather than a field of the session, because that is what a
/// replay source can reach. Recording the payloads verbatim is what makes a
/// networked desync reproducible from a file: the same bytes arrive on the
/// same ticks, and `ExternalIo` is already the rule that a replay reads them
/// from the file instead of from a socket.
#[derive(Default)]
pub struct PeerTraffic(ExternalIo<Arrival>);

/// Register the session's recording source. Called once by `App`.
pub(crate) fn build_session_source(app: &mut App) {
    app.add_replay_source(
        SOURCE,
        |eng| eng.resource::<PeerTraffic>().borrow().0.capture(),
        |eng, value| eng.resource::<PeerTraffic>().borrow().0.restore(value),
    );
}

/// One machine's view of a networked rollback session.
pub struct NetSession {
    /// The players this machine sends inputs for; the first is its own.
    locals: Vec<PlayerId>,
    session: Session,
    depth: usize,
    peers: Vec<Box<dyn Transport>>,
    /// Each local player's input for the tick about to run.
    pending: BTreeMap<PlayerId, Input>,
    /// What each local player has sent, per tick, so a datagram can repeat
    /// the recent past. Trimmed to the window it feeds.
    mine: BTreeMap<PlayerId, BTreeMap<u64, Input>>,
    /// Digests peers reported, by tick and link, until ours catches up.
    claimed: BTreeMap<(u64, usize), Digest>,
    /// What each link is doing, and the counts behind it.
    links: Vec<Link>,
    /// Datagrams sent per local player, which is the sequence peers count
    /// gaps in.
    sent: BTreeMap<PlayerId, u64>,
    /// Pings in flight, by id, with when they went out.
    pinged: BTreeMap<u64, crate::time::Instant>,
    /// The newest tick this peer has published a digest for, so a tick that
    /// takes a while to confirm is still published once it does.
    published: u64,
    desync: Option<Desync>,
    /// Whether an accepted input is forwarded to every other link.
    relay: bool,
    /// Control messages that arrived, with the link each came in on.
    controls: Vec<(usize, Control)>,
}

impl NetSession {
    /// A session where `local` is this machine's player.
    #[must_use]
    pub fn new(local: PlayerId, players: &[PlayerId], depth: usize) -> Self {
        Self {
            locals: vec![local],
            session: Session::new(players, depth),
            depth,
            peers: Vec::new(),
            pending: BTreeMap::new(),
            mine: BTreeMap::new(),
            claimed: BTreeMap::new(),
            links: Vec::new(),
            sent: BTreeMap::new(),
            pinged: BTreeMap::new(),
            published: 0,
            desync: None,
            relay: false,
            controls: Vec::new(),
        }
    }

    /// Add a peer, whichever end of the link this is, trusted to send any
    /// player's input but this machine's.
    pub fn add_peer(&mut self, eng: &Engine, peer: Box<dyn Transport>) {
        self.add_link(eng, peer, None, None);
    }

    /// Add a peer that is `direct` and may send inputs for `speaks_for` only.
    pub fn add_bound_peer(
        &mut self,
        eng: &Engine,
        peer: Box<dyn Transport>,
        direct: PlayerId,
        speaks_for: Vec<PlayerId>,
    ) {
        self.add_link(eng, peer, Some(direct), Some(speaks_for));
    }

    /// The link is wrapped in [`crate::transport::Faulty`] when the
    /// multiplayer settings ask for it, so every session honours the toggle.
    fn add_link(
        &mut self,
        eng: &Engine,
        peer: Box<dyn Transport>,
        direct: Option<PlayerId>,
        speaks_for: Option<Vec<PlayerId>>,
    ) {
        let peer: Box<dyn Transport> = match crate::settings::faults(eng) {
            // Seeded by position, so two links misbehave differently and the
            // same run twice misbehaves the same way.
            Some(faults) => Box::new(crate::transport::Faulty::new(
                peer,
                faults,
                FAULT_SEED ^ self.peers.len() as u64,
            )),
            None => peer,
        };
        self.peers.push(peer);
        self.links.push(Link {
            direct,
            speaks_for,
            ..Link::default()
        });
    }

    /// Forward every accepted input to the other links.
    pub const fn set_relay(&mut self, on: bool) {
        self.relay = on;
    }

    /// Send inputs for `player` from this machine too: a bot it drives.
    pub fn add_local(&mut self, player: PlayerId) {
        if !self.locals.contains(&player) && self.session.players().contains(&player) {
            self.locals.push(player);
        }
    }

    /// The players this machine sends inputs for, its own first.
    #[must_use]
    pub fn locals(&self) -> &[PlayerId] {
        &self.locals
    }

    /// What every link is doing, in the order the peers were added.
    #[must_use]
    pub fn stats(&self) -> Vec<LinkStats> {
        self.links.iter().map(|link| link.stats).collect()
    }

    /// The link `player`'s inputs reach this machine over: its own, or the
    /// one relaying for it.
    #[must_use]
    pub fn link_of(&self, player: PlayerId) -> Option<usize> {
        self.links
            .iter()
            .position(|link| link.direct == Some(player))
            .or_else(|| {
                self.links.iter().position(|link| {
                    link.speaks_for
                        .as_ref()
                        .is_none_or(|list| list.contains(&player))
                })
            })
    }

    /// What the link `player` is reached over is doing.
    #[must_use]
    pub fn stats_of(&self, player: PlayerId) -> Option<LinkStats> {
        self.links.get(self.link_of(player)?).map(|link| link.stats)
    }

    /// What this machine's own player does on the next tick.
    ///
    /// Held until [`NetSession::advance`] runs, which is what stamps it with
    /// a tick number and puts it on the wire.
    pub fn set_input(&mut self, value: Input) {
        if let Some(&local) = self.locals.first() {
            self.pending.insert(local, value);
        }
    }

    /// What one of this machine's players does on the next tick; ignored
    /// for a player this machine does not send for.
    pub fn set_input_for(&mut self, player: PlayerId, value: Input) {
        if self.locals.contains(&player) {
            self.pending.insert(player, value);
        }
    }

    /// Hand the session an input from outside the wire: one a relaying
    /// peer passed on when its sender left.
    pub fn submit(&mut self, player: PlayerId, tick: u64, value: Input) {
        self.session.submit(player, tick, value);
    }

    /// `player` left; see [`Session::set_absent`].
    pub fn set_absent(&mut self, player: PlayerId, from: u64) {
        self.session.set_absent(player, from);
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

    /// How many ticks this machine is ahead of the newest input it holds
    /// from the furthest-behind player still present.
    #[must_use]
    pub fn lead(&self) -> i64 {
        let tick = self.session.tick();
        self.session
            .players()
            .iter()
            .filter(|p| !self.locals.contains(p) && self.session.is_present(**p, tick))
            .map(|p| self.session.newest_input(*p).unwrap_or(0))
            .min()
            .map_or(0, |newest| signed(tick) - signed(newest))
    }

    /// Whether running the next tick now would outpace a peer: further
    /// ahead than it is of this machine, or so far that its inputs would
    /// land outside the snapshot ring.
    #[must_use]
    pub fn should_wait(&self) -> bool {
        let lead = self.lead();
        let ring = i64::try_from(self.depth).unwrap_or(i64::MAX) - LEAD_SLACK;
        if lead >= ring {
            return true;
        }
        let tick = self.session.tick();
        self.links.iter().any(|link| {
            !link.closed
                && link
                    .direct
                    .is_some_and(|p| self.session.is_present(p, tick))
                && lead - link.their_lead > LEAD_SLACK
        })
    }

    /// Where each link stands, in the order the peers were added.
    #[must_use]
    pub fn link_states(&self) -> Vec<LinkState> {
        self.peers
            .iter()
            .zip(&self.links)
            .map(|(peer, link)| {
                if link.closed {
                    LinkState::Closed(String::from("closed"))
                } else {
                    peer.state()
                }
            })
            .collect()
    }

    /// Seconds since anything arrived on `link`; `None` before anything has.
    #[must_use]
    pub fn silent_for(&self, link: usize) -> Option<f32> {
        self.links
            .get(link)?
            .heard
            .map(|at| at.elapsed().as_secs_f32())
    }

    /// Close one link; nothing is sent on or read from it again.
    pub fn close_link(&mut self, link: usize) {
        if let (Some(peer), Some(state)) = (self.peers.get_mut(link), self.links.get_mut(link)) {
            if !state.closed {
                peer.close();
            }
            state.closed = true;
        }
    }

    /// Close every link.
    pub fn close_all(&mut self) {
        for link in 0..self.peers.len() {
            self.close_link(link);
        }
    }

    /// The control messages that arrived since the last call.
    pub fn take_controls(&mut self) -> Vec<(usize, Control)> {
        std::mem::take(&mut self.controls)
    }

    /// Say something reliably to one link.
    pub fn send_control(&mut self, link: usize, control: &Control) {
        self.send_on(link, &Message::Control(control.clone()), Delivery::Reliable);
    }

    /// Say something reliably to every link.
    pub fn broadcast_control(&mut self, control: &Control) {
        self.send(&Message::Control(control.clone()), Delivery::Reliable);
    }

    /// Read the peers without running a tick, for a frame spent waiting.
    pub fn poll(&mut self, eng: &Engine) {
        self.read_peers(eng);
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
        self.broadcast_inputs(tick);
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
    #[allow(
        clippy::disallowed_methods,
        reason = "when a peer was last heard is an observer, never a simulation input"
    )]
    fn read_peers(&mut self, eng: &Engine) {
        let traffic = eng.resource::<PeerTraffic>();
        let peers = &mut self.peers;
        let links = &mut self.links;
        traffic.borrow().0.start(eng, |report| {
            for (index, (peer, link)) in peers.iter_mut().zip(links.iter_mut()).enumerate() {
                if link.closed {
                    continue;
                }
                for received in peer.receive() {
                    link.stats.bytes_in += received.bytes.len() as u64;
                    link.heard = Some(crate::time::Instant::now());
                    let _ = report.send(Arrival {
                        link: index,
                        received,
                    });
                }
            }
        });
        let arrivals = traffic.borrow_mut().0.drain();
        for arrival in arrivals {
            match serde_json::from_slice::<Message>(&arrival.received.bytes) {
                Ok(message) => self.handle(arrival.link, &arrival.received.bytes, message),
                Err(e) => tracing::warn!(error = %e, "a peer sent something unreadable"),
            }
        }
    }

    fn handle(&mut self, link: usize, bytes: &[u8], message: Message) {
        match message {
            Message::Inputs {
                player,
                seq,
                from,
                values,
                lead,
            } => {
                if !self.may_speak(link, player) {
                    tracing::warn!(
                        player,
                        link,
                        "a link sent an input for a player it does not speak for"
                    );
                    return;
                }
                if let Some(state) = self.links.get_mut(link)
                    && state.direct.is_none_or(|direct| direct == player)
                {
                    state.saw(seq);
                    state.their_lead = lead;
                }
                // Capped and saturated: the count and the first tick are
                // both whatever the peer put in the datagram.
                let window = usize::try_from(INPUT_WINDOW).unwrap_or(usize::MAX);
                for (at, value) in values.into_iter().take(window).enumerate() {
                    let at = u64::try_from(at).unwrap_or(u64::MAX);
                    self.session.submit(player, from.saturating_add(at), value);
                }
                if self.relay {
                    self.forward(link, bytes);
                }
            }
            Message::Digest { tick, digest } => {
                self.claimed.insert((tick, link), Digest(digest));
            }
            Message::Ping { id } => self.send_on(link, &Message::Pong { id }, Delivery::Datagram),
            Message::Pong { id } => {
                if let (Some(sent), Some(state)) = (self.pinged.get(&id), self.links.get_mut(link))
                {
                    state.stats.rtt_ms = sent.elapsed().as_secs_f32() * 1000.0;
                }
            }
            Message::Control(control) => self.controls.push((link, control)),
        }
    }

    /// Whether `link` may send `player`'s input: never this machine's own,
    /// and only a player the link was bound to.
    fn may_speak(&self, link: usize, player: PlayerId) -> bool {
        if self.locals.contains(&player) {
            return false;
        }
        self.links.get(link).is_none_or(|state| {
            state
                .speaks_for
                .as_ref()
                .is_none_or(|list| list.contains(&player))
        })
    }

    /// Ping every so often, and forget any that never came back.
    ///
    /// The clock is what a round trip is; nothing here reaches the
    /// simulation, in the sense `crate::timings` sets out.
    #[allow(
        clippy::disallowed_methods,
        reason = "an observer, never a simulation input"
    )]
    fn measure(&mut self, tick: u64) {
        if !tick.is_multiple_of(PING_EVERY) {
            return;
        }
        self.pinged.insert(tick, crate::time::Instant::now());
        // A ping older than a few seconds is not coming back; keeping it
        // would leak and would never resolve.
        let oldest = tick.saturating_sub(PING_EVERY * 8);
        self.pinged.retain(|at, _| *at >= oldest);
        self.send(&Message::Ping { id: tick }, Delivery::Datagram);
    }

    /// Submit and send each local player's input for this tick, and the
    /// last few again behind it.
    fn broadcast_inputs(&mut self, tick: u64) {
        let lead = self.lead();
        let from = tick.saturating_sub(INPUT_WINDOW - 1).max(1);
        for local in self.locals.clone() {
            let value = self.pending.get(&local).cloned().unwrap_or(Input::Nil);
            self.session.submit(local, tick, value.clone());
            let mine = self.mine.entry(local).or_default();
            mine.insert(tick, value);
            mine.retain(|at, _| *at >= from);
            let values: Vec<Input> = (from..=tick)
                .map(|at| mine.get(&at).cloned().unwrap_or(Input::Nil))
                .collect();
            let seq = self.sent.entry(local).or_insert(0);
            *seq += 1;
            let message = Message::Inputs {
                player: local,
                seq: *seq,
                from,
                values,
                lead,
            };
            self.send(&message, Delivery::Datagram);
        }
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
        self.compare_claims();
    }

    /// Check what peers claimed against every tick settled here, in order.
    fn compare_claims(&mut self) {
        let keys: Vec<(u64, usize)> = self.claimed.keys().copied().collect();
        for key in keys {
            let tick = key.0;
            // In tick order, stopping at the first one not settled here yet:
            // inputs are datagrams, so a later tick can confirm first, and
            // comparing it first would name the wrong tick as the desync.
            if !self.session.confirmed(tick) {
                // One the ring has dropped never will confirm. Blocking on it
                // would hide every tick behind it; `stale_inputs` is where
                // that loss is already counted.
                if self.session.earliest().is_some_and(|first| tick < first) {
                    self.claimed.remove(&key);
                    continue;
                }
                break;
            }
            let (Some(theirs), Some(ours)) = (
                self.claimed.get(&key).copied(),
                self.session.digest_at(tick),
            ) else {
                continue;
            };
            self.claimed.remove(&key);
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
        for link in 0..self.peers.len() {
            self.send_bytes(link, &bytes, delivery);
        }
    }

    fn send_on(&mut self, link: usize, message: &Message, delivery: Delivery) {
        if let Ok(bytes) = serde_json::to_vec(message) {
            self.send_bytes(link, &bytes, delivery);
        }
    }

    /// Pass a datagram on to every link but the one it came in on.
    fn forward(&mut self, from: usize, bytes: &[u8]) {
        for link in (0..self.peers.len()).filter(|link| *link != from) {
            self.send_bytes(link, bytes, Delivery::Datagram);
        }
    }

    fn send_bytes(&mut self, link: usize, bytes: &[u8], delivery: Delivery) {
        let (Some(peer), Some(state)) = (self.peers.get_mut(link), self.links.get_mut(link)) else {
            return;
        };
        if state.closed {
            return;
        }
        let sent = match delivery {
            Delivery::Reliable => peer.send_reliable(bytes),
            Delivery::Datagram => peer.send_datagram(bytes),
        };
        match sent {
            Ok(()) => state.stats.bytes_out += bytes.len() as u64,
            // A link that closed says so through `link_states`.
            Err(_) if matches!(peer.state(), LinkState::Closed(_)) => {}
            Err(e) => tracing::warn!(error = %e, link, "a peer send failed"),
        }
    }
}

/// A tick as a signed count, for a difference that may go negative.
fn signed(tick: u64) -> i64 {
    i64::try_from(tick).unwrap_or(i64::MAX)
}

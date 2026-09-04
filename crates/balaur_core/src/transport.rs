//! One link to one peer, with the two delivery guarantees a game needs.
//!
//! Shaped by QUIC, not by whatever the transport underneath happens to
//! offer. A session sends a tick's inputs as a datagram, because an input
//! that arrives late is worth less than the one behind it, and sends its
//! handshake and its digests reliably, because losing either is not
//! recoverable. Any transport that cannot do both has to fake the one it is
//! missing, and the faking is its problem rather than the caller's: a
//! websocket sends a "datagram" reliably, which is slower than it should be
//! but never wrong.
//!
//! **Polled, not awaited.** Everything else in the engine delivers on a
//! worker thread, crosses a channel, and enters the simulation once per tick
//! at `Stage::First`. [`Transport::receive`] is that same shape, so a
//! transport records and replays like every other source, and a handler never
//! runs off the frame loop.
//!
//! **One reliable channel, not many.** QUIC opens as many streams as it
//! likes, and the reason to want several is that a big message on one blocks
//! small ones behind it. Nothing here sends a big message yet, so the trait
//! carries one ordered channel and can grow a stream id when something does.

use anyhow::Result;

/// How a payload was sent, and therefore what was promised about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Delivery {
    /// Arrives, in order, or the link is broken.
    Reliable,
    /// May be dropped or reordered. What inputs travel as.
    Datagram,
}

/// One payload that arrived.
///
/// Serializable because a recording captures these verbatim: a session
/// replays from what the wire delivered, not from what it was decoded into.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Received {
    pub delivery: Delivery,
    pub bytes: Vec<u8>,
}

/// Where a link is in its life.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// Not usable yet; sends are refused rather than queued.
    Connecting,
    Open,
    /// Ended, with why. Terminal: a transport never reopens itself.
    Closed(String),
}

/// A link to one peer.
///
/// Implementations are expected to be cheap to poll and to do their real work
/// on a worker thread, which is why nothing here is async and nothing blocks.
pub trait Transport {
    /// Send bytes that must arrive, in order, on the one ordered channel.
    ///
    /// # Errors
    /// When the link is not open.
    fn send_reliable(&mut self, bytes: &[u8]) -> Result<()>;

    /// Send bytes that may be lost or reordered.
    ///
    /// A transport with no unreliable mode sends them reliably instead. That
    /// is a performance loss, never a correctness one: a receiver may not
    /// assume a datagram was dropped.
    ///
    /// # Errors
    /// When the link is not open, or the payload is over [`Transport::max_datagram`].
    fn send_datagram(&mut self, bytes: &[u8]) -> Result<()>;

    /// Everything that arrived since the last call, in arrival order.
    ///
    /// Empty is the common answer and is not an error.
    fn receive(&mut self) -> Vec<Received>;

    /// The largest datagram this link will carry.
    ///
    /// QUIC's is set by the path MTU and changes; a transport that fakes
    /// datagrams reports whatever its own frame limit is.
    fn max_datagram(&self) -> usize;

    fn state(&self) -> LinkState;

    /// Ask the link to close. [`Transport::state`] reports `Closed` once it
    /// has, which may be a later tick.
    fn close(&mut self);
}

/// The three ways a real link misbehaves.
///
/// Loopback does none of them: it never delays a payload, never drops one and
/// never reorders two. A session tested only against loopback has therefore
/// never exercised the property QUIC was chosen for, which is that losing one
/// input costs a misprediction instead of a stall.
#[derive(Clone, Copy, Debug)]
pub struct Faults {
    /// How many polls every payload waits before it is delivered.
    pub delay: u32,
    /// Up to this many extra polls, drawn per payload. Jitter is what
    /// reorders a stream — a fixed delay alone preserves order exactly.
    pub jitter: u32,
    /// The fraction of datagrams dropped, from 0.0 to 1.0.
    ///
    /// Datagrams only. A transport that loses a reliable payload has broken
    /// its contract, and a session must never be asked to cope with that.
    pub loss: f32,
}

impl Faults {
    /// A link that behaves. What loopback already is, spelled out.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            delay: 0,
            jitter: 0,
            loss: 0.0,
        }
    }

    /// A link across a continent: about 150 ms of round trip at 60 Hz, a
    /// little jitter, and one datagram in twenty lost.
    #[must_use]
    pub const fn typical() -> Self {
        Self {
            delay: 9,
            jitter: 3,
            loss: 0.05,
        }
    }
}

/// A [`Transport`] that misbehaves on purpose, wrapping one that does not.
///
/// Delay is counted in [`Transport::receive`] calls rather than in wall time.
/// A session polls once per tick, so nine polls is 150 ms at 60 Hz — and the
/// same run twice holds the same payloads for the same ticks, which a
/// `Duration` could not promise.
///
/// Only the inbound direction is disturbed, which costs no generality:
/// delaying what this peer sends is the same experiment as delaying what the
/// other peer receives, and a two-peer test can do either.
pub struct Faulty<T> {
    inner: T,
    faults: Faults,
    /// Payloads waiting for the poll they come out on.
    held: Vec<(u64, Received)>,
    polls: u64,
    rng: crate::rng::Pcg32,
}

impl<T: Transport> Faulty<T> {
    /// Wrap `inner`, drawing its dice from `seed` so a failing run repeats.
    #[must_use]
    pub fn new(inner: T, faults: Faults, seed: u64) -> Self {
        Self {
            inner,
            faults,
            held: Vec::new(),
            polls: 0,
            rng: crate::rng::Pcg32::new(seed),
        }
    }

    /// The transport underneath, once the experiment is over.
    pub fn into_inner(self) -> T {
        self.inner
    }

    /// A number in `0.0..1.0` from the same stream the delays come from.
    fn chance(&mut self) -> f32 {
        // 24 bits, which is every value an f32 can tell apart in this range.
        let bits = self.rng.next_u32() >> 8;
        bits as f32 / f32::from(1u16 << 8) / 65_536.0
    }
}

impl<T: Transport> Transport for Faulty<T> {
    fn send_reliable(&mut self, bytes: &[u8]) -> Result<()> {
        self.inner.send_reliable(bytes)
    }

    fn send_datagram(&mut self, bytes: &[u8]) -> Result<()> {
        self.inner.send_datagram(bytes)
    }

    fn receive(&mut self) -> Vec<Received> {
        self.polls += 1;
        for received in self.inner.receive() {
            if received.delivery == Delivery::Datagram && self.chance() < self.faults.loss {
                continue;
            }
            let jitter = if self.faults.jitter == 0 {
                0
            } else {
                self.rng.next_u32() % (self.faults.jitter + 1)
            };
            let due = self.polls + u64::from(self.faults.delay) + u64::from(jitter);
            self.held.push((due, received));
        }
        // By due poll, so jitter reorders; stable, so two payloads due on the
        // same poll keep the order they arrived in.
        self.held.sort_by_key(|(due, _)| *due);
        let ready = self.held.partition_point(|(due, _)| *due <= self.polls);
        self.held
            .drain(..ready)
            .map(|(_, received)| received)
            .collect()
    }

    fn max_datagram(&self) -> usize {
        self.inner.max_datagram()
    }

    fn state(&self) -> LinkState {
        self.inner.state()
    }

    fn close(&mut self) {
        self.inner.close();
    }
}

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Arrives, in order, or the link is broken.
    Reliable,
    /// May be dropped or reordered. What inputs travel as.
    Datagram,
}

/// One payload that arrived.
#[derive(Clone, Debug, PartialEq, Eq)]
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

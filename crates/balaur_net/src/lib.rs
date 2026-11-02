//! Balaur networking: a rollback session between peers, the transport it
//! speaks through, and the `rollback` script module. Rollback itself, a
//! tick resimulated from its snapshot, is the kernel's.

pub mod netsession;
pub mod rollback_api;
pub mod transport;

pub use netsession::{Desync, NetSession};
pub use transport::{Delivery, Faults, Faulty, LinkState, Received, Transport};

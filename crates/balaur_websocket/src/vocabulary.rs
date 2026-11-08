//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) const CLIENT_NO_CONTEXT_TAKEOVER: &str = "client_no_context_takeover";
    pub(crate) const PERMESSAGE_DEFLATE: &str = "permessage-deflate";
    pub(crate) const SERVER_MAX_WINDOW_BITS: &str = "server_max_window_bits";
    pub(crate) const SERVER_NO_CONTEXT_TAKEOVER: &str = "server_no_context_takeover";
}

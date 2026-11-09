//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) const ATTACH: &str = "attach";
    pub(crate) const CONTINUE: &str = "continue";
    pub(crate) const DISCONNECT: &str = "disconnect";
    pub(crate) const ERROR: &str = "error";
    pub(crate) const EVALUATE: &str = "evaluate";
    pub(crate) const INITIALIZE: &str = "initialize";
    pub(crate) const LAUNCH: &str = "launch";
    pub(crate) const NEXT: &str = "next";
    pub(crate) const OUTPUT: &str = "output";
    pub(crate) const PAUSE: &str = "pause";
    pub(crate) const SCOPES: &str = "scopes";
    pub(crate) const TERMINATE: &str = "terminate";
    pub(crate) const THREADS: &str = "threads";
    pub(crate) const VARIABLES: &str = "variables";
    pub(crate) const WARN: &str = "warn";
}

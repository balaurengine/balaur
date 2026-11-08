//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) const CHECK: &str = "check";
    pub(crate) const COMPLETE: &str = "complete";
    pub(crate) const DEFINITION: &str = "definition";
    pub(crate) const FIND: &str = "find";
    pub(crate) const FORMAT: &str = "format";
    pub(crate) const HOVER: &str = "hover";
    pub(crate) const REFERENCES: &str = "references";
    pub(crate) const RENAME: &str = "rename";
    pub(crate) const REPLACE: &str = "replace";
    pub(crate) const SIGNATURE: &str = "signature";
    pub(crate) const SYMBOLS: &str = "symbols";

    pub(crate) mod value {
        pub(crate) const NODE: &str = "node";
    }
}

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod keys {
    pub(crate) const LINE: &str = "line";
}

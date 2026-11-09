//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) const ASSET: &str = "asset";
    pub(crate) const COLOR: &str = "color";
    pub(crate) const DEFAULT: &str = "default";
    pub(crate) const DESCRIPTION: &str = "description";
    pub(crate) const FLAGS: &str = "flags";
    pub(crate) const NAME: &str = "name";
    pub(crate) const NODE: &str = "node";
    pub(crate) const TYPE: &str = "type";
}

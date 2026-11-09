//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) const ANDROID: &str = "android";
    pub(crate) const IOS: &str = "ios";
    pub(crate) const WASM: &str = "wasm";
}

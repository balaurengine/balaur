//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) const ALL_TIME: &str = "all_time";
    pub(crate) const FRIENDS: &str = "friends";
    pub(crate) const GLOBAL: &str = "global";
    pub(crate) const TODAY: &str = "today";
    pub(crate) const WEEK: &str = "week";
}

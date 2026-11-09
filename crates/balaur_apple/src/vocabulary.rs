//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) const ACHIEVEMENTS: &str = "achievements";
    pub(crate) const BOTTOM_LEADING: &str = "bottom_leading";
    pub(crate) const BOTTOM_TRAILING: &str = "bottom_trailing";
    pub(crate) const CHALLENGES: &str = "challenges";
    pub(crate) const DASHBOARD: &str = "dashboard";
    pub(crate) const DEFAULT: &str = "default";
    pub(crate) const FRIENDS: &str = "friends";
    pub(crate) const LEADERBOARDS: &str = "leaderboards";
    pub(crate) const PROFILE: &str = "profile";
    pub(crate) const TOP_LEADING: &str = "top_leading";
    pub(crate) const TOP_TRAILING: &str = "top_trailing";

    pub(crate) mod ui {
        pub(crate) const MACOS: &str = "macos";
    }
}

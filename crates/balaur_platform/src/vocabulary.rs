//! The words this crate compares and the keys it reads, each spelled once.

/// The keys a call's options and an answer's map carry.
#[allow(dead_code, reason = "a key's only reader may sit behind a target")]
pub(crate) mod keys {
    pub(crate) const AFTER: &str = "after";
    pub(crate) const ALLOWED: &str = "allowed";
    pub(crate) const BODY: &str = "body";
    pub(crate) const DATA: &str = "data";
    pub(crate) const FILE: &str = "file";
    pub(crate) const ID: &str = "id";
    pub(crate) const START: &str = "start";
    pub(crate) const TARGET: &str = "target";
    pub(crate) const TEXT: &str = "text";
    pub(crate) const TITLE: &str = "title";
}

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) const ALL_TIME: &str = "all_time";
    pub(crate) const FRIENDS: &str = "friends";
    pub(crate) const GLOBAL: &str = "global";
    pub(crate) const TODAY: &str = "today";
    pub(crate) const WEEK: &str = "week";

    /// `device.haptic`'s effects, as the Java shim reads them too.
    pub(crate) mod haptics {
        pub(crate) const LIGHT: &str = "light";
        pub(crate) const MEDIUM: &str = "medium";
        pub(crate) const HEAVY: &str = "heavy";
        pub(crate) const SELECTION: &str = "selection";
        pub(crate) const SUCCESS: &str = "success";
        pub(crate) const WARNING: &str = "warning";
    }

    /// `platform.is_available`'s services, as the Java shim reads them too.
    pub(crate) mod services {
        pub(crate) const STEAM: &str = "steam";
        pub(crate) const GAME_CENTER: &str = "game_center";
        pub(crate) const PLAY_GAMES: &str = "play_games";
        pub(crate) const ICLOUD: &str = "icloud";
        pub(crate) const APPLE_SIGN_IN: &str = "apple_sign_in";
        pub(crate) const GOOGLE_SIGN_IN: &str = "google_sign_in";
        pub(crate) const PUSH: &str = "push";
        pub(crate) const PURCHASES: &str = "purchases";
        pub(crate) const HAPTICS: &str = "haptics";
        pub(crate) const SHARE: &str = "share";
        pub(crate) const REVIEW: &str = "review";
    }
}

//! The words this crate compares and the keys it reads, each spelled once.

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod words {
    pub(crate) const CONNECT: &str = "connect";
    pub(crate) const EXPIRES_IN: &str = "expires_in";
    pub(crate) const LEAVE: &str = "leave";
    pub(crate) const OK: &str = "ok";
    pub(crate) const PHOENIX: &str = "phoenix";
    pub(crate) const PHX_CLOSE: &str = "phx_close";
    pub(crate) const PHX_ERROR: &str = "phx_error";
    pub(crate) const PHX_REPLY: &str = "phx_reply";

    pub(crate) mod auth {
        pub(crate) const USERNAME: &str = "username";
    }

    pub(crate) mod functions {
        pub(crate) const CALL_HOOK: &str = "call_hook";
        pub(crate) const REPLY: &str = "reply";
    }

    pub(crate) mod general {
        pub(crate) const JOIN: &str = "join";
    }

    pub(crate) mod inspect {
        pub(crate) const ACCESS_TOKEN: &str = "access_token";
        pub(crate) const DISPLAY_NAME: &str = "display_name";
        pub(crate) const REFRESH_TOKEN: &str = "refresh_token";
        pub(crate) const USER_ID: &str = "user_id";
    }

    pub(crate) mod rest {
        pub(crate) const AUTHORIZATION: &str = "authorization";
        pub(crate) const CONTENT_TYPE: &str = "content-type";
    }

    pub(crate) mod target {
        pub(crate) const URL: &str = "url";
    }
}

#[allow(dead_code, reason = "a word's only reader may sit behind a feature")]
pub(crate) mod keys {
    pub(crate) const KIND: &str = "kind";
    pub(crate) const REASON: &str = "reason";
    pub(crate) const REQUEST: &str = "request";
    pub(crate) const SOCKET: &str = "socket";
    pub(crate) const STATUS: &str = "status";
}

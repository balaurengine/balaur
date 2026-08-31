//! Login and token refresh.
//!
//! Gamend issues a short-lived access token (`expires_in` seconds, 15
//! minutes) and a 30-day refresh token. [`crate::rest::Client::call`]
//! refreshes and retries once on a 401, so callers rarely touch this module
//! after login.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::rest::{required, Client};

/// A logged-in identity: the token pair plus who it belongs to.
///
/// String fields follow the server's convention: unset is `""`, never null.
#[derive(Clone, Debug)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    /// Access-token lifetime in seconds. The refresh token's own 30-day
    /// lifetime is not reported by the server.
    pub expires_in: u64,
}

pub enum Credentials {
    EmailPassword {
        email: String,
        password: String,
    },
    /// Any string; an unknown id creates an anonymous account (when the
    /// server has device auth enabled, its default).
    Device {
        device_id: String,
    },
}

/// Log in and store the session on the client for every later call.
pub fn login(client: &mut Client, credentials: &Credentials) -> Result<Session> {
    let (path, body) = match credentials {
        Credentials::EmailPassword { email, password } => (
            "/api/v1/login",
            json!({ "email": email, "password": password }),
        ),
        Credentials::Device { device_id } => {
            ("/api/v1/login/device", json!({ "device_id": device_id }))
        }
    };
    let reply = client.call_raw("POST", path, Some(&body), false)?;
    let session = session_of(&reply.body, reply.status, "login")?;
    client.set_session(Some(session.clone()));
    Ok(session)
}

/// Trade a refresh token for a fresh access token. The refresh token itself
/// is echoed back unchanged by the server.
pub fn refresh(client: &Client, refresh_token: &str) -> Result<Session> {
    let body = json!({ "refresh_token": refresh_token });
    let reply = client.call_raw("POST", "/api/v1/refresh", Some(&body), false)?;
    session_of(&reply.body, reply.status, "refresh")
}

/// The `{"data": {...}}` envelope both login and refresh reply with.
fn session_of(body: &Value, status: u16, what: &str) -> Result<Session> {
    if status != 200 {
        let error = body
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        return Err(anyhow!("{what} failed ({status}): {error}"));
    }
    Ok(Session {
        access_token: required(body, &["data", "access_token"], what)?.to_string(),
        refresh_token: required(body, &["data", "refresh_token"], what)?.to_string(),
        user_id: required(body, &["data", "user_id"], what)?.to_string(),
        username: required(body, &["data", "username"], what)?.to_string(),
        display_name: required(body, &["data", "display_name"], what)?.to_string(),
        expires_in: body
            .pointer("/data/expires_in")
            .and_then(Value::as_u64)
            .unwrap_or(900),
    })
}

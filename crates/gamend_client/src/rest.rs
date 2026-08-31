//! The REST layer: one authenticated JSON caller for the whole API surface.
//!
//! Deliberately generic — Gamend's API is large and uniform (JSON in, JSON
//! out, Bearer auth), so a `(method, path, body)` caller covers all of it
//! without a generated model layer. Typed helpers live in [`crate::auth`]
//! and can grow beside it.

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use serde_json::Value;

use crate::auth::Session;

/// What a call produced: the status and the decoded JSON body (`Null` when
/// the body is empty or not JSON).
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub body: Value,
}

pub struct Client {
    base_url: String,
    agent: ureq::Agent,
    session: Option<Session>,
}

impl Client {
    /// `base_url` is the server root, e.g. `http://localhost:4000` — paths
    /// are appended verbatim.
    pub fn new(base_url: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            // A 4xx or 5xx is a reply the caller must see, not a transport
            // failure.
            .http_status_as_error(false)
            .build()
            .into();
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            agent,
            session: None,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    pub fn set_session(&mut self, session: Option<Session>) {
        self.session = session;
    }

    /// One authenticated API call. On a 401 with a refresh token in hand, the
    /// session is refreshed once and the call retried, so an expired access
    /// token heals invisibly.
    pub fn call(&mut self, method: &str, path: &str, body: Option<&Value>) -> Result<Reply> {
        let reply = self.call_raw(method, path, body, true)?;
        if reply.status != 401
            || self
                .session
                .as_ref()
                .is_none_or(|s| s.refresh_token.is_empty())
        {
            return Ok(reply);
        }
        let session = crate::auth::refresh(self, &self.refresh_token())?;
        self.session = Some(session);
        self.call_raw(method, path, body, true)
    }

    fn refresh_token(&self) -> String {
        self.session
            .as_ref()
            .map(|s| s.refresh_token.clone())
            .unwrap_or_default()
    }

    /// The call itself, no refresh logic. `authenticated` controls the Bearer
    /// header; login and refresh go without it.
    pub(crate) fn call_raw(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        authenticated: bool,
    ) -> Result<Reply> {
        let url = format!("{}{}", self.base_url, path);
        let token = self
            .session
            .as_ref()
            .filter(|_| authenticated)
            .map(|s| format!("Bearer {}", s.access_token));
        let request_body = body.map(Value::to_string);
        let mut response = match method {
            "GET" | "DELETE" => {
                let mut r = if method == "GET" {
                    self.agent.get(&url)
                } else {
                    self.agent.delete(&url)
                };
                if let Some(token) = &token {
                    r = r.header("authorization", token);
                }
                r.call()?
            }
            "POST" | "PUT" | "PATCH" => {
                let mut r = match method {
                    "POST" => self.agent.post(&url),
                    "PUT" => self.agent.put(&url),
                    _ => self.agent.patch(&url),
                };
                if let Some(token) = &token {
                    r = r.header("authorization", token);
                }
                r.header("content-type", "application/json")
                    .send(request_body.as_deref().unwrap_or("{}"))?
            }
            other => bail!("unsupported method `{other}`"),
        };
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string()?;
        let body = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok(Reply { status, body })
    }
}

/// Pull a required string field out of a reply body, with a message that
/// names what was being read when it is missing.
pub(crate) fn required<'a>(body: &'a Value, path: &[&str], what: &str) -> Result<&'a str> {
    let mut cursor = body;
    for key in path {
        cursor = cursor
            .get(key)
            .ok_or_else(|| anyhow!("{what}: no `{}` in {body}", path.join(".")))?;
    }
    cursor
        .as_str()
        .ok_or_else(|| anyhow!("{what}: `{}` is not a string", path.join(".")))
}

//! The REST layer: one authenticated JSON caller for the whole API surface.
//!
//! Deliberately generic — Gamend's API is large and uniform (JSON in, JSON
//! out, Bearer auth), so a `(method, path, body)` caller covers all of it
//! without a generated model layer. Typed helpers live in [`super::auth`]
//! and can grow beside it.

use anyhow::{Result, anyhow};
use serde_json::Value;

use super::auth::Session;

/// What a call produced: the status and the decoded JSON body (`Null` when
/// the body is empty or not JSON).
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub body: Value,
}

/// One call as the transport sends it: what [`Client::prepare`] produces
/// and both the blocking and the browser transports consume.
pub(crate) struct Prepared {
    pub(crate) method: String,
    pub(crate) url: String,
    /// The `Authorization` header's value, when the call is authenticated.
    pub(crate) bearer: Option<String>,
    pub(crate) body: Option<String>,
}

pub struct Client {
    base_url: String,
    #[cfg(not(target_family = "wasm"))]
    agent: ureq::Agent,
    session: Option<Session>,
}

impl Client {
    /// `base_url` is the server root, e.g. `http://localhost:4000` — paths
    /// are appended verbatim.
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            #[cfg(not(target_family = "wasm"))]
            agent: agent(),
            session: None,
        }
    }

    /// The request a `(method, path, body)` call becomes, before any
    /// transport touches it. `authenticated` controls the Bearer header;
    /// login and refresh go without it.
    pub(crate) fn prepare(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        authenticated: bool,
    ) -> Prepared {
        Prepared {
            method: method.to_string(),
            url: format!("{}{}", self.base_url, path),
            bearer: self
                .session
                .as_ref()
                .filter(|_| authenticated)
                .map(|s| format!("Bearer {}", s.access_token)),
            body: body.map(Value::to_string),
        }
    }

    /// The refresh token in hand, or empty.
    pub(crate) fn refresh_token(&self) -> String {
        self.session
            .as_ref()
            .map(|s| s.refresh_token.clone())
            .unwrap_or_default()
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
    #[cfg(not(target_family = "wasm"))]
    pub fn call(&mut self, method: &str, path: &str, body: Option<&Value>) -> Result<Reply> {
        let reply = self.call_raw(method, path, body, true)?;
        if reply.status != 401 || self.refresh_token().is_empty() {
            return Ok(reply);
        }
        let session = super::auth::refresh(self, &self.refresh_token())?;
        self.session = Some(session);
        self.call_raw(method, path, body, true)
    }

    /// The call itself, no refresh logic.
    #[cfg(not(target_family = "wasm"))]
    pub(crate) fn call_raw(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        authenticated: bool,
    ) -> Result<Reply> {
        let prepared = self.prepare(method, path, body, authenticated);
        let mut response = match prepared.method.as_str() {
            "GET" | "DELETE" => {
                let mut r = if prepared.method == "GET" {
                    self.agent.get(&prepared.url)
                } else {
                    self.agent.delete(&prepared.url)
                };
                if let Some(token) = &prepared.bearer {
                    r = r.header("authorization", token);
                }
                r.call()?
            }
            "POST" | "PUT" | "PATCH" => {
                let mut r = match prepared.method.as_str() {
                    "POST" => self.agent.post(&prepared.url),
                    "PUT" => self.agent.put(&prepared.url),
                    _ => self.agent.patch(&prepared.url),
                };
                if let Some(token) = &prepared.bearer {
                    r = r.header("authorization", token);
                }
                r.header("content-type", "application/json")
                    .send(prepared.body.as_deref().unwrap_or("{}"))?
            }
            other => anyhow::bail!("unsupported method `{other}`"),
        };
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string()?;
        Ok(reply_of(status, &text))
    }
}

/// A reply from its status and body text; a body that is not JSON is `Null`.
pub(crate) fn reply_of(status: u16, text: &str) -> Reply {
    Reply {
        status,
        body: serde_json::from_str(text).unwrap_or(Value::Null),
    }
}

#[cfg(not(target_family = "wasm"))]
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        // A 4xx or 5xx is a reply the caller must see, not a transport
        // failure.
        .http_status_as_error(false)
        .build()
        .into()
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

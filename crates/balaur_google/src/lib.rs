//! Google services as a Balaur plugin: `google.*`. Today that is Sign in
//! with Google on Android, through Android's own Credential Manager, with no
//! Google library in the APK (`docs/PLAN-google.md` step 4).
//!
//! ```bs
//! let answer = await task::wait(google::sign_in());
//! if answer["kind"].text() == google::EVENT_SIGNED_IN {
//!     await client::login_google(answer["token"].text());
//! }
//! ```
//!
//! The token is issued for `[google] server_client_id`, the server's web
//! client id, which is what a server checks it against. A call answers
//! `task::wait` on a later tick, through [`ExternalIo`], so it is recorded and
//! a replay plays it back.
//!
//! Anywhere but Android, below Android 14, with no client id set, or with no
//! Google account on the device, the call answers `unsupported` and a game
//! opens Google's page instead. A test
//! answers it from its stand-in file, `"google.sign_in"`
//! (`balaur_platform::stand_ins`).

use anyhow::Result;
use balaur_core::engine_api::from_json;
use balaur_core::handler::id_value;
use balaur_core::replay::ExternalIo;
use balaur_core::{Engine, Stage};
use balaur_script::{Module, Value};
use serde::{Deserialize, Serialize};

const GOOGLE: &str = "google";
const KIND: &str = "kind";
const REQUEST: &str = "request";
const SIGN_IN: &str = "sign_in";

/// The setting that holds the web client id a token is issued for.
pub const SERVER_CLIENT_ID: &str = "google/server_client_id";

#[cfg(target_os = "android")]
mod android;

#[cfg(target_os = "android")]
mod backend {
    pub(crate) use crate::android::sign_in;
}

/// Keep the Java `Activity` Credential Manager shows its sheet over; the
/// facade hands it over as the app is built.
#[cfg(target_os = "android")]
pub use android::keep_activity;

/// Everywhere else: no Credential Manager, so sign-in answers `unsupported`.
#[cfg(not(target_os = "android"))]
mod backend {
    use std::sync::mpsc::Sender;

    use crate::GoogleEvent;

    pub(crate) fn sign_in(request: u64, _server_client_id: &str, report: &Sender<GoogleEvent>) {
        let _ = report.send(GoogleEvent::unsupported(request, "this is not Android"));
    }
}

/// The `kind` each answer carries, and the `EVENT_*` constants of it. The
/// Java shim spells the same words.
pub mod kind {
    pub const SIGNED_IN: &str = "signed_in";
    pub const CANCELLED: &str = "cancelled";
    pub const UNSUPPORTED: &str = "unsupported";
    pub const ERROR: &str = balaur_core::handler::ERROR;
    pub const ALL: &[&str] = &[SIGNED_IN, CANCELLED, UNSUPPORTED, ERROR];
}

/// One answer to a `google.*` call: the map its waiter receives, `kind` and
/// all, from the Java shim, a stand-in or this crate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GoogleEvent {
    pub request: u64,
    pub answer: serde_json::Value,
}

impl GoogleEvent {
    /// An answer of `unsupported`, with the reason a log and a player see.
    #[must_use]
    pub fn unsupported(request: u64, why: &str) -> Self {
        Self {
            request,
            answer: serde_json::json!({ KIND: kind::UNSUPPORTED, kind::ERROR: why }),
        }
    }

    /// An answer of `error`, with the message a log and a player see.
    #[must_use]
    pub fn failed(request: u64, message: &str) -> Self {
        Self {
            request,
            answer: serde_json::json!({ KIND: kind::ERROR, kind::ERROR: message }),
        }
    }
}

/// The `google.*` calls in flight, and the channel their answers cross.
#[derive(Default)]
pub struct GoogleState {
    io: ExternalIo<GoogleEvent>,
}

impl GoogleState {
    /// Sign in with Google under `id`, an [`Engine::next_token`] value.
    pub fn sign_in(&self, eng: &Engine, id: u64) {
        balaur_core::replay::event(
            eng,
            "google.call",
            SIGN_IN.to_string(),
            Some(serde_json::json!({ "id": id, "call": SIGN_IN })),
        );
        if let Some(answer) = balaur_platform::stand_in(eng, GOOGLE, SIGN_IN) {
            self.io.start(eng, |report| {
                if let Some(answer) = answer {
                    let _ = report.send(GoogleEvent {
                        request: id,
                        answer,
                    });
                }
            });
            return;
        }
        let client = server_client_id(eng);
        self.io.start(eng, |report| {
            if client.is_empty() {
                let _ = report.send(GoogleEvent::unsupported(
                    id,
                    "no [google] server_client_id in project.toml",
                ));
            } else {
                backend::sign_in(id, &client, report);
            }
        });
    }
}

/// `[google] server_client_id`, or "" when the project sets none.
#[must_use]
pub fn server_client_id(eng: &Engine) -> String {
    balaur_core::settings::get(eng, SERVER_CLIENT_ID)
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// This tick's `google.*` answers, as their waiters received them.
#[derive(Default)]
pub struct GoogleSnapshot {
    pub events: Vec<Value>,
}

fn pump_google_system(eng: &Engine, _: f32) {
    let finished: Vec<(u64, Value)> = eng
        .resource::<GoogleState>()
        .borrow_mut()
        .io
        .drain()
        .into_iter()
        .map(|event| (event.request, event_value(&event)))
        .collect();
    eng.resource::<GoogleSnapshot>().borrow_mut().events = finished
        .iter()
        // copy: the snapshot keeps what each waiter is handed.
        .map(|(_, value)| value.clone())
        .collect();
    balaur_core::handler::wake_all(eng, finished);
}

fn event_value(event: &GoogleEvent) -> Value {
    let mut pairs = vec![(REQUEST.into(), id_value(event.request))];
    if let Ok(Value::Map(fields)) = from_json(&event.answer) {
        pairs.extend(fields.into_iter().filter(|(key, _)| key != REQUEST));
    }
    Value::Map(pairs)
}

#[derive(Default)]
pub struct GooglePlugin;

impl balaur_plugin::Plugin for GooglePlugin {
    fn name(&self) -> &'static str {
        GOOGLE
    }

    fn requires(&self) -> Vec<String> {
        // Its stand-ins are the platform plugin's.
        vec!["platform".to_string()]
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        reg.insert_resource(GoogleState::default());
        reg.insert_resource(GoogleSnapshot::default());
        reg.add_system(Stage::First, pump_google_system);
        reg.add_replay_source(GOOGLE, capture, restore);
        balaur_core::settings::define_group(
            reg.engine(),
            GOOGLE,
            balaur_core::settings::Scope::Project,
            &balaur_core::ComponentDef::parse_schema(
                "settings.google",
                r#"
server_client_id = { type = "string", default = "", help = "The server's web OAuth client id (`….apps.googleusercontent.com`), which an id token is issued for. Not a secret. Empty: `google.sign_in` answers `unsupported`." }
"#,
            ),
        );
        let mut m = reg.script_module(GOOGLE)?;
        install_google_api(&mut m);
        Ok(())
    }
}

fn capture(eng: &Engine) -> serde_json::Value {
    eng.resource::<GoogleState>().borrow().io.capture()
}

fn restore(eng: &Engine, value: &serde_json::Value) {
    eng.resource::<GoogleState>().borrow().io.restore(value);
}

/// The script module, whose one call today is `sign_in`.
fn install_google_api(m: &mut Module<Engine>) {
    m.module_doc(
        "Google services: Sign in with Google on Android 14 and later, through the system's Credential Manager. A call answers `task::wait` on a later tick as a map whose `kind` is an `EVENT_*` constant.",
    );
    balaur_core::handler::install_event_kinds(m, kind::ALL);
    m.describe(&[(
        SIGN_IN,
        &[],
        "() -> int",
        "Sign in with Google, and return the id the answer carries: `signed_in` with `token` (an id token for `[google] server_client_id`), `email` and `name`; `cancelled`; `unsupported` off Android, below Android 14, with no client id or with no Google account on the device; or `error`. The accounts already signed in to the app come first, without a tap when there is one.",
    )]);
    m.function(SIGN_IN, |eng: &Engine, ()| {
        let id = eng.next_token();
        eng.resource::<GoogleState>().borrow().sign_in(eng, id);
        Ok(id_value(id))
    });
}

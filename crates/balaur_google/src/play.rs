//! Play's services behind `platform.*` on Android: Play Games Services v2
//! for the player, achievements, leaderboards, saved games and the server
//! code, and Play Billing for purchases. Each is a Java shim in the template
//! (`play/<capability>`), carried by a game whose `[android] capabilities`
//! names it, and reached through `balaur_platform`'s capability call: a call
//! goes out as its name and a JSON map, and comes back as the map its waiter
//! receives. Play Games signs the player in by itself as the game starts,
//! which arrives under request 0 as `platform_event`.

use balaur_platform::Call;

/// The shim behind the player, achievements, leaderboards and saved games.
pub const GAMES: &str = "org.balaur.google.Games";
/// The shim behind purchases.
pub const BILLING: &str = "org.balaur.google.Billing";
/// What the backend names itself, `platform.backend()` on Android.
pub const BACKEND: &str = "google";
/// The call Play Games' automatic sign-in starts with.
pub const BEGIN: &str = "begin";

/// The shim a call goes to and the arguments it takes, or `None` for one
/// Play has no answer to (presence) or that is not Play's (a sign-in page).
#[must_use]
pub fn route(call: &Call, server_client_id: &str) -> Option<(&'static str, serde_json::Value)> {
    use serde_json::json;
    let games = |args| Some((GAMES, args));
    let billing = |args| Some((BILLING, args));
    match call {
        Call::SignIn => games(json!({})),
        Call::Unlock { achievement } => games(json!({ "achievement": achievement })),
        Call::Progress {
            achievement,
            percent,
        } => games(json!({ "achievement": achievement, "percent": percent })),
        Call::SubmitScore { board, score } => games(json!({ "board": board, "score": score })),
        Call::Scores {
            board,
            count,
            scope,
            period,
            start,
        } => games(json!({
            "board": board,
            "count": count,
            "start": start,
            "scope": scope.name(),
            "period": period.name(),
        })),
        Call::CloudRead { key } => games(json!({ "key": key })),
        Call::CloudWrite { key, value } => games(json!({ "key": key, "value": value })),
        Call::Identity => games(json!({ "server_client_id": server_client_id })),
        Call::Products { ids } => billing(json!({ "ids": ids })),
        Call::Purchase { product } => billing(json!({ "product": product })),
        Call::FinishPurchase {
            transaction,
            consume,
        } => billing(json!({ "transaction": transaction, "consume": consume })),
        Call::RestorePurchases | Call::Entitlements => billing(json!({})),
        Call::SetPresence { .. } | Call::WebSignIn { .. } => None,
    }
}

/// What a call answers when the game's export left its capability out.
#[must_use]
pub fn left_out(class: &str) -> &'static str {
    if class == BILLING {
        "this export carries no Play Billing: add \"in-app-purchase\" to [android] capabilities"
    } else {
        "this export carries no Play Games: add \"play-games\" to [android] capabilities"
    }
}

#[cfg(target_os = "android")]
pub(crate) mod android {
    use std::sync::mpsc::Sender;
    use std::sync::{Mutex, PoisonError};

    use balaur_platform::{Call, PlatformBackend, PlatformEvent};

    /// `[google] server_client_id`, which the plugin's pump keeps current:
    /// a backend has no engine to read the setting from.
    pub(crate) static SERVER_CLIENT_ID: Mutex<String> = Mutex::new(String::new());

    /// Play behind `platform.*`.
    pub(crate) struct Play {
        begun: bool,
    }

    impl Play {
        pub(crate) const fn new() -> Self {
            Self { begun: false }
        }
    }

    impl PlatformBackend for Play {
        fn name(&self) -> &'static str {
            super::BACKEND
        }

        fn serves(&self, call: &Call) -> bool {
            !matches!(call, Call::WebSignIn { .. })
        }

        fn start(&mut self, request: u64, call: &Call, report: &Sender<PlatformEvent>) {
            let client = SERVER_CLIENT_ID
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            let Some((class, args)) = super::route(call, &client) else {
                let _ = report.send(PlatformEvent::Unsupported {
                    request,
                    call: call.name().to_string(),
                });
                return;
            };
            match balaur_platform::start_capability(class, request, call.name(), &args, report) {
                Ok(true) => {}
                Ok(false) => {
                    let _ = report.send(PlatformEvent::Answer {
                        request,
                        answer: serde_json::json!({
                            "kind": balaur_platform::kind::UNSUPPORTED,
                            "call": call.name(),
                            "error": super::left_out(class),
                        }),
                    });
                }
                Err(err) => {
                    let _ = report.send(PlatformEvent::Failed {
                        request,
                        message: format!("{}: {err}", call.name()),
                    });
                }
            }
        }

        /// Play Games signs in by itself once asked to begin: the first pump
        /// that may reach the outside world.
        fn pump(&mut self, report: &Sender<PlatformEvent>) {
            if self.begun {
                return;
            }
            self.begun = true;
            let args = serde_json::json!({});
            if let Err(err) =
                balaur_platform::start_capability(super::GAMES, 0, super::BEGIN, &args, report)
            {
                tracing::info!("Play Games: {err}");
            }
        }
    }
}

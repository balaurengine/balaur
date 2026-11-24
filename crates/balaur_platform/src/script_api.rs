//! `platform.*` as scripts call it: each function starts a [`Call`] and
//! answers the id its waiter hears.

use anyhow::anyhow;
use balaur_core::Engine;
use balaur_core::handler::opt;
use balaur_script::{Module, Value};

use crate::vocabulary::keys as k;
use crate::{
    BACKEND, CLOSE_WEB_SIGN_IN, CLOUD_READ, CLOUD_WRITE, Call, DEFAULT_SCORES, IDENTITY, PAGE_NONE,
    PAGE_OPEN, PLATFORM, PLAYER, PROGRESS, Period, PlatformState, SET_PRESENCE, SIGN_IN,
    SUBMIT_SCORE, Scope, UNLOCK, WEB_SIGN_IN_STATE, counted, device, kind, number, player_value,
    purchases, services, stand_in, start_call, text,
};

/// `platform.*`. Declared against the backend seam, so the same script runs
/// with a store, without one, and inside a replay.
pub(super) fn install(m: &mut Module<Engine>) {
    m.module_doc(
        "Store services every platform shares: sign-in, achievements, leaderboards, cloud saves, purchases, a sign-in page in the OS's own sheet, local notifications and push. A call answers `task::wait` later with a map whose `kind` is `signed_in`, `signed_out`, `done`, `scores`, `read`, `identity`, `web_sign_in`, `notifications`, `scheduled`, `push_registered`, `push_failed`, `products`, `purchased`, `pending`, `entitlements`, `restored`, `finished`, `cancelled`, `error` or `unsupported`, each an `EVENT_*` constant. What arrives unasked is the `platform_event` event, `events::listen(node, \"platform_event\")`: a sign-in or sign-out, a notification or a push tapped (`notification_opened`), one shown in front (`notification_received`) or a push that reached the game in front (`push_received`), a push token the OS replaced (`push_token_changed`), and a purchase that landed with nobody asking (`transaction`). `is_available` says whether a service works here. A test answers calls from the file `BALAUR_STAND_INS` names.",
    );
    balaur_core::handler::install_event_kinds(m, kind::ALL);
    m.records(&[
        (
            "Player",
            "{ id: string, alias: string }",
            "Who the store says is playing: its stable `id` on this platform, and the `alias` to show.",
        ),
        (
            "Score",
            "{ player: string, alias: string, rank: int, score: int }",
            "One row of a leaderboard, as a `scores` answer's `entries` hold it: `let rows: [platform::Score] = reply[\"entries\"];`.",
        ),
        (
            "ScoresOptions",
            "{ count: int?, start: int?, scope: string?, period: string? }",
            "Which entries `scores` reads: how many, the rank to read from (from 1), `scope` (\"global\" or \"friends\") and `period` (\"today\", \"week\" or \"all_time\").",
        ),
    ]);
    m.describe(&[
        (
            BACKEND,
            &[],
            "() -> string",
            "The name of the loaded store, or \"none\".",
        ),
        (
            PLAYER,
            &[],
            "() -> Player?",
            "Who the store says is playing, or nil before a sign-in has landed.",
        ),
        (
            SIGN_IN,
            &[],
            "() -> int",
            "Ask the store who is playing, and return the id its answer carries. A later sign-out is the `platform_event` event.",
        ),
        (
            UNLOCK,
            &[],
            "(achievement: string) -> int",
            "Award an achievement whole, and return the id its answer carries.",
        ),
        (
            PROGRESS,
            &[],
            "(achievement: string, percent: float) -> int",
            "Report an achievement's completion, 0 to 100, and return the id its answer carries.",
        ),
        (
            SUBMIT_SCORE,
            &[],
            "(board: string, score: int) -> int",
            "Post a score to a leaderboard, and return the id its answer carries.",
        ),
        (
            kind::SCORES,
            &[],
            "(board: string, opts: ScoresOptions?) -> int",
            "Read a leaderboard's entries, and return the id whose `scores` answer holds them as `entries`, each a `Score`.",
        ),
        (
            CLOUD_READ,
            &[],
            "(key: string) -> int",
            "Read a value the store syncs between the player's devices, and return the id whose `read` answer holds it as `value`: a string, or nil when the key holds nothing. On Play Games it is a saved game named `key`, and a conflict between two devices' saves resolves to the one modified last.",
        ),
        (
            CLOUD_WRITE,
            &[],
            "(key: string, value: string) -> int",
            "Write a value for the store to sync between the player's devices, and return the id its answer carries. On Play Games it is a saved game named `key` (letters, digits and `-._~`, up to 100), and a save another device made that Play has not reconciled with this one's is refused with `error`, never overwritten: read it, then write again.",
        ),
        (
            IDENTITY,
            &[],
            "() -> int",
            "Ask the store for a proof of who is playing that a server checks with it, and return the id whose `identity` answer carries it: Steam's Web API ticket as `ticket`, hex, for `ISteamUserAuth/AuthenticateUserTicket`; Play Games' one-time server code as `code`, for `[google] server_client_id`, which the server exchanges with Google.",
        ),
        (
            SET_PRESENCE,
            &[],
            "(text: string) -> int",
            "Say what the player is doing, where the store shows it, and return the id its answer carries.",
        ),
    ]);
    install_platform_reads(m);
    install_platform_session(m);
    install_platform_achievements(m);
    install_platform_leaderboards(m);
    install_platform_cloud(m);
    install_web_sign_in(m);
    purchases::install(m);
    services::install(m);
    device::install_notifications(m);
}

/// What a script can ask without waiting: which store is loaded, and who it
/// says is playing.
fn install_platform_reads(m: &mut Module<Engine>) {
    m.function(BACKEND, |eng: &Engine, ()| {
        // A test names the store it plays against (`"platform.backend"`).
        if let Some(Some(serde_json::Value::String(named))) = stand_in(eng, PLATFORM, BACKEND) {
            return Ok(Value::Str(named));
        }
        Ok(Value::Str(
            eng.resource::<PlatformState>()
                .borrow()
                .backend_name()
                .into(),
        ))
    });
    m.function(PLAYER, |eng: &Engine, ()| {
        let state = eng.resource::<PlatformState>();
        let state = state.borrow();
        Ok(state.player().map_or(Value::Nil, player_value))
    });
}

/// Signing in, and saying what the player is doing.
fn install_platform_session(m: &mut Module<Engine>) {
    m.function(SIGN_IN, |eng: &Engine, ()| {
        Ok(start_call(eng, Call::SignIn))
    });
    m.function(IDENTITY, |eng: &Engine, ()| {
        Ok(start_call(eng, Call::Identity))
    });
    m.function(SET_PRESENCE, |eng: &Engine, presence: Value| {
        let text = text(Some(&presence), "what the player is doing")?;
        Ok(start_call(eng, Call::SetPresence { text }))
    });
}

fn install_platform_achievements(m: &mut Module<Engine>) {
    m.function(UNLOCK, |eng: &Engine, achievement: Value| {
        let achievement = text(Some(&achievement), "an achievement id")?;
        Ok(start_call(eng, Call::Unlock { achievement }))
    });
    m.function(
        PROGRESS,
        |eng: &Engine, (achievement, percent): (Value, Value)| {
            let achievement = text(Some(&achievement), "an achievement id")?;
            let percent = number(Some(&percent), "a percentage")?;
            Ok(start_call(
                eng,
                Call::Progress {
                    achievement,
                    percent,
                },
            ))
        },
    );
}

fn install_platform_leaderboards(m: &mut Module<Engine>) {
    m.function(
        SUBMIT_SCORE,
        |eng: &Engine, (board, score): (Value, Value)| {
            let board = text(Some(&board), "a leaderboard id")?;
            #[allow(clippy::cast_possible_truncation, reason = "a leaderboard score")]
            let score = number(Some(&score), "a score")? as i64;
            Ok(start_call(eng, Call::SubmitScore { board, score }))
        },
    );
    m.function(
        kind::SCORES,
        |eng: &Engine, (board, opts): (Value, Option<Value>)| {
            let board = text(Some(&board), "a leaderboard id")?;
            let opts = opts.as_ref();
            let count = counted(opts, "count", DEFAULT_SCORES)?;
            let start = counted(opts, k::START, 1)?.max(1);
            let scope = match opt(opts, "scope") {
                Some(Value::Str(text)) => Scope::parse(text)?,
                Some(other) => return Err(anyhow!("`scope` should be a string, got {other:?}")),
                None => Scope::default(),
            };
            let period = match opt(opts, "period") {
                Some(Value::Str(text)) => Period::parse(text)?,
                Some(other) => return Err(anyhow!("`period` should be a string, got {other:?}")),
                None => Period::default(),
            };
            Ok(start_call(
                eng,
                Call::Scores {
                    board,
                    count,
                    scope,
                    period,
                    start,
                },
            ))
        },
    );
}

fn install_platform_cloud(m: &mut Module<Engine>) {
    m.function(CLOUD_READ, |eng: &Engine, key: Value| {
        let key = text(Some(&key), "a key")?;
        Ok(start_call(eng, Call::CloudRead { key }))
    });
    m.function(CLOUD_WRITE, |eng: &Engine, (key, value): (Value, Value)| {
        let key = text(Some(&key), "a key")?;
        let value = text(Some(&value), "a value")?;
        Ok(start_call(eng, Call::CloudWrite { key, value }))
    });
}

/// A provider's sign-in page in the OS's own sheet, and shutting it.
fn install_web_sign_in(m: &mut Module<Engine>) {
    m.records(&[(
            "WebSignInOptions",
            "{ scheme: string? }",
            "The custom scheme a sign-in page ends on (`com.example.app` for `com.example.app:/done`): the sheet shuts there and its `web_sign_in` answer carries that URL as `url`. On Android the export declares it in `[android] schemes`, the intent filter a Custom Tab's redirect lands on.",
    )]);
    m.describe(&[
        (
            kind::WEB_SIGN_IN,
            &[],
            "(url: string, opts: WebSignInOptions?) -> int",
            "Open a sign-in page in the OS's own sheet over the game (a Custom Tab on Android), and return the id its answer carries: `web_sign_in` with the `url` it ended on when it reached `opts.scheme`, `cancelled` when shut, `unsupported` where the platform has no such sheet (open the page with `engine::open_url` there).",
        ),
        (
            WEB_SIGN_IN_STATE,
            &[],
            "() -> string",
            "How the page `web_sign_in` opened last stands, without waiting: `PAGE_NONE` before any, `PAGE_OPEN` while it has not answered, else the `kind` it answered. A game polling a server meanwhile reads it to hear the player shut the page.",
        ),
        (
            CLOSE_WEB_SIGN_IN,
            &[],
            "() -> nil",
            "Shut the sign-in page `web_sign_in` opened, once the game has what it opened it for: its call answers `cancelled`, and on Android the game comes back to the front over the tab.",
        ),
    ]);
    m.function(
        kind::WEB_SIGN_IN,
        |eng: &Engine, (url, opts): (Value, Option<Value>)| {
            let url = text(Some(&url), "a sign-in page's address")?;
            let scheme = match opt(opts.as_ref(), "scheme") {
                Some(Value::Str(scheme)) => Some(scheme.clone()),
                Some(other) => return Err(anyhow!("`scheme` should be a string, got {other:?}")),
                None => None,
            };
            Ok(start_call(eng, Call::WebSignIn { url, scheme }))
        },
    );
    m.constant("PAGE_NONE", Value::Str(PAGE_NONE.into()));
    m.constant("PAGE_OPEN", Value::Str(PAGE_OPEN.into()));
    m.function(WEB_SIGN_IN_STATE, |eng: &Engine, ()| {
        Ok(Value::Str(
            eng.resource::<PlatformState>()
                .borrow()
                .web_sign_in_state()
                .into(),
        ))
    });
    m.function(CLOSE_WEB_SIGN_IN, |eng: &Engine, ()| {
        eng.resource::<PlatformState>()
            .borrow_mut()
            .close_web_sign_in(eng);
        Ok(())
    });
}

//! What the OS lends a game, whichever store is loaded: local notifications
//! (`platform.notify`), a push token (`platform.register_for_push`), the
//! motor, the share sheet and the review prompt (`device.*`).
//!
//! A second seam beside the store's, because these belong to the OS: a Mac
//! game Steam started still notifies through Apple's centre. Apple's plugin
//! fills it there; elsewhere this crate's own (`native.rs`) answers,
//! through the Java shims on Android and `navigator.vibrate` on a page. An
//! answer crosses the platform's channel, so it is recorded and replayed as a
//! store's is, and a tapped notification is the `platform_event` event.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_core::handler::opt;
use balaur_script::{Module, Value};

use crate::vocabulary::{keys as k, words};
use crate::{PlatformEvent, PlatformState, kind};

/// The script module of what a phone lends a game.
pub const DEVICE: &str = "device";

const CANCEL_NOTIFICATION: &str = "cancel_notification";
const HAPTIC: &str = "haptic";
const NOTIFY: &str = "notify";
const REGISTER_FOR_PUSH: &str = "register_for_push";
const REQUEST_NOTIFICATIONS: &str = "request_notifications";
const REQUEST_REVIEW: &str = "request_review";
const SHARE: &str = "share";
const VIBRATE: &str = "vibrate";

/// One call on its way to the OS.
#[derive(Clone, Debug)]
pub enum DeviceCall {
    /// Permission to show notifications at all.
    RequestNotifications,
    /// One local notification, `after` seconds from now, carrying `data` back
    /// when it is tapped.
    Notify {
        id: String,
        title: String,
        body: String,
        after: f64,
        data: serde_json::Value,
    },
    /// The OS's share sheet over the game, with text, a file or both.
    Share {
        text: String,
        file: Option<PathBuf>,
        title: String,
    },
    /// The store's own prompt to rate the game, which the store may not show.
    RequestReview,
    /// The device's push token: APNs' on Apple, FCM's on Android.
    RegisterForPush,
}

impl DeviceCall {
    /// The name an `unsupported` event carries, and what the recording logs.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::RequestNotifications => REQUEST_NOTIFICATIONS,
            Self::Notify { .. } => NOTIFY,
            Self::Share { .. } => SHARE,
            Self::RequestReview => REQUEST_REVIEW,
            Self::RegisterForPush => REGISTER_FOR_PUSH,
        }
    }

    /// The script module the call is in, which its stand-in is keyed by.
    #[must_use]
    pub const fn module(&self) -> &'static str {
        match self {
            Self::RequestNotifications | Self::Notify { .. } | Self::RegisterForPush => {
                crate::PLATFORM
            }
            Self::Share { .. } | Self::RequestReview => DEVICE,
        }
    }

    /// Whether a rollback could not take it back: a notification on the
    /// schedule stays there.
    #[must_use]
    pub const fn writes(&self) -> bool {
        matches!(self, Self::Notify { .. })
    }
}

/// A tap of the motor, as the OS names its feedback: a `HAPTIC_*` constant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Haptic {
    Light,
    Medium,
    Heavy,
    /// A picker moving one step.
    Selection,
    Success,
    Warning,
    Error,
}

impl Haptic {
    pub const ALL: [Self; 7] = [
        Self::Light,
        Self::Medium,
        Self::Heavy,
        Self::Selection,
        Self::Success,
        Self::Warning,
        Self::Error,
    ];

    /// The word its constant holds, and the Java shim reads.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Light => words::haptics::LIGHT,
            Self::Medium => words::haptics::MEDIUM,
            Self::Heavy => words::haptics::HEAVY,
            Self::Selection => words::haptics::SELECTION,
            Self::Success => words::haptics::SUCCESS,
            Self::Warning => words::haptics::WARNING,
            Self::Error => kind::ERROR,
        }
    }

    fn parse(word: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|haptic| haptic.name() == word)
            .ok_or_else(|| {
                anyhow!("no haptic called {word:?}: name one with a device::HAPTIC_* constant")
            })
    }
}

/// What an OS's plugin implements: the calls that answer later, and the
/// motor, which answers nothing.
pub trait DeviceBackend {
    /// Start one call. What it produces goes down `report`, whenever the OS
    /// answers.
    fn start(&mut self, request: u64, call: &DeviceCall, report: &Sender<PlatformEvent>);

    /// Drop a scheduled notification, and take a delivered one out of the shade.
    fn cancel_notification(&mut self, _id: &str) {}

    /// Buzz the motor for `seconds`.
    fn vibrate(&mut self, _seconds: f64) {}

    /// One tap of feedback.
    fn haptic(&mut self, _effect: Haptic) {}

    /// Once per tick, to move what arrived unasked (a notification tapped)
    /// onto the channel, where it is recorded.
    fn pump(&mut self, _report: &Sender<PlatformEvent>) {}

    /// Instead of [`DeviceBackend::pump`] on a replayed or re-simulated tick:
    /// what arrived meanwhile is not this run's.
    fn discard(&mut self) {}
}

/// Give the loaded engine its OS's services, in place of this crate's own.
///
/// # Errors
/// If `PlatformPlugin` was never loaded.
pub fn set_device(eng: &Engine, backend: Box<dyn DeviceBackend>) -> Result<()> {
    let state = eng
        .try_resource::<PlatformState>()
        .ok_or_else(|| anyhow!("the platform plugin has to be loaded before an OS's"))?;
    state.borrow_mut().device = backend;
    Ok(())
}

fn start_device(eng: &Engine, call: DeviceCall) -> Value {
    let id = eng.next_token();
    // A notification the script did not name is named after the call, so two
    // runs of the same session schedule the same identifier.
    let call = match call {
        DeviceCall::Notify {
            id: name,
            title,
            body,
            after,
            data,
        } if name.is_empty() => DeviceCall::Notify {
            id: format!("balaur-{id}"),
            title,
            body,
            after,
            data,
        },
        call => call,
    };
    let state = eng.resource::<PlatformState>();
    state.borrow_mut().start_device(eng, id, call);
    balaur_core::handler::id_value(id)
}

fn text_opt(opts: Option<&Value>, key: &str) -> Result<String> {
    match opt(opts, key) {
        Some(Value::Str(text)) => Ok(text.clone()),
        Some(other) => Err(anyhow!("`{key}` should be a string, got {other:?}")),
        None => Ok(String::new()),
    }
}

/// `platform.request_notifications`, `notify` and `cancel_notification`, and
/// `register_for_push`.
pub(crate) fn install_notifications(m: &mut Module<Engine>) {
    install_push(m);
    m.records(&[(
        "NotifyOptions",
        "{ title: string?, after: float?, id: string?, data: any? }",
        "A local notification's `title`, how many seconds from now it shows (`after`), the `id` to cancel or recognise it by, and the `data` its `notification_opened` arrival carries back when the player taps it.",
    )]);
    m.describe(&[
        (
            REQUEST_NOTIFICATIONS,
            &[],
            "() -> int",
            "Ask the player to allow notifications, and return the id whose `notifications` answer says what they said as `allowed`. On Android 13 and later this is the system's permission prompt.",
        ),
        (
            NOTIFY,
            &[],
            "(body: string, opts: NotifyOptions?) -> int",
            "Schedule a local notification, and return the id its `scheduled` answer carries. A tap brings the game forward and is the `platform_event` event, `notification_opened` with the `id` and `data`; one shown while the game is in front is `notification_received`.",
        ),
        (
            CANCEL_NOTIFICATION,
            &[],
            "(id: string)",
            "Drop a scheduled notification, and take a delivered one out of the shade.",
        ),
    ]);
    m.function(REQUEST_NOTIFICATIONS, |eng: &Engine, ()| {
        Ok(start_device(eng, DeviceCall::RequestNotifications))
    });
    m.function(
        NOTIFY,
        |eng: &Engine, (body, opts): (Value, Option<Value>)| {
            let Value::Str(body) = body else {
                return Err(anyhow!(
                    "the notification text should be a string, got {body:?}"
                ));
            };
            let opts = opts.as_ref();
            let after = match opt(opts, k::AFTER) {
                Some(Value::Num(seconds)) => *seconds,
                #[allow(clippy::cast_precision_loss, reason = "seconds from now")]
                Some(Value::Int(seconds)) => *seconds as f64,
                Some(other) => return Err(anyhow!("`after` should be a number, got {other:?}")),
                None => 0.0,
            };
            let data = match opt(opts, k::DATA) {
                Some(value) => balaur_core::file_api::to_json(value)?,
                None => serde_json::Value::Null,
            };
            Ok(start_device(
                eng,
                DeviceCall::Notify {
                    id: text_opt(opts, k::ID)?,
                    title: text_opt(opts, k::TITLE)?,
                    body,
                    after,
                    data,
                },
            ))
        },
    );
    m.function(CANCEL_NOTIFICATION, |eng: &Engine, (id,): (Value,)| {
        let Value::Str(id) = id else {
            return Err(anyhow!("a notification id should be a string, got {id:?}"));
        };
        eng.resource::<PlatformState>()
            .borrow_mut()
            .cancel_notification(eng, &id);
        Ok(())
    });
}

/// `platform.register_for_push`, and the `PUSH_*` words its answer carries.
fn install_push(m: &mut Module<Engine>) {
    for (name, word) in [
        ("PUSH_PROVIDER_APNS", words::push::APNS),
        ("PUSH_PROVIDER_FCM", words::push::FCM),
        ("PUSH_PLATFORM_IOS", words::push::IOS),
        ("PUSH_PLATFORM_ANDROID", words::push::ANDROID),
    ] {
        m.constant(name, Value::Str(word.into()));
    }
    m.describe(&[(
        REGISTER_FOR_PUSH,
        &[],
        "() -> int",
        "Ask the OS for this device's push token, and return the id whose `push_registered` answer carries it as `token`, with `provider` (`PUSH_PROVIDER_APNS` or `PUSH_PROVIDER_FCM`) and `platform` (`PUSH_PLATFORM_IOS`, a Mac's too, or `PUSH_PLATFORM_ANDROID`): the three fields Gamend's `push_tokens::register_push_token` takes, so the answer is handed to it as it is. `push_failed` says why with `error`, naming the capability to add when the export left it out (`[apple] capabilities = [\"push-notifications\"]`, `[android] capabilities = [\"firebase-messaging\"]`); `unsupported` elsewhere. A token the OS replaces later is `push_token_changed` on `platform_event`, with the same fields. Showing a push needs `request_notifications` first, as a local notification does.",
    )]);
    m.function(REGISTER_FOR_PUSH, |eng: &Engine, ()| {
        Ok(start_device(eng, DeviceCall::RegisterForPush))
    });
}

/// `device.*`: the motor, the share sheet and the review prompt.
pub(crate) fn install_device(m: &mut Module<Engine>) {
    m.module_doc(
        "What a phone lends a game beyond the store: its motor, the share sheet and the store's review prompt. A call that answers does so through `task::wait` with a map whose `kind` is `shared`, `done`, `cancelled`, `error` or `unsupported`, each an `EVENT_*` constant. `platform::is_available` says which of them work here. A test answers `share` and `request_review` from the file `BALAUR_STAND_INS` names.",
    );
    balaur_core::handler::install_event_kinds(m, kind::DEVICE);
    for haptic in Haptic::ALL {
        m.constant(
            &format!("HAPTIC_{}", haptic.name().to_ascii_uppercase()),
            Value::Str(haptic.name().into()),
        );
    }
    m.records(&[(
        "ShareOptions",
        "{ text: string?, file: string?, title: string? }",
        "What the share sheet hands on: `text`, a `file` the game may read (project-relative, or in the user data directory), or both, and the sheet's `title` where the OS shows one.",
    )]);
    m.describe(&[
        (
            VIBRATE,
            &[],
            "(seconds: float)",
            "Buzz the device's motor for that many seconds: Android's vibrator, a page's `navigator.vibrate`, the iPhone's one system buzz whatever the length. Nothing on a desktop, and never recorded, like rumble.",
        ),
        (
            HAPTIC,
            &[],
            "(effect: string)",
            "One tap of feedback, named by a `HAPTIC_*` constant: iOS's feedback generators, Android's predefined effects, a short buzz on a page. Nothing on a desktop, and never recorded.",
        ),
        (
            SHARE,
            &[],
            "(opts: ShareOptions) -> int",
            "Show the OS's share sheet over the game, and return the id its answer carries: `shared` with the `target` the player picked where the OS names it, `cancelled`, `error`, or `unsupported` where there is no sheet.",
        ),
        (
            REQUEST_REVIEW,
            &[],
            "() -> int",
            "Ask the store to show its prompt to rate the game, and return the id its `done` answer carries. The store decides whether it shows, and says nothing about what the player did. `unsupported` off iOS and macOS.",
        ),
    ]);
    m.function(VIBRATE, |eng: &Engine, seconds: f64| {
        eng.resource::<PlatformState>()
            .borrow_mut()
            .vibrate(eng, seconds);
        Ok(())
    });
    m.function(HAPTIC, |eng: &Engine, effect: Value| {
        let Value::Str(word) = effect else {
            return Err(anyhow!(
                "a haptic should be a device::HAPTIC_* constant, got {effect:?}"
            ));
        };
        let effect = Haptic::parse(&word)?;
        eng.resource::<PlatformState>()
            .borrow_mut()
            .haptic(eng, effect);
        Ok(())
    });
    m.function(SHARE, |eng: &Engine, opts: Value| {
        let opts = Some(&opts);
        let text = text_opt(opts, k::TEXT)?;
        let file = match opt(opts, k::FILE) {
            Some(Value::Str(path)) => {
                let real = balaur_core::file_api::resolve(eng, path)?;
                anyhow::ensure!(real.is_file(), "no file to share at {}", real.display());
                Some(real)
            }
            Some(other) => return Err(anyhow!("`file` should be a string, got {other:?}")),
            None => None,
        };
        anyhow::ensure!(
            !text.is_empty() || file.is_some(),
            "a share needs `text`, a `file` or both"
        );
        let title = text_opt(opts, k::TITLE)?;
        Ok(start_device(eng, DeviceCall::Share { text, file, title }))
    });
    m.function(REQUEST_REVIEW, |eng: &Engine, ()| {
        Ok(start_device(eng, DeviceCall::RequestReview))
    });
}

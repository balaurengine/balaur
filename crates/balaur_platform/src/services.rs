//! `platform.is_available`: whether a service works on this device in this
//! run, one check for every store and every device.
//!
//! The plugin that reaches a service answers for it, through [`provide`]:
//! Apple's for Game Center and the share sheet, Google's for its sign-in, the
//! facade for Steam, this crate for what Android and a page lend a game. A
//! service nobody answers for is not available. The answers are pinned in a
//! recording's header, so a replay takes the branch the recording took.

use std::collections::HashMap;
use std::rc::Rc;

use anyhow::{Result, anyhow};
use balaur_core::Engine;
use balaur_script::{Module, Value};

use crate::vocabulary::words::services;
use crate::{PlatformState, kind};

const IS_AVAILABLE: &str = "is_available";
/// The setup source the answers are pinned under.
pub(crate) const SERVICES: &str = "platform_services";

/// One service a game can ask about: a `SERVICE_*` constant in scripts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Service {
    /// Valve's library loaded and the Steam client runs the game.
    Steam,
    /// Game Center: `platform.sign_in` and the rest on Apple, `apple.identity`.
    GameCenter,
    /// Play Games: `platform.sign_in`, achievements, leaderboards, saved
    /// games and `identity` on Android.
    PlayGames,
    /// iCloud's key-value store behind `platform.cloud_read` and `cloud_write`.
    Icloud,
    /// Sign in with Apple, `apple.sign_in`.
    AppleSignIn,
    /// Sign in with Google, `google.sign_in`.
    GoogleSignIn,
    /// A sign-in page in the OS's own sheet, `platform.web_sign_in`.
    WebSignIn,
    /// Local notifications, `platform.notify`.
    Notifications,
    /// A push token, `platform.register_for_push`.
    Push,
    /// The store's purchases, `platform.purchase` and the rest: StoreKit, or
    /// Play Billing.
    Purchases,
    /// The device's own motor, `device.vibrate` and `device.haptic`.
    Haptics,
    /// The OS's share sheet, `device.share`.
    Share,
    /// The store's review prompt, `device.request_review`.
    Review,
}

impl Service {
    pub const ALL: [Self; 13] = [
        Self::Steam,
        Self::GameCenter,
        Self::PlayGames,
        Self::Icloud,
        Self::AppleSignIn,
        Self::GoogleSignIn,
        Self::WebSignIn,
        Self::Notifications,
        Self::Push,
        Self::Purchases,
        Self::Haptics,
        Self::Share,
        Self::Review,
    ];

    /// The word a script's `SERVICE_*` constant holds, and a recording keys.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Steam => services::STEAM,
            Self::GameCenter => services::GAME_CENTER,
            Self::PlayGames => services::PLAY_GAMES,
            Self::Icloud => services::ICLOUD,
            Self::AppleSignIn => services::APPLE_SIGN_IN,
            Self::GoogleSignIn => services::GOOGLE_SIGN_IN,
            Self::WebSignIn => kind::WEB_SIGN_IN,
            Self::Notifications => kind::NOTIFICATIONS,
            Self::Push => services::PUSH,
            Self::Purchases => services::PURCHASES,
            Self::Haptics => services::HAPTICS,
            Self::Share => services::SHARE,
            Self::Review => services::REVIEW,
        }
    }

    /// The service a word names.
    ///
    /// # Errors
    /// For a word no `SERVICE_*` constant holds: a typo ends the call.
    pub fn parse(word: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|service| service.name() == word)
            .ok_or_else(|| {
                anyhow!("no service called {word:?}: name one with a platform::SERVICE_* constant")
            })
    }
}

/// What answers for one service, asked each time a script asks.
pub type Provider = Rc<dyn Fn(&Engine) -> bool>;

/// Who answers for each service, and the answers a recording pinned.
#[derive(Default)]
pub(crate) struct Services {
    providers: HashMap<Service, Provider>,
    pinned: Option<HashMap<String, bool>>,
}

/// Answer for `service` from now on, in place of whoever answered before: a
/// plugin's `declare`, which runs after this crate's.
///
/// # Errors
/// If the platform plugin was never loaded.
pub fn provide(
    eng: &Engine,
    service: Service,
    provider: impl Fn(&Engine) -> bool + 'static,
) -> Result<()> {
    let state = eng
        .try_resource::<PlatformState>()
        .ok_or_else(|| anyhow!("the platform plugin has to be loaded before a provider"))?;
    state
        .borrow_mut()
        .services
        .providers
        .insert(service, Rc::new(provider));
    Ok(())
}

/// Whether `service` works here: the recording's answer in a replay, a
/// test's stand-in, else its provider's, else false.
#[must_use]
pub fn is_available(eng: &Engine, service: Service) -> bool {
    if let Some(Some(serde_json::Value::Object(named))) =
        crate::stand_in(eng, crate::PLATFORM, IS_AVAILABLE)
        && let Some(answer) = named.get(service.name())
    {
        return answer.as_bool().unwrap_or(false);
    }
    let provider = {
        let state = eng.resource::<PlatformState>();
        let state = state.borrow();
        if let Some(pinned) = &state.services.pinned {
            return pinned.get(service.name()).copied().unwrap_or(false);
        }
        state.services.providers.get(&service).cloned()
    };
    provider.is_some_and(|provider| provider(eng))
}

/// Every service's answer, pinned so the rest of the recording answers the
/// same: what the header carries.
pub(crate) fn capture(eng: &Engine) -> serde_json::Value {
    let answers: HashMap<String, bool> = Service::ALL
        .into_iter()
        .map(|service| (service.name().to_string(), is_available(eng, service)))
        .collect();
    let value = serde_json::to_value(&answers).unwrap_or_default();
    eng.resource::<PlatformState>().borrow_mut().services.pinned = Some(answers);
    value
}

/// A recording's answers, which a replay gives in place of this machine's.
pub(crate) fn restore(eng: &Engine, value: &serde_json::Value) {
    if let Ok(answers) = serde_json::from_value::<HashMap<String, bool>>(value.clone()) {
        eng.resource::<PlatformState>().borrow_mut().services.pinned = Some(answers);
    }
}

/// `platform.is_available` and its `SERVICE_*` constants.
pub(crate) fn install(m: &mut Module<Engine>) {
    m.describe(&[(
        IS_AVAILABLE,
        &[],
        "(service: string) -> bool",
        "Whether a service works on this device in this run, named by a `SERVICE_*` constant: the store or OS answers, a recording's answer in a replay, false where nothing reaches it.",
    )]);
    for service in Service::ALL {
        m.constant(
            &format!("SERVICE_{}", service.name().to_ascii_uppercase()),
            Value::Str(service.name().into()),
        );
    }
    m.function(IS_AVAILABLE, |eng: &Engine, service: Value| {
        let Value::Str(word) = service else {
            return Err(anyhow!(
                "a service should be a platform::SERVICE_* constant, got {service:?}"
            ));
        };
        Ok(Value::Bool(is_available(eng, Service::parse(&word)?)))
    });
}

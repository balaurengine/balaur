//! Stand-in answers: a run whose platform calls are answered from a file
//! rather than by a store, for a game's tests.
//!
//! A test cannot sign in with Apple or show a sign-in page: there is no
//! account and, offscreen, no window. With `BALAUR_STAND_INS` naming a JSON
//! file, a call named there gets that answer instead of reaching its
//! backend:
//!
//! ```json
//! {
//!   "apple.sign_in": { "kind": "signed_in", "user": "leo", "code": "test-apple:leo" },
//!   "platform.web_sign_in": null
//! }
//! ```
//!
//! Each key is `<module>.<call>`. An answer is the map the call's waiter
//! receives, `kind` and all; its `request` is filled in. `null` is an answer
//! that never comes, as a sign-in page left open does. A call the file does
//! not name goes to its backend as usual. `"platform.backend": "steam"` is
//! the store `platform.backend()` names, so a test plays a store's build;
//! `"engine.prefers_reduced_motion": "reduce"` (or `"no-preference"`, CSS's
//! two words) is the system's reduce-motion setting, whatever the machine
//! says; `"secrets.backend": "file"` keeps `secrets.*` in a file under the
//! user directory rather than the OS's keychain.
//!
//! An answer travels the backend's own channel, so it is recorded and a
//! replay plays it back like any other.

use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use balaur_core::Engine;

/// The variable that names the file.
pub const ENV: &str = "BALAUR_STAND_INS";

/// The answers this run gives in place of its backends, by `<module>.<call>`.
#[derive(Default)]
pub struct StandIns {
    answers: HashMap<String, serde_json::Value>,
}

impl StandIns {
    /// Answers from a JSON object of `<module>.<call>` to answer.
    ///
    /// # Errors
    /// If `json` is not an object, or an answer is neither a map nor `null`.
    pub fn parse(json: &str) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_str(json)?;
        let serde_json::Value::Object(entries) = value else {
            return Err(anyhow!(
                "stand-ins should be an object of \"module.call\": answer"
            ));
        };
        for (call, answer) in &entries {
            if !(answer.is_object() || answer.is_null() || answer.is_string()) {
                return Err(anyhow!(
                    "the stand-in for {call} should be a map, a name or null, got {answer}"
                ));
            }
        }
        Ok(Self {
            answers: entries.into_iter().collect(),
        })
    }

    /// The file `BALAUR_STAND_INS` names, or none when it is unset.
    ///
    /// # Errors
    /// If the file cannot be read or does not parse: a test that names one
    /// must not quietly reach a real store.
    pub fn from_env() -> Result<Self> {
        let Some(path) = std::env::var_os(ENV) else {
            return Ok(Self::default());
        };
        let path = std::path::PathBuf::from(path);
        let json = std::fs::read_to_string(&path)
            .with_context(|| format!("{ENV}: cannot read {}", path.display()))?;
        Self::parse(&json).with_context(|| format!("{ENV}: {}", path.display()))
    }

    /// The answer for `module.call`: `None` when the file does not name it,
    /// `Some(None)` when it names it `null`.
    #[must_use]
    pub fn answer(&self, module: &str, call: &str) -> Option<Option<serde_json::Value>> {
        self.answers
            .get(&format!("{module}.{call}"))
            // copy: every call to it gets an answer of its own.
            .map(|answer| (!answer.is_null()).then(|| answer.clone()))
    }
}

/// CSS's words for the reduce-motion setting, as a stand-in names it.
const REDUCE: &str = "reduce";
const NO_PREFERENCE: &str = "no-preference";

/// The module the reduce-motion call is in, which its stand-in is keyed by.
const ENGINE: &str = "engine";

/// What `stand_ins` holds the device to, if they name any of it: the
/// reduce-motion setting.
///
/// # Errors
/// For a reduce-motion word other than `reduce` or `no-preference`.
pub(crate) fn held_device(stand_ins: &StandIns) -> Result<Option<balaur_core::facts::DevicePatch>> {
    let reduced = match stand_ins.answer(ENGINE, balaur_core::facts::PREFERS_REDUCED_MOTION) {
        None | Some(None) => return Ok(None),
        Some(Some(serde_json::Value::String(word))) if word == REDUCE => true,
        Some(Some(serde_json::Value::String(word))) if word == NO_PREFERENCE => false,
        Some(Some(other)) => {
            return Err(anyhow!(
                "engine.prefers_reduced_motion names \"{REDUCE}\" or \"{NO_PREFERENCE}\", got {other}"
            ));
        }
    };
    Ok(Some(std::rc::Rc::new(
        move |facts: &mut balaur_core::facts::DeviceFacts| {
            facts.reduced_motion = reduced;
        },
    )))
}

/// This run's stand-in for `module.call`, as [`StandIns::answer`] gives it.
/// `None` on an engine without the platform plugin.
#[must_use]
pub fn stand_in(eng: &Engine, module: &str, call: &str) -> Option<Option<serde_json::Value>> {
    eng.try_resource::<StandIns>()?
        .borrow()
        .answer(module, call)
}

/// Replace this run's stand-ins, for a test that sets them from Rust.
///
/// # Errors
/// If the platform plugin was never loaded.
pub fn set_stand_ins(eng: &Engine, stand_ins: StandIns) -> Result<()> {
    let held = eng
        .try_resource::<StandIns>()
        .ok_or_else(|| anyhow!("the platform plugin has to be loaded first"))?;
    balaur_core::facts::hold_device(eng, held_device(&stand_ins)?);
    *held.borrow_mut() = stand_ins;
    Ok(())
}

// `task::seconds`, `task::frames` and `task::wait`: what a script awaits on
// the engine. Each answers with a token the engine wakes at a fixed step, so
// a wait replays exactly.

use balaur_bs::runtime::Value as L;
use balaur_bs::{HostSignature, Ty};
use balaur_core::Engine;

const MODULE: &str = "task";

pub(crate) fn signatures() -> Vec<HostSignature> {
    let wait = |name: &str, param: Ty, ret: Ty| HostSignature {
        key: format!("{MODULE}::{name}"),
        params: vec![param],
        ret,
        is_async: true,
    };
    vec![
        wait("seconds", Ty::Float, Ty::Void),
        wait("frames", Ty::Int, Ty::Void),
        // A token a binding handed out, resolving to what the engine woke it with.
        wait("wait", Ty::Int, Ty::Any),
    ]
}

/// What each wait does, keyed `task::name`.
pub(crate) fn docs() -> impl Iterator<Item = (String, &'static str)> {
    [
        ("seconds", "Park an async method for a span of simulation time, in fixed steps; the wall clock never enters it."),
        ("frames", "Park an async method for a number of fixed steps; counted on the tick, so a replay waits exactly as long."),
        ("wait", "Park an async method until the engine wakes the token a binding gave it, and answer what it woke with."),
    ]
    .into_iter()
    .map(|(name, doc)| (format!("{MODULE}::{name}"), doc))
}

/// What the module is for, as the reference opens it.
pub(crate) const MODULE_DOC: (&str, &str) = (
    MODULE,
    "Waiting inside an async method: `init` and event handlers may await; `update` is synchronous.",
);

/// Whether `key` is one of these.
pub(crate) fn is_wait(key: &str) -> bool {
    key.strip_prefix(MODULE)
        .is_some_and(|rest| rest.starts_with("::"))
}

/// The token `key(args)` waits on, when `key` is one of these.
pub(crate) fn token(eng: &Engine, key: &str, args: &[L]) -> Option<u64> {
    let name = key.strip_prefix(MODULE)?.strip_prefix("::")?;
    Some(match (name, args.first()) {
        ("seconds", Some(L::Float(s))) => balaur_core::timers::after_seconds(eng, *s as f32),
        ("frames", Some(L::Int(n))) => {
            balaur_core::timers::after_frames(eng, u32::try_from(*n).unwrap_or(0))
        }
        ("wait", Some(L::Int(t))) => t.cast_unsigned(),
        _ => return None,
    })
}

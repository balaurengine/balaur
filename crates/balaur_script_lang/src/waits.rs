// `task::seconds`, `task::frames` and `task::wait`: what a script awaits on
// the engine. Each answers with a token the engine wakes at a fixed step, so
// a wait replays exactly.

use balaur_core::Engine;
use balaur_lang::runtime::Value as L;
use balaur_lang::{HostSignature, Ty};

const MODULE: &str = "task";

pub(crate) fn signatures() -> Vec<HostSignature> {
    let wait = |name: &str, param: Ty| HostSignature {
        key: format!("{MODULE}::{name}"),
        params: vec![param],
        ret: Ty::Void,
        is_async: true,
    };
    vec![
        wait("seconds", Ty::Float),
        wait("frames", Ty::Int),
        // A token a binding handed out; what it carries is dropped for now.
        wait("wait", Ty::Int),
    ]
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

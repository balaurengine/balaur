//! `rollback.*`: what a script needs to take part in a rollback session.
//!
//! Two questions, because there are only two a script can usefully ask. What
//! is this player doing on the tick being simulated, and is this tick one the
//! engine is running for the second time.

use balaur_script::{Bindings, BindingsExt, Value};

use crate::engine::Engine;

pub fn install_rollback_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Rollback netcode from a script's side. `input` reads this tick's input, real or predicted; `is_resimulating` is true when a tick runs again after a late input.",
    );
    m.describe(&[
        (
            "input",
            &[],
            "(player: int)",
            "What that player is doing on the tick being simulated, real or predicted; nil outside a session or for a player it does not know.",
        ),
        (
            "is_resimulating",
            &[],
            "()",
            "Whether this tick is a re-run of one already simulated, so a script can skip anything it must not do twice.",
        ),
    ]);
    m.function("input", |eng: &Engine, player: i64| {
        let Ok(player) = u32::try_from(player) else {
            return Ok(Value::Nil);
        };
        Ok(crate::rollback::input(eng, player).unwrap_or(Value::Nil))
    });
    m.function("is_resimulating", |eng: &Engine, (): ()| {
        Ok(Value::Bool(crate::rollback::is_resimulating(eng)))
    });
}

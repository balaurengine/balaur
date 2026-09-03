//! `rollback.*`: what a script needs to take part in a rollback session.
//!
//! Two questions, because there are only two a script can usefully ask. What
//! is this player doing on the tick being simulated, and is this tick one the
//! engine is running for the second time.

use balaur_script::{Bindings, BindingsExt, Value};

use crate::engine::Engine;

pub fn install_rollback_api(m: &mut dyn Bindings<Engine>) {
    m.module_doc(
        "Rollback netcode from a script's side. The session decides each \
         tick's inputs before the tick runs — the real one where it has \
         arrived, a repeat of the player's last one where it has not — and \
         `input` reads whichever it settled on. A tick may run more than \
         once: when a late input contradicts a prediction, the engine \
         restores the tick before it and simulates forward again, so \
         anything a script does with an effect outside the simulation has to \
         ask `is_resimulating` first.",
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

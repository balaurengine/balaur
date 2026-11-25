//! A pack in pieces: the head a game starts from, and the tail of assets an
//! export left out of it, fetched while the game runs. A switch to a scene
//! that names an asset only the tail holds waits for the tail.

use std::cell::RefCell;
use std::collections::BTreeSet;

use crate::Pack;
use crate::engine::Engine;
use crate::vocabulary::keys;

/// Where a head names its tail: the tail's file, beside the head, and the
/// scenes that wait for it.
pub const TAIL: &str = ".balaur/tail.toml";

/// A head's tail, until it arrives.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tail {
    /// The tail's file name, beside the head's.
    pub file: String,
    /// Each scene a switch to waits for the tail.
    pub waits: BTreeSet<String>,
}

impl Tail {
    /// The tail `head` names, when it is a head.
    #[must_use]
    pub fn of(head: &Pack) -> Option<Tail> {
        let text = std::str::from_utf8(head.assets.get(TAIL)?).ok()?;
        let table: toml::Table = toml::from_str(text).ok()?;
        let file = table.get(keys::FILE)?.as_str()?.to_string();
        let waits = table
            .get(keys::WAITS)
            .and_then(toml::Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|scene| scene.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        Some(Tail { file, waits })
    }

    /// The bytes [`TAIL`] holds, which [`Tail::of`] reads.
    #[must_use]
    pub fn written(&self) -> Vec<u8> {
        let mut table = toml::Table::new();
        table.insert(keys::FILE.into(), toml::Value::String(self.file.clone()));
        table.insert(
            keys::WAITS.into(),
            toml::Value::Array(
                self.waits
                    .iter()
                    .map(|scene| toml::Value::String(scene.clone()))
                    .collect(),
            ),
        );
        table.to_string().into_bytes()
    }
}

thread_local! {
    /// The tail a fetch delivered, for the next tick to add.
    static DELIVERED: RefCell<Option<Pack>> = const { RefCell::new(None) };
}

/// Hand the running game its tail, from whatever fetched it. The next tick
/// adds its files, and a switch that waited goes ahead.
pub fn deliver(tail: Pack) {
    DELIVERED.with(|slot| *slot.borrow_mut() = Some(tail));
    crate::wake::wake();
}

/// Whether a switch to `scene` waits for a tail that has not arrived.
#[must_use]
pub fn waits(eng: &Engine, scene: &str) -> bool {
    eng.try_resource::<Tail>()
        .is_some_and(|tail| tail.borrow().waits.contains(scene))
}

/// Note the tail a run from `pack` waits for, if it is a head.
pub(crate) fn note(eng: &Engine, pack: Option<&Pack>) {
    if let Some(tail) = pack.and_then(Tail::of) {
        eng.insert_resource(tail);
    }
}

/// At the start of a tick: a delivered tail's files join the pack's, and the
/// scenes waiting on them go on.
pub(crate) fn arrive_system(eng: &Engine, _dt: f32) {
    let Some(tail) = DELIVERED.with(|slot| slot.borrow_mut().take()) else {
        return;
    };
    let count = tail.assets.len();
    if let Some(files) = eng.try_resource::<crate::project::ProjectFiles>() {
        files.borrow_mut().arrive(tail.assets);
    }
    if let Some(waiting) = eng.try_resource::<Tail>() {
        waiting.borrow_mut().waits.clear();
    }
    tracing::info!("the pack's tail arrived: {count} files");
}

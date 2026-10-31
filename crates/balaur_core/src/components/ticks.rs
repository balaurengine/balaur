//! When each registered component was put on a node and last changed, and
//! what came off, by change tick (PLAN-ecs §2): what a system asks "added,
//! changed or removed since I last ran" of, as a comparison.
//!
//! Stamped from the calls that feed the change log, which every write of a
//! registered component makes (`changes::component`, `property`, `attached`,
//! `removed`) and a freed node's components, so the log and the ticks are
//! one source.

use crate::collections::DetHashMap;
use crate::engine::Engine;
use crate::hecs::Entity;

/// A component's added and changed ticks on one node. A write that put the
/// component on stamps both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub added: u64,
    pub changed: u64,
}

/// Every stamp, and the components that came off.
#[derive(Default)]
pub struct Ticks {
    stamps: DetHashMap<(Entity, usize), Stamp>,
    /// What came off, with the tick it came off at; kept for the frame it
    /// came off in and the next, so a reader that runs every frame hears it.
    removed: Vec<(Entity, usize, u64)>,
    /// The tick a frame was restored at: stamps are not carried in a frame, so
    /// a reader that last ran before it hears every component as changed.
    restored_at: u64,
    /// The ticks the last two frames started at, which bound what `removed`
    /// keeps.
    frames: [u64; 2],
}

fn with(eng: &Engine, write: impl FnOnce(&mut Ticks)) {
    if let Some(ticks) = eng.try_resource::<Ticks>() {
        write(&mut ticks.borrow_mut());
    }
}

/// Component `index` went onto `node` now.
pub(crate) fn added(eng: &Engine, node: Entity, index: usize) {
    let now = eng.change_tick();
    with(eng, |ticks| {
        ticks.stamps.insert(
            (node, index),
            Stamp {
                added: now,
                changed: now,
            },
        );
    });
}

/// Component `index` of `node` changed now.
pub(crate) fn changed(eng: &Engine, node: Entity, index: usize) {
    let now = eng.change_tick();
    with(eng, |ticks| {
        ticks
            .stamps
            .entry((node, index))
            .and_modify(|stamp| stamp.changed = now)
            .or_insert(Stamp {
                added: now,
                changed: now,
            });
    });
}

/// Component `index` came off `node` now.
pub(crate) fn removed(eng: &Engine, node: Entity, index: usize) {
    let now = eng.change_tick();
    with(eng, |ticks| {
        ticks.stamps.swap_remove(&(node, index));
        ticks.removed.push((node, index, now));
    });
}

/// Every component in `present` came off `node`, which is being freed.
pub(crate) fn freed(eng: &Engine, node: Entity, present: u128) {
    let now = eng.change_tick();
    with(eng, |ticks| {
        for index in 0..u128::BITS as usize {
            if present & (1 << index) != 0 {
                ticks.stamps.swap_remove(&(node, index));
                ticks.removed.push((node, index, now));
            }
        }
    });
}

/// A frame starts at a tick of its own: what came off before the frame before
/// it is forgotten.
pub(crate) fn frame_started(eng: &Engine) {
    let now = eng.advance_change_tick();
    with(eng, |ticks| {
        ticks.frames = [ticks.frames[1], now];
        let keep_from = ticks.frames[0];
        ticks.removed.retain(|(_, _, at)| *at >= keep_from);
    });
}

/// A restored frame starts now: every stamp is gone, and a reader that
/// last ran before now hears every component as changed.
pub(crate) fn restored(eng: &Engine) {
    let now = eng.change_tick();
    with(eng, |ticks| {
        ticks.stamps.clear();
        ticks.removed.clear();
        ticks.restored_at = now;
    });
}

/// Component `index`'s stamp on `node`, if it holds one.
#[must_use]
pub fn stamp(eng: &Engine, node: Entity, index: usize) -> Option<Stamp> {
    let ticks = eng.try_resource::<Ticks>()?;
    ticks.borrow().stamps.get(&(node, index)).copied()
}

/// The nodes component `index` went onto after tick `since`.
#[must_use]
pub fn added_since(eng: &Engine, index: usize, since: u64) -> Vec<Entity> {
    matching(eng, index, since, |stamp| stamp.added)
}

/// The nodes component `index` went onto or changed on after tick `since`.
#[must_use]
pub fn changed_since(eng: &Engine, index: usize, since: u64) -> Vec<Entity> {
    matching(eng, index, since, |stamp| stamp.changed)
}

/// The nodes component `index` came off after tick `since`, oldest first.
/// One that came off twice is named twice.
#[must_use]
pub fn removed_since(eng: &Engine, index: usize, since: u64) -> Vec<Entity> {
    let Some(ticks) = eng.try_resource::<Ticks>() else {
        return Vec::new();
    };
    let ticks = ticks.borrow();
    ticks
        .removed
        .iter()
        .filter(|(_, at_index, at)| *at_index == index && *at > since)
        .map(|(node, _, _)| *node)
        .collect()
}

/// The nodes whose stamp of component `index`, as `of` reads it, is after
/// `since`, in node order; every holder when `since` is before a restore.
fn matching(eng: &Engine, index: usize, since: u64, of: impl Fn(&Stamp) -> u64) -> Vec<Entity> {
    let Some(ticks) = eng.try_resource::<Ticks>() else {
        return Vec::new();
    };
    let ticks = ticks.borrow();
    let restored = since < ticks.restored_at;
    let mut found: Vec<Entity> = if restored {
        drop(ticks);
        let world = eng.world();
        world
            .query::<(Entity, &super::Attached)>()
            .iter()
            .filter(|(_, attached)| attached.has(index))
            .map(|(entity, _)| entity)
            .collect()
    } else {
        ticks
            .stamps
            .iter()
            .filter(|((_, at_index), stamp)| *at_index == index && of(stamp) > since)
            .map(|((node, _), _)| *node)
            .collect()
    };
    found.sort_unstable_by_key(|node| node.to_bits());
    found
}

/// What a system keeps between its runs: the change tick it last read at.
/// Each read answers the tick it covers from, exclusive, and moves on to now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChangeCursor {
    last: u64,
}

impl ChangeCursor {
    /// The tick this read covers from, exclusive, and the cursor moved to now.
    pub fn read(&mut self, eng: &Engine) -> u64 {
        let since = self.last;
        self.last = eng.change_tick();
        since
    }
}

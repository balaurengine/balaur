//! When each registered component was put on a node and last changed, and
//! what came off, by change tick (PLAN-ecs §2): what a system asks "added,
//! changed or removed since I last ran" of, as a comparison.
//!
//! Stamped from the calls that feed the change log, which every write of a
//! registered component makes (`changes::component`, `property`, `attached`,
//! `removed`) and a freed node's components, so the log and the ticks are
//! one source; and from a system's writes through a tracked query
//! (`system_param::Tracked`), noted on the writing thread and stamped in one
//! pass when the system ends.

use crate::engine::Engine;
use crate::hecs::Entity;

/// A component's added and changed ticks on one node. A write that put the
/// component on stamps both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub added: u64,
    pub changed: u64,
}

/// Every stamp, and the components that came off. The stamps are an ordered
/// map, whose dense entries a "changed since" walks faster than a plain
/// map's table; every reader sorts what it finds by node.
#[derive(Default)]
pub struct Ticks {
    stamps: crate::collections::DetHashMap<(Entity, usize), Stamp>,
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

/// Every component in `present` came off `node`, which is being freed. Only
/// a component that holds a stamp is heard going: one the registry never
/// wrote, a node's spawned `transform`, was never heard coming either. Freeing
/// fifty thousand bare nodes is a check each, not a lookup per component.
pub(crate) fn freed(eng: &Engine, node: Entity, present: u128) {
    let now = eng.change_tick();
    with(eng, |ticks| {
        if ticks.stamps.is_empty() {
            return;
        }
        let mut bits = present;
        while bits != 0 {
            let index = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            if ticks.stamps.swap_remove(&(node, index)).is_some() {
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

/// The nodes component `index` went onto or changed on after tick `since`:
/// written through the registry, or by a system through a tracked query.
#[must_use]
pub fn changed_since(eng: &Engine, index: usize, since: u64) -> Vec<Entity> {
    let mut found = matching(eng, index, since, |stamp| stamp.changed);
    let column = eng
        .try_resource::<super::ComponentRegistry>()
        .and_then(|registry| registry.borrow().at(index).and_then(|(_, def)| def.ticks));
    if let Some(column) = column {
        found.extend((column.changed_since)(&eng.world(), since));
        found.sort_unstable_by_key(|node| node.to_bits());
        found.dedup();
    }
    found
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

thread_local! {
    /// The change tick of the system running on this thread: what a tracked
    /// write stores. Set as the engine steps it, and on a pool thread for the
    /// batch it runs.
    static NOW: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Say the change tick now on this thread.
pub(crate) fn set_now(tick: u64) {
    NOW.with(|now| now.set(tick));
}

/// The change tick of the system running on this thread.
#[must_use]
pub fn now_here() -> u64 {
    NOW.with(std::cell::Cell::get)
}

/// A derived component's change tick, in a column beside it: what a write
/// through a tracked query stores, as bevy keeps a tick beside each
/// component.
pub struct Ticked<T> {
    pub changed: u64,
    marker: std::marker::PhantomData<fn() -> T>,
}

impl<T> Ticked<T> {
    #[must_use]
    pub fn at(changed: u64) -> Self {
        Self {
            changed,
            marker: std::marker::PhantomData,
        }
    }
}

/// What a derived component's definition does with its tick column, which
/// its `apply` puts on with the component.
#[derive(Clone, Copy)]
pub struct TickColumn {
    /// Take it off a node the component came off.
    pub detach: fn(&Engine, Entity),
    /// The nodes whose column says written after a tick.
    pub changed_since: fn(&hecs::World, u64) -> Vec<Entity>,
}

impl TickColumn {
    /// The column for component `C`.
    #[must_use]
    pub fn of<C: 'static>() -> Self {
        Self {
            detach: |eng, node| {
                let _ = eng.remove_one::<Ticked<C>>(node);
            },
            changed_since: |world, since| {
                world
                    .query::<(Entity, &Ticked<C>)>()
                    .iter()
                    .filter(|(_, ticked)| ticked.changed > since)
                    .map(|(node, _)| node)
                    .collect()
            },
        }
    }
}

/// A write a system made through a tracked query: the component's Rust type
/// and the node.
type Written = (std::any::TypeId, Entity);

thread_local! {
    /// This thread's writes since it last handed them over.
    static WRITTEN: std::cell::RefCell<Vec<Written>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// What the pool's threads handed over, for the drain on the engine's thread.
static HANDED: std::sync::Mutex<Vec<Written>> = std::sync::Mutex::new(Vec::new());

/// Whether anything waits in [`HANDED`], read without the lock: every
/// system's end drains, and almost none has anything to stamp.
static HANDED_ANY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A tracked query's item was written through: noted on this thread now,
/// stamped when the system that wrote it ends ([`drain_written`]).
pub(crate) fn note_written(type_id: std::any::TypeId, node: Entity) {
    WRITTEN.with(|written| written.borrow_mut().push((type_id, node)));
}

/// Hand this thread's notes to the engine's thread: what a batch run on the
/// pool does when it ends, once rather than a lock a write.
pub(crate) fn hand_over() {
    let mine = WRITTEN.with(|written| std::mem::take(&mut *written.borrow_mut()));
    if !mine.is_empty() {
        HANDED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .extend(mine);
        HANDED_ANY.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// Stamp every write noted since the last drain with the change tick now, a
/// component the registry does not know left alone: what a system's tracked
/// writes become, at the end of its run.
pub(crate) fn drain_written(eng: &Engine) {
    // Read, not taken: the common case is nothing at all.
    if !HANDED_ANY.load(std::sync::atomic::Ordering::Relaxed)
        && WRITTEN.with(|written| written.borrow().is_empty())
    {
        return;
    }
    let mut noted = WRITTEN.with(|written| std::mem::take(&mut *written.borrow_mut()));
    if HANDED_ANY.swap(false, std::sync::atomic::Ordering::Acquire) {
        noted.extend(std::mem::take(
            &mut *HANDED
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        ));
    }
    if noted.is_empty() {
        return;
    }
    let Some(registry) = eng.try_resource::<super::ComponentRegistry>() else {
        return;
    };
    let Some(ticks) = eng.try_resource::<Ticks>() else {
        return;
    };
    let now = eng.change_tick();
    let registry = registry.borrow();
    let mut ticks = ticks.borrow_mut();
    // Looked up once a type: a system writes one or two kinds a run.
    let mut places: Vec<(std::any::TypeId, Option<usize>)> = Vec::new();
    for (type_id, node) in noted {
        let index = if let Some((_, index)) = places.iter().find(|(held, _)| *held == type_id) {
            *index
        } else {
            let index = registry.index_of_type(type_id);
            places.push((type_id, index));
            index
        };
        if let Some(index) = index {
            ticks
                .stamps
                .entry((node, index))
                .and_modify(|stamp| stamp.changed = now)
                .or_insert(Stamp {
                    added: now,
                    changed: now,
                });
        }
    }
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

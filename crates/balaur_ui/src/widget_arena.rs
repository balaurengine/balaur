//! Last pass's arena, and how this pass gets one.
//!
//! Rebuilding the forest walks the world and clones a widget per node, which
//! was a quarter of the pass. So the arena is kept whole between passes and
//! only the slots that were written are re-read.

use std::cell::RefCell;

use balaur_core::Engine;
use balaur_core::hecs::Entity;
use smol_str::SmolStr;

use crate::widget_layer::{Placed, Widget, lays_out};

thread_local! {
    /// Last pass's arena, kept whole. Rebuilding it walked the world, looked
    /// each node's widget and name up and cloned both, which was a quarter of
    /// the pass and answered "nothing moved" every time.
    static ARENA: RefCell<Cached> = RefCell::new(Cached::default());
    /// The widgets written since the last pass, by entity bits. Named rather
    /// than counted: one inspector row changing its number should re-read that
    /// row, not the ten thousand nodes around it.
    pub(crate) static DIRTY: RefCell<rustc_hash::FxHashSet<u64>> = RefCell::new(rustc_hash::FxHashSet::default());
    /// The tree's appearance revision last pass drew at: when it moves, a
    /// node was hidden or shown and every widget's visibility is re-read.
    static SEEN: std::cell::Cell<u64> = const { std::cell::Cell::new(u64::MAX) };
}

#[derive(Default)]
struct Cached {
    arena: Vec<Placed>,
    roots: Vec<usize>,
    /// Which arena slot each entity holds, so a widget that was written can be
    /// refreshed in place rather than by rebuilding the forest around it.
    index_of: rustc_hash::FxHashMap<u64, usize>,
    /// The scene shape and locale the arena was built at. Widget properties
    /// are not in here: those name the entity that changed instead.
    stamp: Option<(u64, u64)>,
}

/// Say that one widget's properties changed, so the next pass re-reads it.
///
/// Every path that writes a `Widget` names it here: the component's `apply`
/// and `remove`, and the input pass writing a click or an edit back.
pub(crate) fn widget_changed(entity: Entity) {
    DIRTY.with(|d| {
        d.borrow_mut().insert(entity.to_bits().get());
    });
}

pub(crate) fn stamp_now(eng: &Engine) -> (u64, u64) {
    use std::hash::{Hash as _, Hasher as _};
    // The locale is a whole-forest change and belongs here: a `text_key` is
    // resolved every pass, so a switch re-captions every widget at once
    // without any component being written.
    let mut hasher = rustc_hash::FxHasher::default();
    balaur_core::strings::locale(eng).hash(&mut hasher);
    (balaur_core::scene::shape_revision(), hasher.finish())
}

/// The arena kept from last pass, when nothing has changed since.
/// Last pass's arena: `Ok` when it still describes the tree, `Err` with its
/// buffers to refill when it does not.
///
/// Taken either way, and never cloned. Handing the buffers back on a miss is
/// what keeps a pass that changed one widget from allocating a second arena
/// beside the one it is about to drop, and from growing it from nothing.
pub(crate) type Arena = (Vec<Placed>, Vec<usize>, rustc_hash::FxHashMap<u64, usize>);

fn kept(stamp: (u64, u64)) -> Result<Arena, Arena> {
    ARENA.with(|held| {
        let mut held = held.borrow_mut();
        let fresh = held.stamp == Some(stamp) && !held.arena.is_empty();
        let taken = (
            std::mem::take(&mut held.arena),
            std::mem::take(&mut held.roots),
            std::mem::take(&mut held.index_of),
        );
        if fresh { Ok(taken) } else { Err(taken) }
    })
}

/// Hand the arena back for the next pass to reuse.
pub(crate) fn keep(
    arena: Vec<Placed>,
    roots: Vec<usize>,
    index_of: rustc_hash::FxHashMap<u64, usize>,
    stamp: (u64, u64),
) {
    ARENA.with(|held| {
        let mut held = held.borrow_mut();
        held.arena = arena;
        held.roots = roots;
        held.index_of = index_of;
        held.stamp = Some(stamp);
    });
}

/// Re-read the widgets written since the last pass, in place.
///
/// `None` where the arena cannot answer for a change and the forest has to be
/// walked again: a node that gained or lost its `widget` component is one the
/// arena has no slot for, or a slot it should no longer hold.
fn patch(
    eng: &Engine,
    arena: &mut [Placed],
    index_of: &rustc_hash::FxHashMap<u64, usize>,
    dirty: &rustc_hash::FxHashSet<u64>,
) -> Option<Vec<usize>> {
    let world = eng.world();
    let mut touched = Vec::with_capacity(dirty.len());
    for bits in dirty {
        let index = *index_of.get(bits)?;
        let entity = arena[index].entity;
        // Gone from the world, or its component removed: either way the shape
        // of the forest is not what the arena says it is.
        let widget = world.get::<&Widget>(entity).ok()?;
        arena[index].widget = Widget::clone(&widget);
        touched.push(index);
    }
    Some(touched)
}

/// The widget forest, in scene-tree order.
///
/// A widget's parent is its nearest *widget* ancestor, not its parent node:
/// a menu is usually a panel with an empty grouping node or two inside it,
/// and the layout should not care. Tree order is sibling order, which is what
/// makes a row read left to right the way the scene reads top to bottom.
fn forest(
    eng: &Engine,
    mut arena: Vec<Placed>,
    mut roots: Vec<usize>,
    mut index_of: rustc_hash::FxHashMap<u64, usize>,
) -> Arena {
    use balaur_core::scene::Children;
    let world = eng.world();
    // Refilled, not rebuilt: last pass's buffers hold room for about as many
    // nodes as this one has, so the walk pushes without growing.
    arena.clear();
    roots.clear();
    index_of.clear();
    // (node, the arena index of the widget laying it out, if any)
    let mut stack: Vec<(Entity, Option<usize>)> = vec![(eng.root(), None)];
    while let Some((entity, owner)) = stack.pop() {
        let mut next_owner = owner;
        if let Ok(widget) = world.get::<&Widget>(entity) {
            let index = arena.len();
            let widget = Widget::clone(&widget);
            let name = world
                .get::<&balaur_core::scene::Name>(entity)
                .map_or_else(|_| SmolStr::default(), |n| SmolStr::new(&n.0));
            // Only a container adopts what is under it; a label with nodes
            // beneath it leaves them to be anchored on their own. Read before
            // the widget moves, so the arena is built with one clone a node.
            next_owner = lays_out(&widget.kind).then_some(index);
            arena.push(Placed {
                entity,
                parent: owner,
                name,
                widget,
                children: Vec::new(),
                look: RefCell::new(None),
                alpha: 1.0,
            });
            index_of.insert(entity.to_bits().get(), index);
            match owner {
                Some(parent) => arena[parent].children.push(index),
                None => roots.push(index),
            }
        }
        if let Ok(children) = world.get::<&Children>(entity) {
            // Pushed in reverse so the stack pops them in declaration order.
            for child in children.0.iter().rev() {
                stack.push((*child, next_owner));
            }
        }
    }
    (arena, roots, index_of)
}

/// What a pass starts from, and whether the arena behind it is new.
pub(crate) struct Begun {
    pub(crate) placed: Vec<Placed>,
    pub(crate) roots: Vec<usize>,
    pub(crate) index_of: rustc_hash::FxHashMap<u64, usize>,
    /// Whether the arena was rebuilt, which is what says the layout pass has
    /// to walk it rather than trust what taffy already holds.
    pub(crate) fresh: bool,
    /// The slots a patch re-read, for a caller that only wants those.
    pub(crate) touched: Vec<usize>,
}

/// Three ways a pass can start. Rebuilt: the tree's shape or the locale
/// moved, so the forest is walked. Patched: some widgets were written, and
/// those slots are re-read where they stand. Kept: neither, and last pass's
/// arena is this pass's.
pub(crate) fn begin(eng: &Engine, stamp: (u64, u64)) -> Begun {
    let written = DIRTY.with(|d| std::mem::take(&mut *d.borrow_mut()));
    let mut fresh = true;
    let mut touched = Vec::new();
    let (mut placed, roots, index_of) = match kept(stamp) {
        Ok((mut arena, roots, index_of)) => {
            match if written.is_empty() {
                Some(Vec::new())
            } else {
                patch(eng, &mut arena, &index_of, &written)
            } {
                Some(patched) => {
                    fresh = false;
                    touched = patched;
                    (arena, roots, index_of)
                }
                None => forest(eng, arena, roots, index_of),
            }
        }
        Err((arena, roots, index_of)) => forest(eng, arena, roots, index_of),
    };
    shown_by_tree(eng, &mut placed, fresh, &mut touched);
    if !fresh {
        // The look is a pass's answer, not the arena's: a theme applied since
        // must not be answered out of the pass that cached it.
        for placed in &placed {
            *placed.look.borrow_mut() = None;
        }
    }
    Begun {
        placed,
        roots,
        index_of,
        fresh,
        touched,
    }
}

/// A widget draws only while its node does: `visible = false` on the node,
/// or on any node above it, hides the widget and everything it lays out, as
/// it hides a sprite; and it draws at its node's inherited tint's alpha, so a
/// fade reaches the UI as it reaches a sprite.
///
/// Read against the tree's appearance revision, so a pass in which nothing
/// was hidden or shown costs nothing; a slot re-read from its component this
/// pass has its own `visible` back and is folded again either way.
fn shown_by_tree(eng: &Engine, placed: &mut [Placed], fresh: bool, touched: &mut Vec<usize>) {
    let revision = balaur_core::scene::appearance_revision();
    let moved = SEEN.with(|seen| seen.replace(revision)) != revision;
    let reread = touched.clone();
    let world = eng.world();
    let mut fold = |index: usize, one: &mut Placed| {
        let authored = world.get::<&Widget>(one.entity).map_or(true, |w| w.visible);
        let (shown, alpha) = world
            .get::<&balaur_core::GlobalAppearance>(one.entity)
            .map_or((true, 1.0), |a| (a.visible, a.tint.w.clamp(0.0, 1.0)));
        // Drawn over, not laid out: a fade changes no size, so it is written
        // without marking the slot for the layout pass.
        one.alpha = alpha;
        let visible = authored && shown;
        if one.widget.visible != visible {
            one.widget.visible = visible;
            if !fresh && !touched.contains(&index) {
                touched.push(index);
            }
        }
    };
    if fresh || moved {
        for (index, one) in placed.iter_mut().enumerate() {
            fold(index, one);
        }
    } else {
        for index in reread {
            if let Some(one) = placed.get_mut(index) {
                fold(index, one);
            }
        }
    }
}

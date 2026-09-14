//! Last pass's arena, and how this pass gets one.
//!
//! Rebuilding the forest walks the world and clones a widget per node, which
//! was a quarter of the pass. So the arena is kept whole between passes and
//! only the slots that were written are re-read.

use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::Engine;
use balaur_core::hecs::Entity;
use egui::Color32;
use smol_str::SmolStr;

use crate::widget::node::{Widget, lays_out};
use crate::widget::theme::{Style, WidgetTheme, face, styled, theme_of};

/// One widget and the widgets laid out inside it, as an arena so the draw can
/// recurse without holding a borrow of the world.
pub(crate) struct Placed {
    pub(crate) entity: Entity,
    /// The arena index of the widget that lays this one out, or `None` for a
    /// root. What lets a patched node work out the theme it inherits without
    /// the walk from the root that put it there.
    pub(crate) parent: Option<usize>,
    /// The node's name: what a tab strip labels a page with when the page
    /// says nothing itself.
    pub(crate) name: SmolStr,
    pub(crate) widget: Widget,
    /// What the widget itself said about being drawn, before the scene tree
    /// had its say. Held because `widget.visible` is folded with the node's
    /// own appearance every pass, and re-reading the world for it would read
    /// the widget as authored rather than as this screen class resolved it.
    pub(crate) authored_visible: bool,
    pub(crate) children: Vec<usize>,
    /// The look resolved for this widget, worked out once a frame.
    ///
    /// Both the measure and the draw ask for it, and each of them more than
    /// once: without this the theme was walked, the role merged and the face
    /// built five times a widget a frame.
    pub(crate) look: RefCell<Option<Rc<Look>>>,
    /// The alpha of the node's inherited tint: every ancestor's multiplied
    /// in, so a fade on a panel reaches what it lays out.
    pub(crate) alpha: f32,
}

/// A widget's resolved look: what to paint it with and what face to draw its
/// caption in, with the theme's roles and its own overrides already applied.
pub(crate) struct Look {
    pub(crate) style: Rc<Style>,
    pub(crate) font: egui::FontId,
    pub(crate) ink: Color32,
}

/// Whether the surface is on the side of every line the widget draws. A
/// surface of nothing is a headless run, which draws everything as authored.
fn fits(widget: &Widget, width: f32, height: f32) -> bool {
    if width <= 0.0 || height <= 0.0 {
        return true;
    }
    (widget.hide_narrower <= 0.0 || width >= widget.hide_narrower)
        && (widget.hide_wider <= 0.0 || width < widget.hide_wider)
        && (widget.hide_shorter <= 0.0 || height >= widget.hide_shorter)
}

/// One widget as this pass sees it: what the scene authored, with any class
/// table the screen answers to folded over it.
fn for_classes(widget: &Widget, classes: &[&str]) -> Widget {
    crate::widget::schema::for_classes(widget, classes).unwrap_or_else(|| Widget::clone(widget))
}

/// The theme in force at one node, folded down its ancestors.
///
/// The walk from the root does this on the way past; a node patched on its
/// own has to climb to it instead.
pub(crate) fn theme_at(
    eng: &Engine,
    arena: &[Placed],
    index: usize,
    base: &Rc<WidgetTheme>,
) -> Rc<WidgetTheme> {
    let mut chain = Vec::new();
    let mut at = Some(index);
    while let Some(i) = at {
        chain.push(i);
        at = arena[i].parent;
    }
    let mut theme = base.clone();
    for i in chain.into_iter().rev() {
        theme = theme_of(eng, &arena[i].widget.theme, &theme);
    }
    theme
}

/// The look of one widget, resolved once and kept for the rest of the frame.
pub(crate) fn look_of(arena: &[Placed], index: usize, theme: &WidgetTheme) -> Rc<Look> {
    let placed = &arena[index];
    if let Some(held) = placed.look.borrow().as_ref() {
        return Rc::clone(held);
    }
    let style = styled(theme, &placed.widget);
    let (ink, font) = face(theme, &style, &placed.widget);
    let made = Rc::new(Look { style, font, ink });
    *placed.look.borrow_mut() = Some(Rc::clone(&made));
    made
}

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
    /// The surface the last pass read visibility against, in whole design
    /// pixels: when it moves, every widget with a line is re-read.
    static SURFACE: std::cell::Cell<(i32, i32)> = const { std::cell::Cell::new((-1, -1)) };
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
    // A rotation is the same shape of change: every class table resolves
    // again, and no component was written to say so.
    pass_classes().hash(&mut hasher);
    (balaur_core::scene::shape_revision(), hasher.finish())
}

thread_local! {
    /// The classes this pass answers to, read once at its start.
    static CLASSES: std::cell::Cell<[&'static str; 3]> = const {
        std::cell::Cell::new([balaur_core::tags::POINTER, balaur_core::facts::TALL, balaur_core::facts::WIDE])
    };
}

/// Read the screen once for the pass; everything after reads this.
pub(crate) fn begin_classes(eng: &Engine) -> [&'static str; 3] {
    let now = read_classes(eng);
    CLASSES.with(|held| held.set(now));
    now
}

/// The classes the pass began with.
pub(crate) fn pass_classes() -> [&'static str; 3] {
    CLASSES.with(std::cell::Cell::get)
}

/// The class words a widget's tables are resolved against this pass, in the
/// order they override: the input class, then the height, then the width.
///
/// Measured against the game's own area rather than the window, so a game
/// played in a small viewport lays out as the viewport, not as the editor
/// around it.
pub(crate) fn read_classes(eng: &Engine) -> [&'static str; 3] {
    let facts = balaur_core::facts::device(eng);
    let [width, height] = facts.design_game_size();
    let lines = crate::class_lines(eng);
    [
        balaur_core::tags::input_class(balaur_core::facts::platform(eng).touchscreen),
        balaur_core::facts::height_class(height, lines),
        balaur_core::facts::width_class(width, lines),
    ]
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
    classes: &[&str],
) -> Option<Vec<usize>> {
    let world = eng.world();
    let mut touched = Vec::with_capacity(dirty.len());
    for bits in dirty {
        let index = *index_of.get(bits)?;
        let entity = arena[index].entity;
        // Gone from the world, or its component removed: either way the shape
        // of the forest is not what the arena says it is.
        let widget = world.get::<&Widget>(entity).ok()?;
        let widget = for_classes(&widget, classes);
        arena[index].authored_visible = widget.visible;
        arena[index].widget = widget;
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
    classes: &[&str],
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
            let widget = for_classes(&widget, classes);
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
                authored_visible: widget.visible,
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
    // What the pass began with, so a widget re-read here resolves against the
    // same words the stamp and the kept arena did.
    let classes = pass_classes();
    let mut fresh = true;
    let mut touched = Vec::new();
    let (mut placed, roots, index_of) = match kept(stamp) {
        Ok((mut arena, roots, index_of)) => {
            match if written.is_empty() {
                Some(Vec::new())
            } else {
                patch(eng, &mut arena, &index_of, &written, &classes)
            } {
                Some(patched) => {
                    fresh = false;
                    touched = patched;
                    (arena, roots, index_of)
                }
                None => forest(eng, arena, roots, index_of, &classes),
            }
        }
        Err((arena, roots, index_of)) => forest(eng, arena, roots, index_of, &classes),
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
    let [width, height] = balaur_core::facts::device(eng).design_game_size();
    let surface = (width as i32, height as i32);
    let resized = SURFACE.with(|seen| seen.replace(surface)) != surface;
    let reread = touched.clone();
    // Each widget with a line, against the room it is laid out in: the
    // nearest container that states a size or grows, where that container
    // drew last pass, or the screen for a root and where none does.
    let roomed: Vec<bool> = placed
        .iter()
        .enumerate()
        .map(|(index, one)| {
            if !has_lines(&one.widget) {
                return true;
            }
            let [w, h] = room_of(placed, index, [width, height]);
            fits(&one.widget, w, h)
        })
        .collect();
    let world = eng.world();
    let mut fold = |index: usize, one: &mut Placed| {
        let authored = one.authored_visible && roomed[index];
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
    if fresh || moved || resized {
        for (index, one) in placed.iter_mut().enumerate() {
            fold(index, one);
        }
    } else {
        for index in &reread {
            if let Some(one) = placed.get_mut(*index) {
                fold(*index, one);
            }
        }
        // A container can change size without the screen doing so, as a dock
        // drag does, so a widget with a line is asked every pass.
        for (index, one) in placed.iter_mut().enumerate() {
            if has_lines(&one.widget) && !reread.contains(&index) {
                fold(index, one);
            }
        }
    }
}

fn has_lines(widget: &Widget) -> bool {
    widget.hide_narrower > 0.0 || widget.hide_wider > 0.0 || widget.hide_shorter > 0.0
}

/// The room a widget's lines are read against, in design pixels.
fn room_of(placed: &[Placed], index: usize, surface: [f32; 2]) -> [f32; 2] {
    let mut up = placed[index].parent;
    while let Some(at) = up {
        let one = &placed[at];
        let w = &one.widget;
        // A root that states a size is a room like any other; one that hugs
        // its contents is the screen's, and so is a line above every root.
        if w.width > 0.0 || w.height > 0.0 || (w.grow > 0.0 && one.parent.is_some()) {
            return crate::widget::arrange::drawn_at(one.entity)
                .map_or(surface, |rect| [rect.width(), rect.height()]);
        }
        up = one.parent;
    }
    surface
}

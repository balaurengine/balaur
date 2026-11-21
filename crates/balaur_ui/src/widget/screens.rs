//! The screen a widget is measured against: the window, or the `viewport` it
//! sits in. Its class tables answer to that screen's classes and its
//! `hide_narrower` and the rest to that screen's room, so two viewports of
//! different widths each lay out as their own size. A viewport is read where
//! it drew last pass, as the layout reads every box.

use std::cell::{Cell, RefCell};

use balaur_core::Engine;
use balaur_core::hecs::Entity;
use rustc_hash::FxHashMap;

use crate::widget::arena::Placed;

type Classes = [&'static str; 3];

thread_local! {
    /// The window's classes this pass.
    static WINDOW: Cell<Classes> = const {
        Cell::new([balaur_core::tags::POINTER, balaur_core::facts::TALL, balaur_core::facts::WIDE])
    };
    /// The classes each viewport answered with when its widgets were read,
    /// so a pass can tell one of them changed.
    static VIEWPORTS: RefCell<FxHashMap<Entity, Classes>> = RefCell::new(FxHashMap::default());
}

/// The classes of a screen `size` design pixels across and down: the input
/// class, then the height's, then the width's, the order they override in.
pub(crate) fn classes_of(eng: &Engine, [width, height]: [f32; 2]) -> Classes {
    let lines = crate::class_lines(eng);
    [
        balaur_core::tags::input_class(balaur_core::facts::platform(eng).touchscreen),
        balaur_core::facts::height_class(height, lines),
        balaur_core::facts::width_class(width, lines),
    ]
}

/// The window, in design pixels.
pub(crate) fn window(eng: &Engine) -> [f32; 2] {
    balaur_core::facts::device(eng).design_size()
}

/// Read the window's classes once for the pass.
pub(crate) fn begin(eng: &Engine) {
    WINDOW.set(classes_of(eng, window(eng)));
}

/// A viewport's size where it drew last pass; `None` before it has drawn.
fn drawn(viewport: Entity) -> Option<[f32; 2]> {
    crate::widget::arrange::drawn_at(viewport)
        .filter(|rect| rect.width() > 0.0 && rect.height() > 0.0)
        .map(|rect| [rect.width(), rect.height()])
}

/// Forget the viewports last read, before the forest is walked again: a
/// viewport freed since is not asked about.
pub(crate) fn forget() {
    VIEWPORTS.with(|held| held.borrow_mut().clear());
}

/// The classes a widget on `screen` answers to: its viewport's, or the
/// window's for none. A viewport not yet drawn answers with `fallback`.
pub(crate) fn classes(
    eng: &Engine,
    placed: &[Placed],
    screen: Option<usize>,
    fallback: Classes,
) -> Classes {
    let Some(viewport) = screen else {
        return WINDOW.get();
    };
    let entity = placed[viewport].entity;
    let now = drawn(entity).map_or(fallback, |size| classes_of(eng, size));
    VIEWPORTS.with(|held| held.borrow_mut().insert(entity, now));
    now
}

/// The room of `screen`: its viewport's, or `window` for none.
pub(crate) fn room(placed: &[Placed], screen: Option<usize>, window: [f32; 2]) -> [f32; 2] {
    screen
        .and_then(|viewport| drawn(placed[viewport].entity))
        .unwrap_or(window)
}

/// What the screens' classes are now, for the arena's stamp: a resize that
/// moves a viewport across a line resolves its widgets' tables again.
pub(crate) fn hash(eng: &Engine, hasher: &mut impl std::hash::Hasher) {
    use std::hash::Hash as _;
    classes_of(eng, window(eng)).hash(hasher);
    VIEWPORTS.with(|held| {
        let mut held: Vec<(Entity, Classes)> =
            held.borrow().iter().map(|(e, c)| (*e, *c)).collect();
        held.sort_by_key(|(entity, _)| entity.to_bits());
        for (entity, was) in held {
            drawn(entity)
                .map_or(was, |size| classes_of(eng, size))
                .hash(hasher);
        }
    });
}

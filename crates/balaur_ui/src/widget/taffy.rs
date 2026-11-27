//! Where every widget goes, decided before anything draws.
//!
//! egui lays out while it draws, which is a frame too late for a container
//! that has to hand its children rects. This mirrors the widget forest into a
//! `taffy` tree, solves it, and hands back an absolute rect per widget; the
//! draw then pins a `Ui` to each one. taffy owns the arithmetic — flex grow,
//! gaps, padding, alignment, wrapping and the minimum sizes — and the only
//! thing asked of this crate is what a leaf measures, which is a font query.

use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

use balaur_core::Engine;
use taffy::compute_leaf_layout;
use taffy::prelude::*;
use taffy::style::{Contain, Direction, Overflow};
use taffy::style_helpers::TaffyMaxContent;

use crate::vocabulary::words as w;
use crate::widget::arena::Placed;
use crate::widget::measure::Measure;
use crate::widget::node::{Widget, lays_out};
use crate::widget::theme::WidgetTheme;

thread_local! {
    /// The tree, kept between frames: a scene that did not change restyles
    /// nothing and taffy re-solves only what it marked dirty.
    static TREE: RefCell<Held> = RefCell::new(Held::default());
}

struct Held {
    tree: TaffyTree<usize>,
    /// One record per widget entity, kept across frames. One map and not
    /// three: every widget asked all three of them every frame. The flag is
    /// set on the node a hidden widget gets as the root of its own solve,
    /// which a `context` menu is: laid out there, `Display::None` in its
    /// parent's tree, and the two must not share a style.
    nodes: FxHashMap<(u64, bool), Kept>,
}

/// What the tree remembers about one widget between frames.
struct Kept {
    /// The node the widget keeps across frames.
    id: NodeId,
    /// Everything the node's style was built from last frame, hashed. Taffy's
    /// `Style` is a few hundred bytes with vectors in it, and building one a
    /// node a frame only to find it equal was most of the sync walk.
    style: u64,
    /// What the leaf measured last frame, or `None` before it has. A leaf's
    /// size comes from its content, which no style comparison can see, so
    /// this is what says whether taffy has to solve it again.
    measure: Option<egui::Vec2>,
}

/// A record for a widget the tree holds no node for yet.
fn kept_of(tree: &mut TaffyTree<usize>, style: Style, index: usize, stamp: u64) -> Kept {
    Kept {
        id: tree
            .new_leaf_with_context(style, index)
            .expect("a taffy tree only fails to make a leaf when out of memory"),
        style: stamp,
        measure: None,
    }
}

impl Default for Held {
    fn default() -> Self {
        let mut tree = TaffyTree::new();
        // Taffy rounds to whole units, and a design pixel is not a device
        // pixel under a zoom or on a Retina display: that cut a 42 px rail to 41.6.
        tree.disable_rounding();
        Self {
            tree,
            nodes: FxHashMap::default(),
        }
    }
}

/// Whether taffy lays this kind's children out, or the kind does it itself.
fn owns_children(kind: &str) -> bool {
    lays_out(kind) && !matches!(kind, w::TABS | w::SCROLL | w::FOLD | w::MENU | w::STACK)
}

/// Which axis a container stacks along, from its kind; `reverse` runs a row
/// or a column from its far end.
fn direction(kind: &str, reverse: bool) -> FlexDirection {
    let reverse = reverse && matches!(kind, w::ROW | w::COLUMN);
    match (kind, reverse) {
        (w::ROW, false) => FlexDirection::Row,
        (w::ROW, true) => FlexDirection::RowReverse,
        (_, false) => FlexDirection::Column,
        (_, true) => FlexDirection::ColumnReverse,
    }
}

/// `safe` qualifies a position only; taffy stops stretching a stretch that
/// carries either word.
fn safety(safe: bool, positional: bool) -> taffy::style::AlignmentSafety {
    match (positional, safe) {
        (false, _) => taffy::style::AlignmentSafety::Default,
        (true, true) => taffy::style::AlignmentSafety::Safe,
        (true, false) => taffy::style::AlignmentSafety::Unsafe,
    }
}

/// An `align_items`, `align_self` or `justify_self` word; `None` for `auto`.
fn align_of(word: &str, safe: bool) -> Option<AlignItems> {
    use taffy::style::AlignItemsKeyword as K;
    let keyword = match word {
        w::AUTO => return None,
        w::START => K::Start,
        w::CENTER => K::Center,
        w::END => K::End,
        w::BASELINE => K::Baseline,
        _ => K::Stretch,
    };
    Some(AlignItems {
        keyword,
        safety: safety(safe, !matches!(keyword, K::Stretch | K::Baseline)),
    })
}

fn content_of(word: &str, safe: bool) -> AlignContent {
    use taffy::style::AlignContentKeyword as K;
    let keyword = match word {
        w::CENTER => K::Center,
        w::STRETCH => K::Stretch,
        w::BETWEEN => K::SpaceBetween,
        w::AROUND => K::SpaceAround,
        w::EVENLY => K::SpaceEvenly,
        // The flex ends, which a `reverse` or a `wrap_reverse` turns round.
        w::END => K::FlexEnd,
        _ => K::FlexStart,
    };
    AlignContent {
        keyword,
        safety: safety(
            safe,
            matches!(keyword, K::Center | K::FlexStart | K::FlexEnd),
        ),
    }
}

/// How a container wraps its children, from its `wrap_children` word.
fn wrap_of(word: &str) -> FlexWrap {
    use crate::vocabulary::words::wrapping as wr;
    match word {
        wr::WRAP => FlexWrap::Wrap,
        wr::WRAP_REVERSE => FlexWrap::WrapReverse,
        wr::BALANCE => FlexWrap::Balance,
        wr::BALANCE_REVERSE => FlexWrap::BalanceReverse,
        _ => FlexWrap::NoWrap,
    }
}

fn overflow_of(word: &str) -> Overflow {
    use crate::vocabulary::words::overflow as o;
    match word {
        o::CLIP => Overflow::Clip,
        o::HIDDEN => Overflow::Hidden,
        o::SCROLL => Overflow::Scroll,
        _ => Overflow::Visible,
    }
}

/// An `absolute` child's distance from one edge; below zero leaves it free.
fn inset_of(px: f32) -> LengthPercentageAuto {
    if px >= 0.0 { length(px) } else { auto() }
}

/// A length in design pixels as taffy takes it, or `auto` for zero — which is
/// what "hug your content" has always meant here.
fn size_or_auto(px: f32) -> Dimension {
    if px > 0.0 { length(px) } else { auto() }
}

/// A floor in design pixels, or none.
///
/// Zero rather than `auto`: CSS gives a flex item an automatic minimum of its
/// own content, so a panel holding a long log would refuse to be narrower
/// than the log and push its neighbours off the row. Nothing here has ever
/// had that floor, and `min_width` is how a scene asks for one.
fn floor_or_none(px: f32) -> LengthPercentageAuto {
    if px > 0.0 { length(px) } else { length(0.0) }
}

/// What a node's style is built from besides the widget and its theme.
#[derive(Clone, Copy)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent facts about one node, each read by its own field of the style"
)]
struct Shape {
    pad: crate::widget::arrange::Pad,
    gap: egui::Vec2,
    /// What a container keeps clear above its children for what it draws
    /// there itself: a panel's caption, a window's title bar.
    band: f32,
    /// How wide what it draws there is, edges included: the least it is.
    heading: f32,
    shown: bool,
    is_root: bool,
    /// A window folded to its bar, which drops its stated height.
    folded: bool,
}

/// Everything [`style_of`] and the `fills` override read, hashed into one
/// number. Must name every input either of them touches: a field left out is
/// a change that never reaches taffy.
fn style_key(
    widget: &Widget,
    style: &crate::widget::theme::Style,
    shape: Shape,
    fills: Fill,
) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let Shape {
        pad,
        gap,
        band,
        heading,
        shown,
        is_root,
        folded,
    } = shape;
    let mut hasher = rustc_hash::FxHasher::default();
    // The role's own size is part of the shape, or a theme swap leaves the
    // node styled by the one before it.
    let asked = crate::widget::arrange::size_of(widget, style);
    asked.x.to_bits().hash(&mut hasher);
    asked.y.to_bits().hash(&mut hasher);
    shown.hash(&mut hasher);
    widget.kind.hash(&mut hasher);
    widget.grow.to_bits().hash(&mut hasher);
    widget.width.to_bits().hash(&mut hasher);
    widget.height.to_bits().hash(&mut hasher);
    widget.min_width.to_bits().hash(&mut hasher);
    widget.min_height.to_bits().hash(&mut hasher);
    widget.align.hash(&mut hasher);
    widget.justify.hash(&mut hasher);
    widget.reverse.hash(&mut hasher);
    widget.inset.map(f32::to_bits).hash(&mut hasher);
    widget.layout.hash(&mut hasher);
    for side in [
        pad.left, pad.top, pad.right, pad.bottom, gap.x, gap.y, band, heading,
    ] {
        side.to_bits().hash(&mut hasher);
    }
    is_root.hash(&mut hasher);
    folded.hash(&mut hasher);
    fit_list(widget, style).is_empty().hash(&mut hasher);
    style.truncate.hash(&mut hasher);
    for side in fills {
        match side {
            Side::Hug => 0_u8.hash(&mut hasher),
            Side::Whole(length) => (1_u8, length.to_bits()).hash(&mut hasher),
            Side::AtLeast(length) => (2_u8, length.to_bits()).hash(&mut hasher),
        }
    }
    hasher.finish()
}

/// The style a node takes, with the box it was handed already applied.
fn styled(
    widget: &Widget,
    style: &crate::widget::theme::Style,
    shape: Shape,
    fills: Fill,
) -> Style {
    let mut want = style_of(widget, style, shape);
    // A menu's box is its button's; the rows it opens take only the room the
    // popup or sheet hands them, not the button's 40 points.
    if shape.is_root && widget.kind == w::MENU {
        rows_free(&mut want);
    }
    // The subtree's own node takes the box it was handed, where it was handed
    // one: a container's child fills its rect, and only a root on a corner
    // sizes itself from what is inside it.
    let [width, height] = fills;
    match width {
        Side::Hug => {}
        Side::Whole(width) => want.size.width = length(width),
        Side::AtLeast(width) => want.min_size.width = length(width.max(widget.min_width)),
    }
    match height {
        Side::Hug => {}
        Side::Whole(height) => want.size.height = length(height),
        Side::AtLeast(height) => want.min_size.height = length(height.max(widget.min_height)),
    }
    want
}

/// A menu's rows' box: nothing of the button's width or place. Its height
/// was never in the way: the rows run down past it, and the popup takes
/// theirs.
fn rows_free(style: &mut Style) {
    style.size.width = auto();
    style.min_size.width = auto();
    style.max_size.width = auto();
    style.position = Position::Relative;
    style.inset = Rect::auto();
}

/// One widget's `taffy::Style`.
///
/// Every layout property is one field here: `grow` is `flex_grow`, `gap` is
/// `gap`, `padding` is `padding`, `align_items` is `align_items`, `justify` is
/// `justify_content`, a `grid` is `Display::Grid` with its tracks, and a
/// hidden widget is `Display::None`.
fn style_of(widget: &Widget, style: &crate::widget::theme::Style, shape: Shape) -> Style {
    let mut asked = crate::widget::arrange::size_of(widget, style);
    if shape.folded {
        asked.y = 0.0;
    }
    if !shape.shown {
        return Style {
            display: Display::None,
            ..Style::default()
        };
    }
    let layout = &widget.layout;
    let container = lays_out(&widget.kind);
    let percent_or = |share: f32, px: f32| {
        if share > 0.0 {
            percent(share / 100.0)
        } else {
            size_or_auto(px)
        }
    };
    let cap = |px: f32| if px > 0.0 { length(px) } else { auto() };
    let [left, top, right, bottom] = layout.border.map(crate::widget::node::Bits::get);
    let margin = layout.margin.map(crate::widget::node::Bits::get);
    let safe = layout.safe_align;
    let (pad, gap) = (shape.pad, shape.gap);
    let mut out = Style {
        display: if widget.kind == w::GRID {
            Display::Grid
        } else {
            Display::Flex
        },
        item_is_replaced: widget.kind == w::IMAGE,
        size: Size {
            width: percent_or(layout.width_percent.get(), asked.x),
            height: percent_or(layout.height_percent.get(), asked.y),
        },
        min_size: Size {
            width: floor_or_none(widget.min_width.max(shape.heading)),
            height: floor_or_none(widget.min_height),
        },
        max_size: Size {
            width: cap(layout.max_width.get()),
            height: cap(layout.max_height.get()),
        },
        aspect_ratio: (layout.aspect_ratio.get() > 0.0).then(|| layout.aspect_ratio.get()),
        margin: Rect {
            left: length(margin[0]),
            top: length(margin[1]),
            right: length(margin[2]),
            bottom: length(margin[3]),
        },
        padding: Rect {
            left: length(pad.left),
            right: length(pad.right),
            top: length(pad.top + shape.band),
            bottom: length(pad.bottom),
        },
        border: Rect {
            left: length(left),
            top: length(top),
            right: length(right),
            bottom: length(bottom),
        },
        gap: Size {
            width: length(gap.x),
            height: length(gap.y),
        },
        align_items: align_of(&widget.align, safe)
            .filter(|_| container)
            .unwrap_or(AlignItems::NORMAL),
        align_self: align_of(&layout.align_self, safe),
        justify_self: align_of(&layout.justify_self, safe),
        align_content: if container {
            content_of(&layout.align_content, safe)
        } else {
            AlignContent::NORMAL
        },
        justify_content: if container {
            content_of(&widget.justify, safe)
        } else {
            AlignContent::NORMAL
        },
        ..Style::default()
    };
    box_into(&mut out, widget, shape.is_root);
    flex_into(&mut out, widget);
    grid_into(&mut out, widget);
    // A fit container stays on one line: its words are what give way.
    if !fit_list(widget, style).is_empty() {
        out.flex_wrap = FlexWrap::NoWrap;
    }
    // A caption its theme cuts gives way in its row for the cut, as CSS's
    // `min-width: 0` lets an ellipsis show.
    if style.truncate == Some(true) && layout.shrink.get() < 0.0 {
        out.flex_shrink = 1.0;
        out.min_size.width = length(widget.min_width.max(0.0));
    }
    out
}

/// The class words a container takes until what it holds fits: its own
/// `fit`, else its theme's.
pub(crate) fn fit_list<'a>(
    widget: &'a Widget,
    style: &'a crate::widget::theme::Style,
) -> &'a [smol_str::SmolStr] {
    if widget.layout.fit.is_empty() {
        style.fit.as_deref().unwrap_or(&[])
    } else {
        &widget.layout.fit
    }
}

/// What box the sizes measure, what it keeps for content past its edge, and
/// where an `absolute` child sits. Taffy lays every node out left to right:
/// a mirrored one is the mirror image of that, which [`gather`] draws, so
/// its paddings, margins and insets swap sides with its children.
fn box_into(out: &mut Style, widget: &Widget, is_root: bool) {
    use crate::vocabulary::words::sizing;
    let layout = &widget.layout;
    out.box_sizing = if layout.box_sizing == sizing::CONTENT {
        BoxSizing::ContentBox
    } else {
        BoxSizing::BorderBox
    };
    out.direction = Direction::Ltr;
    out.overflow = taffy::geometry::Point {
        x: overflow_of(&layout.overflow),
        y: overflow_of(&layout.overflow),
    };
    out.scrollbar_width = layout.scrollbar_width.get();
    out.contain = match layout.contain {
        [true, true] => Contain::CONTENT,
        [true, false] => Contain::LAYOUT,
        [false, true] => Contain::PAINT,
        [false, false] => Contain::NONE,
    };
    // A root is placed by its anchor, never by taffy.
    if layout.absolute && !is_root {
        out.position = Position::Absolute;
        out.inset = Rect {
            left: inset_of(widget.inset[0]),
            top: inset_of(widget.inset[1]),
            right: inset_of(widget.inset[2]),
            bottom: inset_of(widget.inset[3]),
        };
    } else {
        // Taffy's default is static; every box holds its absolute children.
        out.position = Position::Relative;
    }
}

/// The flex fields: the direction from the kind, the wrap, and the share of
/// the line a child grows, shrinks and starts from.
fn flex_into(out: &mut Style, widget: &Widget) {
    let layout = &widget.layout;
    let grow = widget.grow.max(0.0);
    out.flex_direction = direction(&widget.kind, widget.reverse);
    out.flex_wrap = wrap_of(&layout.wrap_children);
    out.flex_line_count = layout.min_lines.max(1);
    out.flex_grow = grow;
    // `flex: 1 1 0` where it grows: content must not inflate the share it
    // starts from. A box that does not grow is never shrunk, but one that
    // scrolls what runs past it: giving way is what it scrolls for.
    out.flex_shrink = if layout.shrink.get() >= 0.0 {
        layout.shrink.get()
    } else if grow > 0.0 || crate::widget::node::scrolls_within(widget) {
        1.0
    } else {
        0.0
    };
    out.flex_basis = if layout.basis.get() >= 0.0 {
        length(layout.basis.get())
    } else if grow > 0.0 {
        length(0.0)
    } else {
        auto()
    };
}

/// A grid's tracks and areas, and any widget's placement in a grid above it.
/// Words that do not read were refused when the widget was applied, so a
/// failure here leaves the field at taffy's default.
fn grid_into(out: &mut Style, widget: &Widget) {
    use crate::widget::grid;
    let layout = &widget.layout;
    out.grid_row = grid::placement(&layout.row).unwrap_or_default();
    out.grid_column = grid::placement(&layout.column).unwrap_or_default();
    if widget.kind != w::GRID {
        return;
    }
    out.grid_template_columns = if layout.grid_columns.is_empty() {
        taffy::style_helpers::evenly_sized_tracks(2)
    } else {
        grid::template(&layout.grid_columns).unwrap_or_default()
    };
    out.grid_template_rows = grid::template(&layout.grid_rows).unwrap_or_default();
    out.grid_auto_columns = grid::auto_tracks(&layout.auto_columns).unwrap_or_default();
    out.grid_auto_rows = grid::auto_tracks(&layout.auto_rows).unwrap_or_default();
    out.grid_auto_flow = {
        use crate::vocabulary::words::flow as f;
        match layout.auto_flow.as_str() {
            f::COLUMN => GridAutoFlow::Column,
            f::ROW_DENSE => GridAutoFlow::RowDense,
            f::COLUMN_DENSE => GridAutoFlow::ColumnDense,
            _ => GridAutoFlow::Row,
        }
    };
    out.grid_template_areas = grid::areas(&layout.areas).ok().flatten();
}

/// The absolute rect of every widget in a solved subtree, by arena index.
pub(crate) type Rects = FxHashMap<usize, egui::Rect>;

/// What a subtree is being solved inside.
pub(crate) struct Room {
    pub(crate) origin: egui::Pos2,
    pub(crate) space: Size<AvailableSpace>,
    /// Whether the subtree's own node takes the whole box or hugs what is in
    /// it, per axis. A container handed a rect fills it; a root anchored to a
    /// corner takes what it measures.
    fill: Fill,
}

/// How the subtree's root takes the box it was handed, per axis.
pub(crate) type Fill = [Side; 2];

/// How the subtree's root takes one axis of the box it was handed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Side {
    /// What is inside it measures: a root on a corner.
    Hug,
    /// The whole box: a container's child.
    Whole(f32),
    /// The box, or longer where what is inside it is: a sideways scroll's
    /// width, so a spring in a short strip still reaches its far end.
    AtLeast(f32),
}

/// Which ways a scroll moves, from its `axis` word: both where it says
/// nothing.
pub(crate) fn scroll_axes(axis: &str) -> (bool, bool) {
    (axis != w::VERTICAL, axis != w::HORIZONTAL)
}

impl Room {
    /// A box of a known size, filled: what a container hands a child.
    pub(crate) fn fixed(rect: egui::Rect) -> Self {
        Self {
            origin: rect.min,
            space: Size {
                width: AvailableSpace::Definite(rect.width()),
                height: AvailableSpace::Definite(rect.height()),
            },
            fill: [Side::Whole(rect.width()), Side::Whole(rect.height())],
        }
    }

    /// A box to stay inside, hugging what is in it: a root on a corner.
    pub(crate) fn hugging(rect: egui::Rect) -> Self {
        Self {
            origin: rect.min,
            space: Size {
                width: AvailableSpace::Definite(rect.width()),
                height: AvailableSpace::Definite(rect.height()),
            },
            fill: [Side::Hug, Side::Hug],
        }
    }

    /// A scroll's inside: free along the way it moves, so the contents take
    /// what they measure and the bar makes up the difference. One that moves
    /// one way fills the other, as a form fills a vertical one.
    pub(crate) fn scrolling(rect: egui::Rect, axis: &str) -> Self {
        let (sideways, downwards) = scroll_axes(axis);
        Self {
            origin: rect.min,
            space: Size {
                width: if sideways && !downwards {
                    AvailableSpace::MAX_CONTENT
                } else {
                    AvailableSpace::Definite(rect.width())
                },
                height: if downwards {
                    AvailableSpace::MAX_CONTENT
                } else {
                    AvailableSpace::Definite(rect.height())
                },
            },
            // One that moves only sideways is at least as wide as itself, as a
            // web page's row is: a spring in a short strip reaches its end.
            fill: [
                match (sideways, downwards) {
                    (false, _) => Side::Whole(rect.width()),
                    (true, false) => Side::AtLeast(rect.width()),
                    (true, true) => Side::Hug,
                },
                if downwards {
                    Side::Hug
                } else {
                    Side::Whole(rect.height())
                },
            ],
        }
    }
}

/// Lay out the subtree at `root` and answer where every widget in it goes.
///
/// The tree is rebuilt from the arena each frame — the arena is itself rebuilt
/// from the world — but the nodes are kept, so taffy restyles only what
/// changed and re-solves only what that dirtied.
#[allow(
    clippy::too_many_arguments,
    reason = "one solve's invariants, threaded down a recursion rather than rebuilt"
)]
pub(crate) fn solve(
    eng: &Engine,
    arena: &[Placed],
    root: usize,
    ui: &egui::Ui,
    theme: &Rc<WidgetTheme>,
    space: &Room,
    fresh: bool,
    touched: &[usize],
) -> Rects {
    let mut measure = Measure::new(eng, arena, ui);
    TREE.with(|held| {
        let mut held = held.borrow_mut();
        // Only the root when the arena is the one taffy was last given: the
        // walk exists to notice changes, and a resize is the one it could not.
        let node = sync(
            &mut held,
            arena,
            root,
            theme,
            &mut measure,
            space.fill,
            true,
            fresh,
        );
        // The slots a write touched, pushed straight at their own nodes: the
        // walk that would have found them is what this pass is skipping.
        for &index in touched {
            // Never this solve's own root: synced above with the box it was
            // handed, it would be restyled here as a child with none.
            if index == root {
                continue;
            }
            let at = crate::widget::arena::theme_at(eng, arena, index, theme);
            sync(
                &mut held,
                arena,
                index,
                &at,
                &mut measure,
                [Side::Hug, Side::Hug],
                false,
                true,
            );
            remeasure_holders(&mut held, arena, root, index, theme, &mut measure);
        }
        let solved = held.tree.compute_layout_with_measure(
            node,
            space.space,
            |inputs, _node, context, style| {
                let index = context.copied();
                compute_leaf_layout(
                    inputs,
                    style,
                    |_, _| 0.0,
                    |known, _available| leaf(index, known, theme, &mut measure),
                )
            },
        );
        if let Err(err) = solved {
            tracing::warn!("widget layout: {err:?}");
            return Rects::default();
        }
        let mut rects = Rects::default();
        // The root sits at the room's corner, whatever taffy says of where a
        // root goes in the space it was offered.
        let size = held.tree.layout(node).map_or(egui::Vec2::ZERO, |layout| {
            egui::vec2(layout.size.width, layout.size.height)
        });
        let whole = egui::Rect::from_min_size(space.origin, size);
        let above = crate::widget::direction::mirrored_above(arena, root);
        gather(&held, arena, (root, node), whole, above, &mut rects);
        if settle_fits(&held, arena, &rects, theme, &mut measure) {
            // The words under a container moved: this pass drew it at the
            // step before, so egui runs the pass again rather than show it.
            ui.ctx().request_discard("a fit container moved a step");
            // A host that runs a frame only when asked still runs this one.
            ui.ctx().request_repaint();
        }
        rects
    })
}

/// Solve one subtree on its own, for a kind that places its own children.
#[allow(
    clippy::too_many_arguments,
    reason = "one solve's invariants, threaded down a recursion rather than rebuilt"
)]
pub(crate) fn solve_subtree(
    eng: &Engine,
    arena: &[Placed],
    root: usize,
    ui: &egui::Ui,
    theme: &Rc<WidgetTheme>,
    space: &Room,
    fresh: bool,
) -> Rects {
    // No touched slots: the pass's first solve pushed them, and the tree they
    // went into is the same one this subtree is solved in.
    solve(eng, arena, root, ui, theme, space, fresh, &[])
}

/// What one leaf needs, asked of the fonts rather than of last frame's draw.
fn leaf(
    index: Option<usize>,
    known: Size<Option<f32>>,
    theme: &Rc<WidgetTheme>,
    measure: &mut Measure<'_>,
) -> Size<f32> {
    let Some(index) = index else {
        return Size::ZERO;
    };
    // A block that wraps is as tall as its box is narrow, so the height has
    // to be measured again once taffy knows the width it settled on.
    if let Some(width) = known.width
        && let Some(size) = measure.wrapped(index, width, theme)
    {
        return Size {
            width,
            height: known.height.unwrap_or(size.y),
        };
    }
    let want = measure.leaf(index, theme);
    Size {
        width: known.width.unwrap_or(want.x),
        height: known.height.unwrap_or(want.y),
    }
}

/// A kind that places its own children (a scroll, a fold, a stack, tabs, a
/// menu) is a leaf in its parent's tree, sized by what it holds: one holding
/// the slot a write touched is measured again, or a list a filter shortened
/// would keep the box it had. Up to the solve's root, which was synced whole.
fn remeasure_holders(
    held: &mut Held,
    arena: &[Placed],
    root: usize,
    touched: usize,
    theme: &Rc<WidgetTheme>,
    measure: &mut Measure<'_>,
) {
    let mut up = arena[touched].parent;
    while let Some(above) = up {
        if above == root {
            return;
        }
        let kind = arena[above].widget.kind.as_str();
        if lays_out(kind) && !owns_children(kind) {
            let at = crate::widget::arena::theme_at(measure.eng, arena, above, theme);
            sync(
                held,
                arena,
                above,
                &at,
                measure,
                [Side::Hug, Side::Hug],
                false,
                true,
            );
        }
        up = arena[above].parent;
    }
}

/// Mirror one widget and its children into the tree, and answer its node.
#[allow(
    clippy::too_many_arguments,
    reason = "one solve's invariants, threaded down a recursion rather than rebuilt"
)]
fn sync(
    held: &mut Held,
    arena: &[Placed],
    index: usize,
    theme: &Rc<WidgetTheme>,
    measure: &mut Measure<'_>,
    fills: Fill,
    is_root: bool,
    deep: bool,
) -> NodeId {
    let placed = &arena[index];
    let widget = &placed.widget;
    let theme = crate::widget::theme::theme_of(measure.eng, &widget.theme, theme);
    let look = crate::widget::arena::look_of(arena, index, &theme);
    // What it asks for, for `ui.natural_size`, on the walk that looks: a
    // hidden one keeps what it asked for when last shown.
    if deep && widget.visible {
        crate::widget::arrange::record_natural(placed.entity, measure.of(index, &theme));
    }
    // A kind that places its own children is measured as a leaf, and so is an
    // empty container. Neither recurses, so the measure can happen here; a
    // folded window keeps only its title bar.
    let folded = widget.kind == w::WINDOW && crate::widget::window::folded(placed.entity, widget);
    let owns = (is_root || owns_children(&widget.kind)) && !folded;
    let leaf = !owns || placed.children.is_empty();
    let shape = Shape {
        // A menu's padding is its button's: the rows it opens sit in the
        // frame its `body` table draws, with that frame's own.
        pad: if is_root && widget.kind == w::MENU {
            crate::widget::arrange::Pad::default()
        } else {
            crate::widget::arrange::padding_only(widget, &look.style)
        },
        gap: crate::widget::arrange::gap_of(widget, &look.style),
        band: if leaf {
            0.0
        } else {
            measure.band(index, &theme)
        },
        heading: if leaf {
            0.0
        } else {
            measure.heading(index, &theme)
        },
        // A solve's root is laid out whatever its `visible` says, on a node
        // of its own: a menu with no button still opens its rows from a `context`.
        shown: widget.visible || is_root,
        is_root,
        folded,
    };
    // A subtree's root is a node apart from the leaf its parent measures: one
    // node holding the subtree's children sized the leaf by them alone.
    let key = (placed.entity.to_bits().get(), is_root);
    let stamp = style_key(widget, &look.style, shape, fills);
    // A leaf's size is its content's, and nothing about the style says the
    // content changed. Measuring once here and comparing is what lets a
    // still screen be solved not at all rather than solved again.
    let want = (deep && leaf).then(|| measure.leaf(index, &theme));
    let node = node_of(held, key, index, stamp, want, || {
        styled(widget, &look.style, shape, fills)
    });
    // A kind that places its own children is a leaf in the tree its parent
    // was solved in, so the subtree solved from it here starts with none.
    let laid = if owns {
        placed
            .children
            .iter()
            .filter(|child| !crate::widget::kinds::in_title_bar(arena, index, **child))
            .count()
    } else {
        0
    };
    let bare = is_root && held.tree.child_count(node) != laid;
    if !deep && !bare {
        // The children taffy holds are the ones this arena put there, and the
        // leaf sizes with them: nothing below this node can have moved.
        return node;
    }
    // A container's children are taffy's, except the five that place their
    // own; each of those solves its subtree separately.
    let kids: Vec<NodeId> = if owns {
        placed
            .children
            .iter()
            .filter(|child| !crate::widget::kinds::in_title_bar(arena, index, **child))
            .map(|child| {
                sync(
                    held,
                    arena,
                    *child,
                    &theme,
                    measure,
                    [Side::Hug, Side::Hug],
                    false,
                    true,
                )
            })
            .collect()
    } else {
        Vec::new()
    };
    if !same_children(&held.tree, node, &kids) {
        let _ = held.tree.set_children(node, &kids);
    }
    node
}

/// The node `key` names, built when it is missing or stale, with its style,
/// context and measure brought up to date. `style` builds the style, called
/// only when the node is built or its `stamp` moved.
fn node_of(
    held: &mut Held,
    key: (u64, bool),
    index: usize,
    stamp: u64,
    want: Option<egui::Vec2>,
    style: impl Fn() -> Style,
) -> NodeId {
    // One lookup for the node, its stamp and what it measured.
    let Held { tree, nodes } = held;
    let kept = nodes
        .entry(key)
        .or_insert_with(|| kept_of(tree, style(), index, stamp));
    // A record can outlive the node it names, when the tree dropped it.
    if tree.style(kept.id).is_err() {
        *kept = kept_of(tree, style(), index, stamp);
    }
    // Only on a change: `set_style` marks the node dirty, and a shell
    // that is not moving should re-solve nothing. The stamp is what
    // says so without building a style to compare against.
    if kept.style != stamp {
        let _ = tree.set_style(kept.id, style());
        kept.style = stamp;
    }
    // Only on a change, because setting a context marks the node dirty and
    // an arena index holds still for as long as the scene does.
    if tree.get_node_context(kept.id).copied() != Some(index) {
        let _ = tree.set_node_context(kept.id, Some(index));
    }
    if want.is_some() && kept.measure != want {
        kept.measure = want;
        let _ = tree.mark_dirty(kept.id);
    }
    kept.id
}

/// Whether a node's children are already these, without asking taffy for a
/// copy of them: `children` clones its vector, once a node a frame.
fn same_children(tree: &TaffyTree<usize>, node: NodeId, kids: &[NodeId]) -> bool {
    if tree.child_count(node) != kids.len() {
        return false;
    }
    kids.iter()
        .enumerate()
        .all(|(slot, kid)| tree.child_at_index(node, slot).is_ok_and(|had| had == *kid))
}

/// Walk the solved tree, turning taffy's parent-relative boxes into the
/// absolute rects the draw pins a `Ui` to. `rect` is where this widget
/// goes; a mirrored one places its children as the mirror image of what
/// taffy laid out left to right (`direction`), `above` saying whether what
/// holds it is.
fn gather(
    held: &Held,
    arena: &[Placed],
    (index, node): (usize, NodeId),
    rect: egui::Rect,
    above: bool,
    out: &mut Rects,
) {
    out.insert(index, rect);
    let flip = crate::widget::direction::mirrored_under(arena, index, above);
    // A child at a time rather than `children`, which clones its vector. A
    // child drawn in a title bar has no node here, so the slots skip it.
    let laid = arena[index]
        .children
        .iter()
        .filter(|child| !crate::widget::kinds::in_title_bar(arena, index, **child));
    for (slot, child) in laid.enumerate() {
        let Ok(kid) = held.tree.child_at_index(node, slot) else {
            continue;
        };
        let Ok(layout) = held.tree.layout(kid) else {
            continue;
        };
        let size = egui::vec2(layout.size.width, layout.size.height);
        let x = crate::widget::direction::placed_x(
            flip,
            rect.min.x + layout.location.x,
            size.x,
            rect.x_range(),
        );
        let at = egui::Rect::from_min_size(egui::pos2(x, rect.min.y + layout.location.y), size);
        gather(held, arena, (*child, kid), at, flip, out);
    }
}

/// Settle every fit container this solve laid out on the step its room
/// holds (`class_words::settle`). `true` when one moved.
fn settle_fits(
    held: &Held,
    arena: &[Placed],
    rects: &Rects,
    theme: &Rc<WidgetTheme>,
    measure: &mut Measure<'_>,
) -> bool {
    let mut moved = false;
    for index in rects.keys() {
        let placed = &arena[*index];
        let bits = placed.entity.to_bits().get();
        let Some(style) = placed.look.borrow().resting_style() else {
            continue;
        };
        let list = fit_list(&placed.widget, &style);
        if list.is_empty() {
            crate::widget::class_words::forget(bits);
            continue;
        }
        if !placed.widget.visible {
            continue;
        }
        let Some(node) = [(bits, false), (bits, true)]
            .iter()
            .find_map(|key| held.nodes.get(key))
            .map(|kept| kept.id)
        else {
            continue;
        };
        let (Ok(layout), Ok(own)) = (held.tree.layout(node), held.tree.style(node)) else {
            continue;
        };
        let row = runs_across(own.flex_direction);
        let room = along(layout.size, row) - sides(layout.padding, row) - sides(layout.border, row);
        let gap = crate::widget::arrange::gap_of(&placed.widget, &style);
        let need = content_need(
            held,
            arena,
            (*index, node),
            (row, axis(gap, row)),
            theme,
            measure,
        );
        let words = crate::widget::class_words::words_of(list);
        moved |= crate::widget::class_words::settle(bits, &words, room, need);
    }
    moved
}

fn runs_across(direction: FlexDirection) -> bool {
    matches!(direction, FlexDirection::Row | FlexDirection::RowReverse)
}

fn along(size: Size<f32>, row: bool) -> f32 {
    if row { size.width } else { size.height }
}

fn axis(vector: egui::Vec2, row: bool) -> f32 {
    if row { vector.x } else { vector.y }
}

fn sides(edges: Rect<f32>, row: bool) -> f32 {
    if row {
        edges.left + edges.right
    } else {
        edges.top + edges.bottom
    }
}

/// What a container's children need along `row`'s axis on one line, with
/// `gap` between each two: a child that keeps its size at what it was laid
/// out at, and one that gives way (it grows, or shrinks for a cut) at what
/// it holds, since the layout may have squeezed or stretched it.
fn content_need(
    held: &Held,
    arena: &[Placed],
    (index, node): (usize, NodeId),
    (row, gap): (bool, f32),
    theme: &Rc<WidgetTheme>,
    measure: &mut Measure<'_>,
) -> f32 {
    let mut total = 0.0;
    let mut count = 0_u16;
    let laid = arena[index]
        .children
        .iter()
        .filter(|child| !crate::widget::kinds::in_title_bar(arena, index, **child));
    for (slot, child) in laid.enumerate() {
        let Ok(kid) = held.tree.child_at_index(node, slot) else {
            continue;
        };
        let (Ok(style), Ok(layout)) = (held.tree.style(kid), held.tree.layout(kid)) else {
            continue;
        };
        if style.display == Display::None || style.position == Position::Absolute {
            continue;
        }
        let gives = style.flex_grow > 0.0 || style.flex_shrink > 0.0;
        let size = if gives {
            natural(held, arena, (*child, kid), row, theme, measure)
        } else {
            along(layout.size, row)
        };
        let margin = if row {
            layout.margin.left + layout.margin.right
        } else {
            layout.margin.top + layout.margin.bottom
        };
        total += size + margin;
        count += 1;
    }
    total + gap * f32::from(count.saturating_sub(1))
}

/// What one widget holds along `row`'s axis, its padding and border in, at
/// least its `min_width` (or height), and its stated size where it states one.
fn natural(
    held: &Held,
    arena: &[Placed],
    (index, node): (usize, NodeId),
    row: bool,
    theme: &Rc<WidgetTheme>,
    measure: &mut Measure<'_>,
) -> f32 {
    let widget = &arena[index].widget;
    let (stated, least) = if row {
        (widget.width, widget.min_width)
    } else {
        (widget.height, widget.min_height)
    };
    if stated > 0.0 {
        return stated;
    }
    let Ok(layout) = held.tree.layout(node) else {
        return least;
    };
    let edge = sides(layout.padding, row) + sides(layout.border, row);
    let inner = if held.tree.child_count(node) == 0 {
        let at = crate::widget::arena::theme_at(measure.eng, arena, index, theme);
        axis(measure.leaf(index, &at), row)
    } else {
        let same_way = held
            .tree
            .style(node)
            .is_ok_and(|own| runs_across(own.flex_direction) == row);
        if same_way {
            let at = crate::widget::arena::theme_at(measure.eng, arena, index, theme);
            let look = crate::widget::arena::look_of(arena, index, &at);
            let gap = axis(crate::widget::arrange::gap_of(widget, &look.style), row);
            content_need(held, arena, (index, node), (row, gap), theme, measure)
        } else {
            // Across its run: as much as its biggest child.
            let mut most = 0.0_f32;
            for slot in 0..held.tree.child_count(node) {
                if let Ok(kid) = held.tree.child_at_index(node, slot)
                    && let Ok(laid) = held.tree.layout(kid)
                {
                    most = most.max(along(laid.size, row));
                }
            }
            most
        }
    };
    (inner + edge).max(least)
}

/// Drop the nodes of widgets that are gone, so a session that opens and
/// closes a thousand panels does not grow the tree forever.
pub(crate) fn sweep(eng: &Engine) {
    TREE.with(|held| {
        let mut held = held.borrow_mut();
        let world = eng.world();
        let gone: Vec<(u64, bool)> = held
            .nodes
            .keys()
            .copied()
            .filter(|(bits, _)| {
                balaur_core::hecs::Entity::from_bits(*bits).is_none_or(|e| !world.contains(e))
            })
            .collect();
        for key in gone {
            if let Some(kept) = held.nodes.remove(&key) {
                let _ = held.tree.remove(kept.id);
            }
            crate::widget::class_words::forget(key.0);
            crate::widget::arrange::forget_natural(key.0);
        }
    });
}

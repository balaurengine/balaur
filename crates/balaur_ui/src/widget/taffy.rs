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

/// Which axis a container stacks along, from its kind; `reverse` runs a row,
/// a column or a flow from its far end.
fn direction(kind: &str, reverse: bool) -> FlexDirection {
    let reverse = reverse && matches!(kind, w::ROW | w::COLUMN | w::FLOW);
    match (kind, reverse) {
        (w::ROW | w::FLOW, false) => FlexDirection::Row,
        (w::ROW | w::FLOW, true) => FlexDirection::RowReverse,
        (_, false) => FlexDirection::Column,
        (_, true) => FlexDirection::ColumnReverse,
    }
}

fn safety(safe: bool) -> taffy::style::AlignmentSafety {
    if safe {
        taffy::style::AlignmentSafety::Safe
    } else {
        taffy::style::AlignmentSafety::Unsafe
    }
}

/// An `align_items` or `align_self` word; `None` for `auto`.
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
        safety: safety(safe),
    })
}

/// A `justify` or `align_content` word.
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
        safety: safety(safe),
    }
}

/// How a container wraps its children, from its `wrap_children` word.
fn wrap_of(kind: &str, word: &str) -> FlexWrap {
    use crate::vocabulary::words::wrapping as wr;
    match word {
        wr::NONE => FlexWrap::NoWrap,
        wr::WRAP => FlexWrap::Wrap,
        wr::WRAP_REVERSE => FlexWrap::WrapReverse,
        wr::BALANCE => FlexWrap::Balance,
        wr::BALANCE_REVERSE => FlexWrap::BalanceReverse,
        _ if kind == w::FLOW => FlexWrap::Wrap,
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
    drawn: bool,
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
        drawn,
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
    for side in [pad.left, pad.top, pad.right, pad.bottom, gap.x, gap.y, band] {
        side.to_bits().hash(&mut hasher);
    }
    drawn.hash(&mut hasher);
    is_root.hash(&mut hasher);
    folded.hash(&mut hasher);
    for side in fills {
        side.map(f32::to_bits).hash(&mut hasher);
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
    // The subtree's own node takes the box it was handed, where it was handed
    // one: a container's child fills its rect, and only a root on a corner
    // sizes itself from what is inside it.
    if let Some(width) = fills[0] {
        want.size.width = length(width);
    }
    if let Some(height) = fills[1] {
        want.size.height = length(height);
    }
    // A scroll is a root in its own solve and a child in its parent's, on one
    // node: filled one way it keeps its `grow`, which is what the parent's
    // solve still needs the other way.
    if fills[0].is_some() && fills[1].is_some() {
        want.flex_grow = 0.0;
    }
    want
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
            width: floor_or_none(widget.min_width),
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
        align_items: if container {
            align_of(&widget.align, safe)
        } else {
            None
        },
        align_self: align_of(&layout.align_self, safe),
        align_content: container.then(|| content_of(&layout.align_content, safe)),
        justify_content: container.then(|| content_of(&widget.justify, safe)),
        ..Style::default()
    };
    box_into(&mut out, widget, shape.is_root);
    flex_into(&mut out, widget, shape.drawn);
    grid_into(&mut out, widget);
    out
}

/// What box the sizes measure, which way it runs, what it keeps for content
/// past its edge, and where an `absolute` child sits.
fn box_into(out: &mut Style, widget: &Widget, is_root: bool) {
    use crate::vocabulary::words::{direction, sizing};
    let layout = &widget.layout;
    out.box_sizing = if layout.box_sizing == sizing::CONTENT {
        BoxSizing::ContentBox
    } else {
        BoxSizing::BorderBox
    };
    out.direction = if layout.direction == direction::RIGHT_TO_LEFT {
        Direction::Rtl
    } else {
        Direction::Ltr
    };
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
    }
}

/// The flex fields: the direction from the kind, the wrap, and the share of
/// the line a child grows, shrinks and starts from.
fn flex_into(out: &mut Style, widget: &Widget, drawn: bool) {
    let layout = &widget.layout;
    let grow = if widget.grow > 0.0 {
        widget.grow
    } else if widget.kind == w::DRAW && !drawn && widget.width <= 0.0 && widget.height <= 0.0 {
        1.0
    } else {
        0.0
    };
    out.flex_direction = direction(&widget.kind, widget.reverse);
    out.flex_wrap = wrap_of(&widget.kind, &layout.wrap_children);
    out.flex_line_count = layout.min_lines.max(1);
    out.flex_grow = grow;
    // `flex: 1 1 0` where it grows: content must not inflate the share it
    // starts from. A box that does not grow is never shrunk.
    out.flex_shrink = if layout.shrink.get() >= 0.0 {
        layout.shrink.get()
    } else if grow > 0.0 {
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

/// A stated width and height the subtree's root takes, each where it is
/// `Some`.
pub(crate) type Fill = [Option<f32>; 2];

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
            fill: [Some(rect.width()), Some(rect.height())],
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
            fill: [None, None],
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
            fill: [
                (!sideways).then_some(rect.width()),
                (!downwards).then_some(rect.height()),
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
                [None, None],
                false,
                true,
            );
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
        // The root sits at the room's corner: taffy puts a right-to-left
        // root at the far side of the space it was offered.
        let corner = held.tree.layout(node).map_or(egui::Vec2::ZERO, |layout| {
            egui::vec2(layout.location.x, layout.location.y)
        });
        gather(&held, arena, root, node, space.origin - corner, &mut rects);
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
    let rects = solve(eng, arena, root, ui, theme, space, fresh, &[]);
    // Solving a node as a root leaves taffy holding a location of zero for
    // it, which a later solve that changes nothing would hand the draw.
    let key = (
        arena[root].entity.to_bits().get(),
        !arena[root].widget.visible,
    );
    TREE.with(|held| {
        let Held { tree, nodes } = &mut *held.borrow_mut();
        if let Some(kept) = nodes.get(&key) {
            let _ = tree.mark_dirty(kept.id);
        }
    });
    rects
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
    // A kind that places its own children is measured as a leaf, and so is an
    // empty container. Neither recurses, so the measure can happen here; a
    // folded window keeps only its title bar.
    let folded = widget.kind == w::WINDOW && crate::widget::window::folded(placed.entity, widget);
    let owns = (is_root || owns_children(&widget.kind)) && !folded;
    let leaf = !owns || placed.children.is_empty();
    let shape = Shape {
        pad: crate::widget::arrange::padding_only(widget, &look.style),
        gap: crate::widget::arrange::gap_of(widget, &look.style),
        band: if leaf {
            0.0
        } else {
            measure.band(index, &theme)
        },
        drawn: crate::widget::arrange::measured_of(placed.entity) != egui::Vec2::ZERO,
        // A solve's root is laid out whatever its `visible` says, on a node
        // of its own: a hidden menu still opens its rows from a `context`.
        shown: widget.visible || is_root,
        is_root,
        folded,
    };
    let key = (placed.entity.to_bits().get(), is_root && !widget.visible);
    let stamp = style_key(widget, &look.style, shape, fills);
    let node = {
        // One lookup for the node, its stamp and what it measured.
        let Held { tree, nodes } = &mut *held;
        let kept = nodes.entry(key).or_insert_with(|| {
            kept_of(
                tree,
                styled(widget, &look.style, shape, fills),
                index,
                stamp,
            )
        });
        // A record can outlive the node it names, when the tree dropped it.
        if tree.style(kept.id).is_err() {
            *kept = kept_of(
                tree,
                styled(widget, &look.style, shape, fills),
                index,
                stamp,
            );
        }
        // Only on a change: `set_style` marks the node dirty, and a shell
        // that is not moving should re-solve nothing. The stamp is what
        // says so without building a style to compare against.
        if kept.style != stamp {
            let _ = tree.set_style(kept.id, styled(widget, &look.style, shape, fills));
            kept.style = stamp;
        }
        // Only on a change, because setting a context marks the node dirty and
        // an arena index holds still for as long as the scene does.
        if tree.get_node_context(kept.id).copied() != Some(index) {
            let _ = tree.set_node_context(kept.id, Some(index));
        }
        // A leaf's size is its content's, and nothing about the style says the
        // content changed. Measuring once here and comparing is what lets a
        // still screen be solved not at all rather than solved again.
        if deep && leaf {
            let want = measure.leaf(index, &theme);
            if kept.measure != Some(want) {
                kept.measure = Some(want);
                let _ = tree.mark_dirty(kept.id);
            }
        }
        kept.id
    };
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
                    [None, None],
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
/// absolute rects the draw pins a `Ui` to.
fn gather(
    held: &Held,
    arena: &[Placed],
    index: usize,
    node: NodeId,
    origin: egui::Pos2,
    out: &mut Rects,
) {
    let Ok(layout) = held.tree.layout(node) else {
        return;
    };
    let at = origin + egui::vec2(layout.location.x, layout.location.y);
    let rect = egui::Rect::from_min_size(at, egui::vec2(layout.size.width, layout.size.height));
    out.insert(index, rect);
    // A child at a time rather than `children`, which clones its vector.
    for (slot, child) in arena[index].children.iter().enumerate() {
        let Ok(kid) = held.tree.child_at_index(node, slot) else {
            continue;
        };
        gather(held, arena, *child, kid, at, out);
    }
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
        }
    });
}

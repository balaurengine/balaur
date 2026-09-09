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
use taffy::style_helpers::TaffyMaxContent;

use crate::vocabulary::words as w;
use crate::widget_layer::{Placed, Widget, lays_out};
use crate::widget_measure::Measure;
use crate::widget_theme::WidgetTheme;

thread_local! {
    /// The tree, kept between frames: a scene that did not change restyles
    /// nothing and taffy re-solves only what it marked dirty.
    static TREE: RefCell<Held> = RefCell::new(Held::default());
}

struct Held {
    tree: TaffyTree<usize>,
    /// One record per widget entity, kept across frames. One map and not
    /// three: every widget asked all three of them every frame.
    nodes: FxHashMap<u64, Kept>,
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
        // Rounding to whole device pixels loses the fraction a design pixel
        // carries at a scale like 1.25, which cut a 42 px rail to 41.6.
        tree.disable_rounding();
        Self {
            tree,
            nodes: FxHashMap::default(),
        }
    }
}

/// Whether taffy lays this kind's children out, or the kind does it itself.
fn owns_children(kind: &str) -> bool {
    lays_out(kind) && !matches!(kind, w::TAB | w::SCROLL | w::GRID | w::FLOW | w::FOLD)
}

/// Which axis a container stacks along, from its kind.
fn direction(kind: &str) -> FlexDirection {
    match kind {
        w::ROW | w::FLOW => FlexDirection::Row,
        _ => FlexDirection::Column,
    }
}

fn align_of(word: &str) -> AlignItems {
    match word {
        w::CENTER => AlignItems::CENTER,
        w::END => AlignItems::END,
        // Stretch, not Start: a child across the container's direction fills
        // it unless the author asked for something else, which is the rule
        // every scene written before `align` was laid out under.
        _ => AlignItems::STRETCH,
    }
}

fn justify_of(word: &str) -> JustifyContent {
    match word {
        w::CENTER => JustifyContent::CENTER,
        w::END => JustifyContent::END,
        w::BETWEEN => JustifyContent::SPACE_BETWEEN,
        w::AROUND => JustifyContent::SPACE_AROUND,
        w::EVENLY => JustifyContent::SPACE_EVENLY,
        _ => JustifyContent::START,
    }
}

/// A length in design pixels as taffy takes it, or `auto` for zero — which is
/// what "hug your content" has always meant here.
fn size_or_auto(px: f32, scale: f32) -> Dimension {
    if px > 0.0 { length(px * scale) } else { auto() }
}

/// A floor in design pixels, or none.
///
/// Zero rather than `auto`: CSS gives a flex item an automatic minimum of its
/// own content, so a panel holding a long log would refuse to be narrower
/// than the log and push its neighbours off the row. Nothing here has ever
/// had that floor, and `min_width` is how a scene asks for one.
fn floor_or_none(px: f32, scale: f32) -> LengthPercentageAuto {
    if px > 0.0 {
        length(px * scale)
    } else {
        length(0.0)
    }
}

/// One widget's `taffy::Style`.
///
/// Every property the widget layer had before is one field here: `grow` is
/// `flex_grow`, `gap` is `gap`, `padding` is `padding`, `align` is
/// `align_items`, `justify` is `justify_content`, `columns` is how many a
/// `flow` puts on a line, and `visible = false` is `Display::None`.
/// Everything [`style_of`] and the `fills` override read, hashed into one
/// number. Must name every input either of them touches: a field left out is
/// a change that never reaches taffy.
fn style_key(widget: &Widget, pad: f32, scale: f32, drawn: bool, fills: Option<egui::Vec2>) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = rustc_hash::FxHasher::default();
    widget.visible.hash(&mut hasher);
    widget.kind.hash(&mut hasher);
    widget.grow.to_bits().hash(&mut hasher);
    widget.width.to_bits().hash(&mut hasher);
    widget.height.to_bits().hash(&mut hasher);
    widget.min_width.to_bits().hash(&mut hasher);
    widget.min_height.to_bits().hash(&mut hasher);
    widget.gap.to_bits().hash(&mut hasher);
    widget.align.hash(&mut hasher);
    widget.justify.hash(&mut hasher);
    pad.to_bits().hash(&mut hasher);
    scale.to_bits().hash(&mut hasher);
    drawn.hash(&mut hasher);
    fills
        .map(|f| (f.x.to_bits(), f.y.to_bits()))
        .hash(&mut hasher);
    hasher.finish()
}

/// The style a node takes, with the box it was handed already applied.
fn styled(widget: &Widget, pad: f32, scale: f32, drawn: bool, fills: Option<egui::Vec2>) -> Style {
    let mut want = style_of(widget, pad, scale, drawn);
    // The subtree's own node takes the box it was handed, where it was handed
    // one: a container's child fills its rect, and only a root on a corner
    // sizes itself from what is inside it.
    if let Some(box_size) = fills {
        want.size = Size {
            width: length(box_size.x),
            height: length(box_size.y),
        };
        want.flex_grow = 0.0;
    }
    want
}

fn style_of(widget: &Widget, pad: f32, scale: f32, drawn: bool) -> Style {
    if !widget.visible {
        return Style {
            display: Display::None,
            ..Style::default()
        };
    }
    let container = lays_out(&widget.kind);
    let grow = if widget.grow > 0.0 {
        widget.grow
    } else if widget.kind == w::DRAW && !drawn && widget.width <= 0.0 && widget.height <= 0.0 {
        1.0
    } else {
        0.0
    };
    Style {
        display: Display::Flex,
        flex_direction: direction(&widget.kind),
        flex_wrap: if widget.kind == w::FLOW {
            FlexWrap::Wrap
        } else {
            FlexWrap::NoWrap
        },
        flex_grow: grow,
        // `flex: 1 1 0` where it grows: content must not inflate the share it
        // starts from. A box that does not grow is never shrunk.
        flex_shrink: if grow > 0.0 { 1.0 } else { 0.0 },
        flex_basis: if grow > 0.0 { length(0.0) } else { auto() },
        size: Size {
            width: size_or_auto(widget.width, scale),
            height: size_or_auto(widget.height, scale),
        },
        min_size: Size {
            width: floor_or_none(widget.min_width, scale),
            height: floor_or_none(widget.min_height, scale),
        },
        gap: Size {
            width: length(widget.gap * scale),
            height: length(widget.gap * scale),
        },
        padding: Rect {
            left: length(pad),
            right: length(pad),
            top: length(pad),
            bottom: length(pad),
        },
        align_items: container.then(|| align_of(&widget.align)),
        justify_content: container.then(|| justify_of(&widget.justify)),
        ..Style::default()
    }
}

/// The absolute rect of every widget in a solved subtree, by arena index.
pub(crate) type Rects = FxHashMap<usize, egui::Rect>;

/// What a subtree is being solved inside.
pub(crate) struct Room {
    pub(crate) origin: egui::Pos2,
    pub(crate) space: Size<AvailableSpace>,
    /// Whether the subtree's own node takes the whole box or hugs what is in
    /// it. A container handed a rect fills it; a root anchored to a corner
    /// takes what it measures.
    fill: Option<egui::Vec2>,
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
            fill: Some(rect.size()),
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
            fill: None,
        }
    }

    /// A box bounded across and free along: what a scroll gives its contents.
    pub(crate) fn scrolling(rect: egui::Rect) -> Self {
        Self {
            origin: rect.min,
            space: Size {
                width: AvailableSpace::Definite(rect.width()),
                height: AvailableSpace::MAX_CONTENT,
            },
            fill: None,
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
    scale: f32,
    theme: &Rc<WidgetTheme>,
    space: &Room,
    fresh: bool,
    touched: &[usize],
) -> Rects {
    let mut measure = Measure::new(eng, arena, ui, scale);
    TREE.with(|held| {
        let mut held = held.borrow_mut();
        // Only the root when the arena is the one taffy was last given: the
        // walk exists to notice changes, and a resize is the one it could not.
        let node = sync(
            &mut held,
            arena,
            root,
            theme,
            scale,
            &mut measure,
            space.fill,
            true,
            fresh,
        );
        // The slots a write touched, pushed straight at their own nodes: the
        // walk that would have found them is what this pass is skipping.
        for &index in touched {
            let at = crate::widget_layer::theme_at(arena, index, theme);
            sync(
                &mut held,
                arena,
                index,
                &at,
                scale,
                &mut measure,
                None,
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
        gather(&held, arena, root, node, space.origin, &mut rects);
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
    scale: f32,
    theme: &Rc<WidgetTheme>,
    space: &Room,
    fresh: bool,
) -> Rects {
    // No touched slots: the pass's first solve pushed them, and the tree they
    // went into is the same one this subtree is solved in.
    solve(eng, arena, root, ui, scale, theme, space, fresh, &[])
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
    scale: f32,
    measure: &mut Measure<'_>,
    fills: Option<egui::Vec2>,
    is_root: bool,
    deep: bool,
) -> NodeId {
    let placed = &arena[index];
    let widget = &placed.widget;
    let theme = crate::widget_layer::theme_of_owned(&widget.theme, theme);
    let look = crate::widget_layer::look_of(arena, index, &theme, scale);
    let pad = crate::widget_arrange::padding_of(widget, &look.style, scale);
    let drawn = crate::widget_arrange::measured_of(placed.entity) != egui::Vec2::ZERO;
    let key = placed.entity.to_bits().get();
    let stamp = style_key(widget, pad, scale, drawn, fills);
    // A kind that places its own children is measured as a leaf, and so is an
    // empty container. Neither recurses, so the measure can happen here.
    let owns = is_root || owns_children(&widget.kind);
    let leaf = !owns || placed.children.is_empty();
    let node = {
        // One lookup for the node, its stamp and what it measured.
        let Held { tree, nodes } = &mut *held;
        let kept = nodes.entry(key).or_insert_with(|| {
            kept_of(tree, styled(widget, pad, scale, drawn, fills), index, stamp)
        });
        // A record can outlive the node it names, when the tree dropped it.
        if tree.style(kept.id).is_err() {
            *kept = kept_of(tree, styled(widget, pad, scale, drawn, fills), index, stamp);
        }
        // Only on a change: `set_style` marks the node dirty, and a shell
        // that is not moving should re-solve nothing. The stamp is what
        // says so without building a style to compare against.
        if kept.style != stamp {
            let _ = tree.set_style(kept.id, styled(widget, pad, scale, drawn, fills));
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
    if !deep {
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
            .map(|child| {
                sync(
                    held, arena, *child, &theme, scale, measure, None, false, true,
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
        let gone: Vec<u64> = held
            .nodes
            .keys()
            .copied()
            .filter(|bits| {
                balaur_core::hecs::Entity::from_bits(*bits).is_none_or(|e| !world.contains(e))
            })
            .collect();
        for bits in gone {
            if let Some(kept) = held.nodes.remove(&bits) {
                let _ = held.tree.remove(kept.id);
            }
        }
    });
}

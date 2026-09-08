//! Where every widget goes, decided before anything draws.
//!
//! egui lays out while it draws, which is a frame too late for a container
//! that has to hand its children rects. This mirrors the widget forest into a
//! `taffy` tree, solves it, and hands back an absolute rect per widget; the
//! draw then pins a `Ui` to each one. taffy owns the arithmetic — flex grow,
//! gaps, padding, alignment, wrapping and the minimum sizes — and the only
//! thing asked of this crate is what a leaf measures, which is a font query.

use std::cell::RefCell;
use std::collections::HashMap;
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
    /// One node per widget entity, so a widget keeps its node across frames.
    nodes: HashMap<u64, NodeId>,
}

impl Default for Held {
    fn default() -> Self {
        let mut tree = TaffyTree::new();
        // Rounding to whole device pixels loses the fraction a design pixel
        // carries at a scale like 1.25, which cut a 42 px rail to 41.6.
        tree.disable_rounding();
        Self {
            tree,
            nodes: HashMap::new(),
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
pub(crate) type Rects = HashMap<usize, egui::Rect>;

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
pub(crate) fn solve(
    eng: &Engine,
    arena: &[Placed],
    root: usize,
    ui: &egui::Ui,
    scale: f32,
    theme: &Rc<WidgetTheme>,
    room: &Room,
) -> Rects {
    let mut measure = Measure::new(eng, arena, ui, scale);
    TREE.with(|held| {
        let mut held = held.borrow_mut();
        let node = sync(
            &mut held,
            arena,
            root,
            theme,
            scale,
            &mut measure,
            room.fill,
            true,
        );
        let solved = held.tree.compute_layout_with_measure(
            node,
            room.space,
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
            return Rects::new();
        }
        let mut rects = Rects::new();
        gather(&held, arena, root, node, room.origin, &mut rects);
        rects
    })
}

/// Solve one subtree on its own, for a kind that places its own children.
pub(crate) fn solve_subtree(
    eng: &Engine,
    arena: &[Placed],
    root: usize,
    ui: &egui::Ui,
    scale: f32,
    theme: &Rc<WidgetTheme>,
    room: &Room,
) -> Rects {
    solve(eng, arena, root, ui, scale, theme, room)
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
fn sync(
    held: &mut Held,
    arena: &[Placed],
    index: usize,
    theme: &Rc<WidgetTheme>,
    scale: f32,
    measure: &mut Measure<'_>,
    fills: Option<egui::Vec2>,
    is_root: bool,
) -> NodeId {
    let placed = &arena[index];
    let widget = &placed.widget;
    let theme = crate::widget_layer::theme_of_owned(&widget.theme, theme);
    let style = crate::widget_layer::styled(&theme, widget);
    let pad = crate::widget_arrange::padding_of(widget, &style, scale);
    let drawn = crate::widget_arrange::measured_of(placed.entity) != egui::Vec2::ZERO;
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
    let key = placed.entity.to_bits().get();
    let node = match held.nodes.get(&key).copied() {
        Some(node) if held.tree.style(node).is_ok() => node,
        _ => {
            let made = held
                .tree
                .new_leaf_with_context(want.clone(), index)
                .expect("a taffy tree only fails to make a leaf when out of memory");
            held.nodes.insert(key, made);
            made
        }
    };
    // Only on a change: `set_style` marks the node dirty, and a shell that is
    // not moving should re-solve nothing.
    if held.tree.style(node).is_ok_and(|held| held != &want) {
        let _ = held.tree.set_style(node, want);
    }
    let _ = held.tree.set_node_context(node, Some(index));
    // A container's children are taffy's, except the five that place their
    // own; each of those solves its subtree separately.
    let kids: Vec<NodeId> = if is_root || owns_children(&widget.kind) {
        placed
            .children
            .iter()
            .map(|child| sync(held, arena, *child, &theme, scale, measure, None, false))
            .collect()
    } else {
        Vec::new()
    };
    let same = held
        .tree
        .children(node)
        .is_ok_and(|had| had.as_slice() == kids.as_slice());
    if !same {
        let _ = held.tree.set_children(node, &kids);
    }
    node
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
    let Ok(kids) = held.tree.children(node) else {
        return;
    };
    for (slot, child) in arena[index].children.iter().enumerate() {
        let Some(kid) = kids.get(slot) else {
            continue;
        };
        gather(held, arena, *child, *kid, at, out);
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
            if let Some(node) = held.nodes.remove(&bits) {
                let _ = held.tree.remove(node);
            }
        }
    });
}

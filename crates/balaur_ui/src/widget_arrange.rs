//! How a container decides where its children go: the sizing rules, the
//! containers that apply them, and the measurement a hugging child needs.
//!
//! Split from `widget_layer` because that file is the component and the walk
//! over the world, and this is the arithmetic between them.

use crate::vocabulary::words as w;
use crate::widget_layer::{Edit, Painting, Widget, draw_one};
use balaur_core::hecs::Entity;
use egui::{Color32, Stroke, pos2, vec2};
use rustc_hash::FxHashMap;
use smol_str::SmolStr;
use std::cell::RefCell;

thread_local! {
    /// What each widget drew last frame. Only a `draw` node needs it now —
    /// everything else the layer draws it can also measure, and a rect a
    /// script fills is the one thing it can only remember.
    static MEASURED: RefCell<FxHashMap<u64, egui::Vec2>> = const { RefCell::new(FxHashMap::with_hasher(rustc_hash::FxBuildHasher)) };
    static MEASURING: RefCell<FxHashMap<u64, egui::Vec2>> = const { RefCell::new(FxHashMap::with_hasher(rustc_hash::FxBuildHasher)) };
    /// Where each widget was drawn, for a script that has to place something
    /// against it — the editor's own chrome reads its shell back this way.
    static PLACED: RefCell<FxHashMap<u64, egui::Rect>> = const { RefCell::new(FxHashMap::with_hasher(rustc_hash::FxBuildHasher)) };
    static PLACING: RefCell<FxHashMap<u64, egui::Rect>> = const { RefCell::new(FxHashMap::with_hasher(rustc_hash::FxBuildHasher)) };
}

/// The rect a widget was last drawn at, or `None` before it has drawn.
pub(crate) fn drawn_at(entity: Entity) -> Option<egui::Rect> {
    PLACED.with(|m| m.borrow().get(&entity.to_bits().get()).copied())
}

pub(crate) fn record_rect(entity: Entity, rect: egui::Rect) {
    PLACING.with(|m| {
        m.borrow_mut().insert(entity.to_bits().get(), rect);
    });
}

/// What a widget drew last frame. Only a `draw` node needs it: everything
/// else the layer draws it can also measure ahead, and a rect a script fills
/// is the one thing that can only be remembered.
pub(crate) fn measured_of(entity: Entity) -> egui::Vec2 {
    MEASURED.with(|m| {
        m.borrow()
            .get(&entity.to_bits().get())
            .copied()
            .unwrap_or(egui::Vec2::ZERO)
    })
}

pub(crate) fn record_measure(entity: Entity, size: egui::Vec2) {
    MEASURING.with(|m| {
        m.borrow_mut().insert(entity.to_bits().get(), size);
    });
    // A `draw` node's size comes from the script that filled it, not from any
    // property, so this is the one layout input no component write announces.
    if measured_of(entity) != size {
        crate::widget_arena::widget_changed(entity);
    }
}

/// Last frame's measurements become this frame's; a widget that stopped
/// drawing drops out rather than accumulating.
pub(crate) fn roll_measurements() {
    // Swapped rather than copied: the map holds an entry a widget, and
    // copying it was an O(n) walk on top of the one that filled it.
    MEASURING.with(|next| {
        MEASURED.with(|now| {
            std::mem::swap(&mut *now.borrow_mut(), &mut *next.borrow_mut());
        });
        next.borrow_mut().clear();
    });
}

/// Publish this frame's rects, at the end of the draw rather than the start
/// of the next one: a script reading them back is a frame behind either way,
/// and this is the smaller frame.
pub(crate) fn settle_rects() {
    PLACING.with(|next| {
        PLACED.with(|now| {
            std::mem::swap(&mut *now.borrow_mut(), &mut *next.borrow_mut());
        });
        next.borrow_mut().clear();
    });
}

/// The space inside a container's edge, in device pixels.
///
/// One rule, wherever a container is measured or drawn: the widget's own
/// `padding` where it states one, else the theme's entry for its kind, else
/// the built-in — 8 for a panel, which is the frame it has always drawn, and
/// nothing for a box that only lays out.
pub(crate) fn padding_of(widget: &Widget, style: &crate::widget_theme::Style, scale: f32) -> f32 {
    let built_in = if widget.kind == w::PANEL { 8.0 } else { 0.0 };
    let stated = if widget.padding > 0.0 {
        widget.padding
    } else {
        style.padding.unwrap_or(built_in)
    };
    stated * scale
}

/// The frame a container paints from its theme entry. `fill` is what a kind
/// shows when the theme says nothing — a panel has always had one, and a box
/// that only clips should stay invisible until asked.
/// The frame carries the look and no margin: `egui::Margin` is whole device
/// pixels, and a caller shrinks its own rect by the float padding instead.
fn themed_frame(
    style: &crate::widget_theme::Style,
    scale: f32,
    fill: Option<Color32>,
) -> egui::Frame {
    egui::Frame::new()
        .fill(style.fill.or(fill).unwrap_or(Color32::TRANSPARENT))
        .corner_radius(egui::CornerRadius::same(
            (style.radius.unwrap_or(0.0) * scale) as u8,
        ))
        .stroke(
            style
                .stroke
                .map_or(Stroke::NONE, |c| Stroke::new(style.stroke_px(), c)),
        )
}

/// A scroll container: the box is the parent's to decide and the children are
/// free to run past it, which is what makes a list in a sized panel scroll
/// rather than stretch the panel.
pub(crate) fn scroller(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let (entity, widget) = (placed.entity, placed.widget.clone());
    let box_size = box_of(&widget, at.assigned, at.scale);
    let room = ui.max_rect();
    let size = vec2(
        if box_size.x > 0.0 {
            box_size.x
        } else {
            room.width()
        },
        if box_size.y > 0.0 {
            box_size.y
        } else {
            room.height()
        },
    );
    let style = at.style_of(&widget);
    let pad = padding_of(&widget, &style, at.scale);
    let frame = themed_frame(&style, at.scale, None);
    let inner = (size - egui::Vec2::splat(pad * 2.0)).max(egui::Vec2::ZERO);
    frame.show(ui, |frame_ui| {
        // The padding comes off the box in floats; the frame itself carries
        // none, so a scroll at a fractional scale keeps the size it was given.
        let held = frame_ui.max_rect();
        let mut inner_ui = frame_ui.new_child(egui::UiBuilder::new().max_rect(held.shrink(pad)));
        let ui = &mut inner_ui;
        hold_to(ui, inner);
        let dead = widget.deadzone * at.scale;
        let mut area = egui::ScrollArea::both()
            .id_salt(("balaur-scroll", entity))
            .max_width(inner.x)
            .max_height(inner.y);
        // With a deadzone the finger scrolls nothing until it has travelled
        // that far, so a tap on a child lands; past it, this drags the
        // offset itself.
        let dragged = (dead > 0.0)
            .then(|| crate::widget_kinds::deadzone_drag(ui, at.eng, entity, dead))
            .flatten();
        if dead > 0.0 {
            area = area.scroll_source(egui::scroll_area::ScrollSource {
                drag: egui::scroll_area::DragScroll::Never,
                ..egui::scroll_area::ScrollSource::default()
            });
        }
        if let Some(offset) = dragged {
            area = area.scroll_offset(offset);
        }
        area.show(ui, |ui| {
            // Solved on its own, with the scroll's axis free: the contents
            // take what they measure and the bar makes up the difference.
            let room = crate::widget_taffy::Room::scrolling(egui::Rect::from_min_size(
                ui.max_rect().min,
                vec2(inner.x, inner.y),
            ));
            let solved = crate::widget_taffy::solve_subtree(
                at.eng,
                at.arena,
                index,
                ui,
                at.scale,
                &at.theme,
                &room,
                at.deep(index),
            );
            let held = std::mem::replace(&mut at.rects, solved);
            lay_out(ui, at, index, Axis::Column);
            at.rects = held;
        });
        // The frame, and the area above it, learn the box the child took;
        // a child ui reports nothing to its parent on its own.
        let used = inner_ui.min_rect().expand(pad);
        frame_ui.allocate_rect(used, egui::Sense::hover());
    });
}

/// A tab container: a strip of its children's names, then the one showing.
///
/// The label is a page's `text` where it has one and its node name otherwise,
/// so a page that is a bare panel still gets a name on the strip.
pub(crate) fn tabs(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let children = placed.children.clone();
    if children.is_empty() {
        return;
    }
    let scale = at.scale;
    let widget = placed.widget.clone();
    let entity = placed.entity;
    // Each page as (index, the name `active` holds, the strip's label). Two
    // pages showing the same text are told apart by their node names.
    let pages: Vec<(usize, SmolStr, SmolStr)> = children
        .iter()
        .map(|child| {
            let page = &at.arena[*child];
            let label = if page.widget.text.is_empty() {
                page.name.clone()
            } else {
                page.widget.text.clone()
            };
            let name = if page.name.is_empty() {
                label.clone()
            } else {
                page.name.clone()
            };
            (*child, name, label)
        })
        .collect();
    let showing = pages
        .iter()
        .position(|(_, name, _)| *name == widget.active)
        // A page's text is the older spelling of `active`, kept so a scene
        // written before the schema said "by node name" still shows it.
        .or_else(|| {
            pages
                .iter()
                .position(|(_, _, label)| *label == widget.active)
        })
        .unwrap_or(0);

    // The rect the layout pass gave this tab, which is the whole of it: the
    // strip takes the top and the page takes what is left.
    let rect = ui.max_rect();
    let style = at.style_of(&widget);
    // The face the theme resolves, not the raw properties: a widget that
    // states no size or colour is asking the theme for them.
    let (color, font) = crate::widget_layer::face(&at.theme, &style, &widget, scale);
    let gap = widget.gap * scale;

    let mut strip = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let chosen = strip
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(gap.max(4.0), 0.0);
            let mut chosen = None;
            for (slot, (_, name, label)) in pages.iter().enumerate() {
                let on = slot == showing;
                let mut button = egui::Button::new(
                    egui::RichText::new(label.as_str())
                        .font(font.clone())
                        .color(color),
                )
                .corner_radius(egui::CornerRadius::same(
                    (style.radius.unwrap_or(5.0) * scale) as u8,
                ));
                button = match (on, style.fill) {
                    (true, Some(fill)) => button.fill(fill),
                    (true, None) => button.fill(Color32::from_black_alpha(96)),
                    (false, _) => button.fill(Color32::TRANSPARENT),
                };
                if ui.add(button).clicked() {
                    chosen = Some(name.clone());
                }
            }
            chosen
        })
        .inner;
    if let Some(name) = chosen {
        at.edits.push((entity, Edit::Active(name.to_string())));
    }
    let strip_h = strip.min_rect().height();

    let page = egui::Rect::from_min_size(
        pos2(rect.min.x, rect.min.y + strip_h + gap),
        vec2(rect.width(), (rect.height() - strip_h - gap).max(0.0)),
    );
    // The page is solved on its own: only one of them is on screen, so the
    // strip's siblings never take part in the same flex line.
    let showing = pages[showing].0;
    let room = crate::widget_taffy::Room::fixed(page);
    let solved = crate::widget_taffy::solve_subtree(
        at.eng,
        at.arena,
        showing,
        ui,
        at.scale,
        &at.theme,
        &room,
        at.deep(index),
    );
    let restore = at.assigned;
    at.assigned = page.size();
    let held = std::mem::replace(&mut at.rects, solved);
    let mut body = ui.new_child(egui::UiBuilder::new().max_rect(page));
    body.set_clip_rect(page.intersect(ui.clip_rect()));
    draw_one(&mut body, at, showing);
    at.rects = held;
    at.assigned = restore;
    ui.advance_cursor_after_rect(rect);
}

/// The box a widget occupies: what it states, else what its parent gave it.
///
/// Godot's container contract — a child fills the rect it was assigned unless
/// it names a size of its own. 0 on an axis means "hug", which is what a root
/// and every scene written before `grow` gets.
pub(crate) fn box_of(widget: &Widget, assigned: egui::Vec2, scale: f32) -> egui::Vec2 {
    let stated = vec2(widget.width, widget.height) * scale;
    let floor = vec2(widget.min_width, widget.min_height) * scale;
    vec2(
        if stated.x > 0.0 { stated.x } else { assigned.x }.max(floor.x),
        if stated.y > 0.0 { stated.y } else { assigned.y }.max(floor.y),
    )
}

/// Hold a ui to a box on the axes the box names, so `available_size` inside it
/// is the room the children actually have to divide.
pub(crate) fn hold_to(ui: &mut egui::Ui, size: egui::Vec2) {
    if size.x > 0.0 {
        ui.set_max_width(size.x);
        ui.set_min_width(size.x);
    }
    if size.y > 0.0 {
        ui.set_max_height(size.y);
        ui.set_min_height(size.y);
    }
}

/// Which way a container stacks what is inside it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    Row,
    Column,
}

impl Axis {
    /// The component along the container's own direction.
    fn along(self, v: egui::Vec2) -> f32 {
        match self {
            Axis::Row => v.x,
            Axis::Column => v.y,
        }
    }
}

/// A box's own stated size, in device pixels: what a seam drag writes back.
pub(crate) fn stated_of(widget: &Widget, scale: f32) -> egui::Vec2 {
    vec2(widget.width, widget.height) * scale
}

/// A bare container: its own frame where the theme gives it one, then the
/// children at the rects the layout pass decided.
///
/// A `row` or a `column` paints nothing unless asked, which is what keeps a
/// box that only lays out invisible; a `fill` or a `stroke` makes it a tile,
/// and that is how a pair of buttons reads as one control.
pub(crate) fn contain(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize, axis: Axis) {
    let widget = at.arena[index].widget.clone();
    let style = at.style_of(&widget);
    if style.fill.is_some() || style.stroke.is_some() {
        let radius = egui::CornerRadius::same((style.radius.unwrap_or(0.0) * at.scale) as u8);
        ui.painter().rect(
            ui.max_rect(),
            radius,
            style.fill.unwrap_or(Color32::TRANSPARENT),
            style
                .stroke
                .map_or(Stroke::NONE, |c| Stroke::new(style.stroke_px(), c)),
            egui::StrokeKind::Inside,
        );
    }
    lay_out(ui, at, index, axis);
}

/// The children themselves, each drawn into the rect the layout pass gave it.
///
/// Nothing is divided here any more: `widget_taffy` solved the whole subtree
/// before the first pixel, so this walks the answers and pins a `Ui` to each.
pub(crate) fn lay_out(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize, axis: Axis) {
    let placed = &at.arena[index];
    if placed.children.is_empty() {
        return;
    }
    let grab = placed.widget.handle * at.scale;
    let cross = match placed.widget.align.as_str() {
        w::CENTER => egui::Align::Center,
        w::END => egui::Align::Max,
        _ => egui::Align::Min,
    };
    let layout = match axis {
        Axis::Row => egui::Layout::left_to_right(cross),
        Axis::Column => egui::Layout::top_down(cross),
    };
    let children = placed.children.clone();
    for (slot, child) in children.iter().enumerate() {
        let entity = at.arena[*child].entity;
        let Some(rect) = at.rects.get(child).copied() else {
            continue;
        };
        record_rect(entity, rect);
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            continue;
        }
        // Off the clip nothing is seen or reached, so the subtree is skipped
        // and its measurement carries over. Only once it has one, though, or
        // a bootstrap frame settles the layout at zero.
        let measured = measured_of(entity);
        if measured != egui::Vec2::ZERO && !ui.clip_rect().intersects(rect) {
            record_measure(entity, measured);
            ui.advance_cursor_after_rect(rect);
            continue;
        }
        let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
        // The box the solve gave this child, so a kind that sizes itself from
        // what it was handed — a slider's track, a dropdown's width — reads
        // the same number the layout decided.
        let restore = at.assigned;
        at.assigned = rect.size();
        draw_one(&mut child_ui, at, *child);
        at.assigned = restore;
        // A `draw` node records what its script painted, from inside the
        // draw; everything else is measured ahead and needs no record.
        if at.arena[*child].widget.kind != w::DRAW {
            record_measure(entity, child_ui.min_rect().size());
        }
        ui.advance_cursor_after_rect(rect);
        if grab > 0.0 && slot + 1 < children.len() {
            // Centred on the seam between this child and the next, so a grab
            // wider than the gap reaches into both rather than only one.
            let next = at.rects.get(&children[slot + 1]).copied().unwrap_or(rect);
            let seam = match axis {
                Axis::Row => egui::Rect::from_min_size(
                    pos2((rect.max.x + next.min.x - grab) / 2.0, rect.min.y),
                    vec2(grab, rect.height()),
                ),
                Axis::Column => egui::Rect::from_min_size(
                    pos2(rect.min.x, (rect.max.y + next.min.y - grab) / 2.0),
                    vec2(rect.width(), grab),
                ),
            };
            drag_seam(ui, at, &children, slot, axis, seam);
        }
    }
}

/// Where a child starts, given how far along the axis the container has got.
/// states a size, so the other keeps growing into what is left.
fn drag_seam(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    children: &[usize],
    slot: usize,
    axis: Axis,
    seam: egui::Rect,
) {
    let before = children[slot];
    let after = children[slot + 1];
    // The one that states a size takes the drag; between two growers there is
    // nothing to write, so the seam is not a handle at all.
    let (target, sign) = if axis.along(stated_of(&at.arena[before].widget, at.scale)) > 0.0 {
        (before, 1.0)
    } else if axis.along(stated_of(&at.arena[after].widget, at.scale)) > 0.0 {
        (after, -1.0)
    } else {
        return;
    };
    let handle = ui.interact(
        seam,
        egui::Id::new(("balaur-seam", at.arena[target].entity, slot)),
        egui::Sense::drag(),
    );
    if handle.hovered() || handle.dragged() {
        ui.output_mut(|out| {
            out.cursor_icon = match axis {
                Axis::Row => egui::CursorIcon::ResizeHorizontal,
                Axis::Column => egui::CursorIcon::ResizeVertical,
            };
        });
    }
    if !handle.dragged() {
        return;
    }
    let moved = axis.along(handle.drag_delta()) * sign;
    if moved == 0.0 {
        return;
    }
    let widget = &at.arena[target].widget;
    let floor = axis.along(vec2(widget.min_width, widget.min_height));
    let was = axis.along(vec2(widget.width, widget.height));
    let now = (was + moved / at.scale).max(floor.max(1.0));
    let entity = at.arena[target].entity;
    at.edits.push((
        entity,
        match axis {
            Axis::Row => Edit::Width(now),
            Axis::Column => Edit::Height(now),
        },
    ));
}

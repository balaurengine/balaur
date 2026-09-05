//! How a container decides where its children go: the sizing rules, the
//! containers that apply them, and the measurement a hugging child needs.
//!
//! Split from `widget_layer` because that file is the component and the walk
//! over the world, and this is the arithmetic between them.

use crate::theme::family;
use crate::widget_layer::{draw_one, rgba_color, Edit, Painting, Widget};
use crate::widget_measure::Measure;
use balaur_core::hecs::Entity;
use egui::{pos2, vec2, Color32, Stroke};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// What each widget drew last frame. Only a `draw` node needs it now —
    /// everything else the layer draws it can also measure, and a rect a
    /// script fills is the one thing it can only remember.
    static MEASURED: RefCell<HashMap<u64, egui::Vec2>> = RefCell::new(HashMap::new());
    static MEASURING: RefCell<HashMap<u64, egui::Vec2>> = RefCell::new(HashMap::new());
    /// Where each widget was drawn, for a script that has to place something
    /// against it — the editor's own chrome reads its shell back this way.
    static PLACED: RefCell<HashMap<u64, egui::Rect>> = RefCell::new(HashMap::new());
    static PLACING: RefCell<HashMap<u64, egui::Rect>> = RefCell::new(HashMap::new());
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

fn measured_of(entity: Entity) -> egui::Vec2 {
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
}

/// Last frame's measurements become this frame's; a widget that stopped
/// drawing drops out rather than accumulating.
pub(crate) fn roll_measurements() {
    MEASURING.with(|next| {
        MEASURED.with(|now| {
            now.borrow_mut().clone_from(&next.borrow());
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
            now.borrow_mut().clone_from(&next.borrow());
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
    let built_in = if widget.kind == "panel" { 8.0 } else { 0.0 };
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
                .map_or(Stroke::NONE, |c| Stroke::new(style.stroke_width, c)),
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
    let style = at.theme.style(&widget.kind);
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
            // Along the scroll the room is unbounded: children take what
            // they measure and the bar makes up the difference.
            let held = std::mem::replace(&mut at.bounds, vec2(inner.x, 0.0));
            lay_out(ui, at, index, Axis::Column);
            at.bounds = held;
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
    let pages: Vec<(usize, String, String)> = children
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

    let box_size = box_of(&widget, at.assigned, scale);
    let room = ui.max_rect();
    let rect = egui::Rect::from_min_size(
        room.min,
        vec2(
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
        ),
    );
    let style = at.theme.style(&widget.kind);
    let font = egui::FontId::new(widget.font_size * scale, family("ui"));
    let color = rgba_color(widget.text_color);
    let gap = widget.gap * scale;

    let mut strip = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let chosen = strip
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(gap.max(4.0), 0.0);
            let mut chosen = None;
            for (slot, (_, name, label)) in pages.iter().enumerate() {
                let on = slot == showing;
                let mut button =
                    egui::Button::new(egui::RichText::new(label).font(font.clone()).color(color))
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
        at.edits.push((entity, Edit::Active(name)));
    }
    let strip_h = strip.min_rect().height();

    let page = egui::Rect::from_min_size(
        pos2(rect.min.x, rect.min.y + strip_h + gap),
        vec2(rect.width(), (rect.height() - strip_h - gap).max(0.0)),
    );
    let restore = at.assigned;
    at.assigned = page.size();
    let mut body = ui.new_child(egui::UiBuilder::new().max_rect(page));
    body.set_clip_rect(page.intersect(ui.clip_rect()));
    draw_one(&mut body, at, pages[showing].0);
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

    fn across(self, v: egui::Vec2) -> f32 {
        match self {
            Axis::Row => v.y,
            Axis::Column => v.x,
        }
    }

    /// A vector from its two components, along first.
    fn vec(self, along: f32, across: f32) -> egui::Vec2 {
        match self {
            Axis::Row => vec2(along, across),
            Axis::Column => vec2(across, along),
        }
    }
}

/// The size a widget states, in device pixels; 0 on an axis it leaves free.
fn stated_of(widget: &Widget, scale: f32) -> egui::Vec2 {
    vec2(widget.width, widget.height) * scale
}

/// What a child asks for along the axis, and the floor it may not go under.
fn asked_of(widget: &Widget, axis: Axis, scale: f32) -> (f32, f32) {
    let stated = axis.along(vec2(widget.width, widget.height)) * scale;
    let floor = axis.along(vec2(widget.min_width, widget.min_height)) * scale;
    (stated, floor)
}

/// A bare container: padding, then the children along `axis`.
pub(crate) fn contain(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize, axis: Axis) {
    let widget = &at.arena[index].widget;
    let scale = at.scale;
    // The padding comes off in floats rather than through a `Margin`, which
    // is whole device pixels: a 14 px gutter at 1.25 scale is not one, and
    // the truncation moved every sheet in the editor's shell by 0.4 px.
    let pad = padding_of(widget, &at.theme.style(&widget.kind), scale);
    let box_size = box_of(widget, at.assigned, scale);
    let room = ui.max_rect();
    let outer = egui::Rect::from_min_size(
        room.min,
        vec2(
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
        ),
    )
    .shrink(pad);
    let min = (box_size - egui::Vec2::splat(pad * 2.0)).max(egui::Vec2::ZERO);
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(outer));
    hold_to(&mut inner, min);
    let held = std::mem::replace(&mut at.bounds, min);
    lay_out(&mut inner, at, index, axis);
    at.bounds = held;
    // What the children took, with the padding back on: a container that
    // states no size is still as big as what is inside it.
    ui.advance_cursor_after_rect(inner.min_rect().expand(pad));
}

/// What one child asks for along the container's axis.
#[derive(Clone, Copy)]
enum Ask {
    /// This many pixels, 0 included: a box with nothing in it is not a box
    /// that wants everything.
    Fixed(f32),
    /// A share of the leftover, once the fixed ones are in.
    Grows,
    /// Whatever is left here. Only for what cannot be measured ahead — a
    /// script's rect, a scroll's contents — and only until it has drawn once.
    Rest,
}

/// What each child asks for, the total the fixed ones spend (gaps included)
/// and the sum of the `grow` shares waiting on the leftover.
fn share_out(
    at: &Painting<'_>,
    ui: &egui::Ui,
    children: &[usize],
    axis: Axis,
    gap: f32,
) -> (Vec<Ask>, f32, f32) {
    let scale = at.scale;
    let mut asked = Vec::with_capacity(children.len());
    let mut spent = 0.0f32;
    let mut shares = 0.0f32;
    let mut seams = 0.0f32;
    let mut measure = Measure::new(at.eng, at.arena, ui, scale);
    for child in children {
        let widget = &at.arena[*child].widget;
        let (stated, floor) = asked_of(widget, axis, scale);
        let ask = if widget.grow > 0.0 {
            shares += widget.grow;
            spent += floor;
            Ask::Grows
        } else if stated > 0.0 {
            let size = stated.max(floor);
            spent += size;
            Ask::Fixed(size)
        } else {
            // What it will need, asked of the fonts rather than remembered
            // from last frame. What the measure cannot answer for falls back
            // to what it drew, and to the leftover before even that.
            let wanted = axis.along(measure.of(*child, &at.theme)).max(floor);
            let known = if wanted > 0.0 || can_measure(&widget.kind) {
                wanted
            } else {
                axis.along(measured_of(at.arena[*child].entity)).max(floor)
            };
            if known <= 0.0 && !can_measure(&widget.kind) {
                Ask::Rest
            } else {
                spent += known;
                Ask::Fixed(known)
            }
        };
        // A box with no size takes no seam either: a hidden rail must not
        // leave a gap where it would have been.
        if !matches!(ask, Ask::Fixed(size) if size <= 0.0) {
            seams += gap;
        }
        asked.push(ask);
    }
    (asked, spent + (seams - gap).max(0.0), shares)
}

/// Whether the measure pass can answer for a kind, or only the last frame can.
fn can_measure(kind: &str) -> bool {
    !matches!(kind, "draw" | "scroll")
}

/// The children themselves: each given a rect along the container's axis,
/// with the leftover divided between those that grow.
///
/// The container places every child; a child that overflows the rect it was
/// given does not move its siblings, which is what kept a 2 px frame stroke
/// compounding down a column. A child that states neither a size nor a `grow`
/// takes what it needs and is measured, which is what keeps every scene
/// written before `grow` existed laying out the way it did.
pub(crate) fn lay_out(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize, axis: Axis) {
    let placed = &at.arena[index];
    if placed.children.is_empty() {
        return;
    }
    let scale = at.scale;
    let gap = placed.widget.gap * scale;
    let grab = placed.widget.handle * scale;
    let cross = match placed.widget.align.as_str() {
        "center" => egui::Align::Center,
        "end" => egui::Align::Max,
        _ => egui::Align::Min,
    };
    let layout = match axis {
        Axis::Row => egui::Layout::left_to_right(cross),
        Axis::Column => egui::Layout::top_down(cross),
    };
    // Copied out: the closure needs `at` mutably, and `placed` borrows it.
    let children = placed.children.clone();
    let (asked, spent, shares) = share_out(at, ui, &children, axis, gap);
    // What is left here, not the whole box: a panel that drew a caption first
    // has that much less to hand out, and dividing the box instead pushed its
    // children past their own frame.
    let outer = ui.available_rect_before_wrap();
    // A container free to grow has no leftover to divide, so `grow` there is
    // the floor and nothing more.
    let bounded = at.bounds;
    let free = if axis.along(bounded) > 0.0 {
        (axis.along(outer.size()) - spent).max(0.0)
    } else {
        0.0
    };
    let mut head = axis.along(outer.min.to_vec2());
    let far = head + axis.along(outer.size());
    let mut laid = 0usize;
    for (slot, child) in children.iter().enumerate() {
        let entity = at.arena[*child].entity;
        let (size, hug) = match asked[slot] {
            Ask::Fixed(size) => (size, false),
            Ask::Grows => {
                let widget = &at.arena[*child].widget;
                let (_, floor) = asked_of(widget, axis, scale);
                (floor + free * (widget.grow / shares), false)
            }
            Ask::Rest => ((far - head).max(0.0), true),
        };
        let breadth = axis.across(outer.size());
        if size <= 0.0 && !hug {
            // Nothing to place, and no seam either. The rect is still
            // recorded, flat along the axis, so a script asking where it went
            // gets an empty box in the right place rather than nothing.
            record_rect(
                entity,
                egui::Rect::from_min_size(rect_head(outer, head, axis), axis.vec(0.0, breadth)),
            );
            continue;
        }
        if laid > 0 {
            head += gap;
        }
        laid += 1;
        let extent = size;
        // Filling the cross is the container's default, and it is what `align`
        // opts out of: a centred child that filled its box would have nothing
        // left to centre in. A container free to grow fills nothing.
        let fills = cross == egui::Align::Min && axis.across(bounded) > 0.0;
        let rect =
            egui::Rect::from_min_size(rect_head(outer, head, axis), axis.vec(extent, breadth));
        let restore = at.assigned;
        at.assigned = axis.vec(
            if hug { 0.0 } else { extent },
            if fills { breadth } else { 0.0 },
        );
        let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(layout));
        draw_one(&mut child_ui, at, *child);
        let used = child_ui.min_rect().size();
        at.assigned = restore;
        record_measure(entity, used);
        record_rect(entity, rect);
        let taken = if hug { axis.along(used) } else { extent };
        ui.allocate_rect(
            egui::Rect::from_min_size(rect.min, axis.vec(taken, axis.across(used))),
            egui::Sense::hover(),
        );
        head += taken;
        if grab > 0.0 && slot + 1 < children.len() {
            // Centred on the seam, so a grab wider than the gap reaches into
            // both children rather than only the one after it.
            let middle = head + gap / 2.0;
            let seam = egui::Rect::from_min_size(
                match axis {
                    Axis::Row => pos2(middle - grab / 2.0, rect.min.y),
                    Axis::Column => pos2(rect.min.x, middle - grab / 2.0),
                },
                axis.vec(grab, breadth),
            );
            drag_seam(ui, at, &children, slot, axis, seam);
        }
    }
}

/// Where a child starts, given how far along the axis the container has got.
fn rect_head(outer: egui::Rect, head: f32, axis: Axis) -> egui::Pos2 {
    match axis {
        Axis::Row => pos2(head, outer.min.y),
        Axis::Column => pos2(outer.min.x, head),
    }
}

/// The grab between two children: dragging it resizes whichever of the pair
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

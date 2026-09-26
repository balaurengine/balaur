//! The widget kinds past the first nine: the controls a settings screen is
//! made of, the containers a shop is laid out with, and the nine-patch
//! that dresses both. Each is egui's own widget where egui has one, drawn
//! from the scene's values and reporting back through the frame's edits.
//! The row kinds, `list`, `tree` and `table`, are [`super::rows`].

use balaur_core::Engine;
use egui::{Color32, Rect, Sense, Stroke, TextureId, pos2, vec2};

use crate::vocabulary::words as w;
use crate::widget::arrange::{Axis, lay_out, padding_of, record_measure, record_rect, solved_of};
use crate::widget::layer::{Edit, Painting, draw_one};
use crate::widget::measure::Measure;
use crate::widget::node::Widget;
use crate::widget::theme::{Pointer, WidgetState};

/// A ticked box with a caption. The tick lives on the widget: the click is
/// reported like a button's and the next tick flips `checked`.
pub(crate) fn check(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
    color: Color32,
) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let mut on = widget.checked;
    let label = egui::RichText::new(caption).font(font.clone()).color(color);
    let response = ui.add(egui::Checkbox::new(&mut on, label));
    if response.clicked() {
        at.clicked.push(placed.entity);
    }
    if at.focused == Some(placed.entity) {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            4.0,
            Stroke::new(2.0, color),
            egui::StrokeKind::Outside,
        );
    }
}

/// An on/off switch: a track the theme fills and a knob at one end of it.
///
/// `checked` is what it holds: `[roles.x]` dresses it off and
/// `[roles.x.checked]` on, the held look standing in where a theme has none.
pub(crate) fn switch(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let (entity, on) = {
        let placed = &at.arena[index];
        (placed.entity, placed.widget.checked)
    };
    let look = at.resting(index);
    let height = look.style.height.unwrap_or(18.0);
    let (rect, response) = ui.allocate_exact_size(vec2(height * 1.75, height), Sense::click());
    if response.clicked() {
        at.clicked.push(entity);
    }
    let dressed = look.style.in_states(WidgetState {
        pointer: Pointer::of(&response),
        ..at.state
    });
    let track = dressed.fill.unwrap_or(Color32::TRANSPARENT);
    let knob = dressed
        .text_color
        .unwrap_or(crate::widget::theme::DEFAULT_INK);
    ui.painter().rect_filled(rect, height / 2.0, track);
    let inset = height / 2.0;
    let x = if on {
        rect.max.x - inset
    } else {
        rect.min.x + inset
    };
    ui.painter()
        .circle_filled(pos2(x, rect.center().y), height * 0.34, knob);
}

/// One of the widget's `options`, chosen from a list that drops down.
pub(crate) fn dropdown(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    font: &egui::FontId,
    color: Color32,
) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let entity = placed.entity;
    let want = solved_of(widget, &at.style_of(widget), at.assigned);
    let mut chosen = widget.text.clone();
    let mut combo = egui::ComboBox::from_id_salt(("balaur-dropdown", entity)).selected_text(
        egui::RichText::new(chosen.as_str())
            .font(font.clone())
            .color(color),
    );
    if want.x > 0.0 {
        combo = combo.width(want.x);
    }
    let up = combo
        .show_ui(ui, |ui| {
            for option in &widget.options {
                let label = egui::RichText::new(option.as_str())
                    .font(font.clone())
                    .color(color);
                ui.selectable_value(&mut chosen, option.clone(), label);
            }
        })
        .inner
        .is_some();
    if up {
        at.shown.push(entity);
    }
    if chosen != widget.text {
        at.edits.push((entity, Edit::Choice(chosen.to_string())));
    }
}

/// A number dragged between `min` and `max`. egui draws it; the value it
/// reports lands on the widget next tick, which is also when `on_change`
/// hears it.
pub(crate) fn slider(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let (low, high) = (widget.min, widget.max.max(widget.min));
    let mut value = widget.value.clamp(low, high);
    let want = solved_of(widget, &at.style_of(widget), at.assigned);
    let width = if want.x > 0.0 {
        want.x
    } else {
        ui.available_width().min(160.0)
    };
    ui.spacing_mut().slider_width = width;
    let mut slider = egui::Slider::new(&mut value, low..=high).show_value(false);
    if widget.step > 0.0 {
        slider = slider.step_by(f64::from(widget.step));
    }
    let response = ui.add(slider);
    if response.changed() {
        at.edits.push((placed.entity, Edit::Value(value)));
    }
    // A drag lets go, or a key moved it with no drag at all.
    if response.drag_stopped() || (response.changed() && !response.dragged()) {
        at.edits.push((placed.entity, Edit::Committed(value)));
    }
}

/// A file being edited, with the gutter and the colouring `ui::code_editor`
/// draws: Godot's `CodeEdit` as a node.
///
/// The kind is the same call a script makes, given the widget's own values
/// instead of an options table, so there is one editor and one highlighter.
pub(crate) fn code(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let (entity, widget) = (placed.entity, placed.widget.clone());
    let want = solved_of(&widget, &at.style_of(&widget), at.assigned);
    let id = format!("balaur-code-{}", entity.to_bits());
    let opts = crate::immediate::code::code_opts(&widget, &at.theme);
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
        ui.max_rect().min,
        egui::vec2(
            if want.x > 0.0 {
                want.x
            } else {
                ui.available_width()
            },
            if want.y > 0.0 {
                want.y
            } else {
                ui.available_height()
            },
        ),
    )));
    crate::bridge::push(&mut inner);
    let edited = crate::immediate::code::code_editor(at.eng, &id, &widget.text, &opts);
    crate::bridge::pop();
    match edited {
        Ok((text, changed, hit, _)) => {
            if changed {
                at.edits.push((entity, Edit::Text(text)));
            }
            // The line alone: what a click on the gutter means is the script's
            // to decide, as a breakpoint in one editor and a value in another.
            if let Some(line) = hit {
                at.edits.push((entity, Edit::Gutter(line)));
            }
        }
        Err(err) => warn_code(&err),
    }
    let used = inner.min_rect().size();
    record_measure(entity, used);
    ui.advance_cursor_after_rect(egui::Rect::from_min_size(inner.max_rect().min, used));
}

fn warn_code(err: &anyhow::Error) {
    tracing::warn!("code widget: {err:#}");
}

/// How much of a `number_field`'s box its two arrows take, gap included.
const ARROWS: f32 = 18.0;

/// A button that drops a list of items: Godot's `MenuButton`, and the same
/// list a `PopupMenu` shows. `options` are the entries and `text` is the
/// button; picking one reports it the way a dropdown reports a choice, so a
/// script hears it through `on_change`.
pub(crate) fn menu(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
    color: Color32,
) {
    let placed = &at.arena[index];
    let entity = placed.entity;
    let showing = placed.widget.showing;
    let placement = placed.widget.placement.clone();
    // A menu drawn inside an open menu is a submenu: egui opens it to the
    // side on hover, keeps one of them open at a time, and shuts it with the
    // menu it hangs off.
    let inner = egui::containers::menu::is_in_menu(ui);
    // A menu whose rows are nodes: its button is dressed by the theme like any
    // other, and the rows carry the icons, shortcuts and ticks a flat list of
    // strings cannot.
    if !placed.children.is_empty() {
        let response = crate::widget::button::button(ui, at, index, caption, font, color);
        let up = if inner {
            egui::containers::menu::SubMenu::new()
                .show(ui, &response, |ui| popup_rows(ui, at, index))
                .is_some()
        } else {
            dropped(&response, &placement, ui, showing)
                .show(|ui| popup_rows(ui, at, index))
                .is_some()
        };
        if up {
            at.shown.push(entity);
        }
        return;
    }
    let options = placed.widget.options.clone();
    let mut picked = None;
    let label = egui::RichText::new(caption).font(font.clone()).color(color);
    // What `Ui::menu_button` does, with the placement this node asked for:
    // the same button and the same menu popup, which is all that call is.
    let response = ui.button(label);
    let rows = |ui: &mut egui::Ui| {
        for option in &options {
            let item = egui::RichText::new(option.as_str())
                .font(font.clone())
                .color(color);
            if ui.button(item).clicked() {
                picked = Some(option.to_string());
                ui.close();
            }
        }
    };
    let up = if inner {
        egui::containers::menu::SubMenu::new()
            .show(ui, &response, rows)
            .is_some()
    } else {
        dropped(&response, &placement, ui, showing)
            .show(rows)
            .is_some()
    };
    if up {
        at.shown.push(entity);
    }
    if let Some(choice) = picked {
        at.edits.push((entity, Edit::Choice(choice)));
    }
}

/// A menu's popup, placed where the node says and held open where the scene
/// says. `below` is egui's own: under the button, flipped above it where
/// there is no room.
fn dropped<'a>(
    response: &egui::Response,
    placement: &str,
    ui: &egui::Ui,
    showing: bool,
) -> egui::Popup<'a> {
    // The rows decide for themselves: egui's default closes on any click
    // inside, which would shut the menu under a toggle that keeps it open.
    let mut popup =
        egui::Popup::menu(response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    popup = match placement {
        w::ABOVE => popup.align(egui::RectAlign::TOP_START),
        // Where the pointer was when the menu opened, not where it is now:
        // egui only remembers that for a popup told to open, so the click
        // says so itself rather than leaving it to the toggle.
        w::POINTER => {
            let id = egui::Popup::default_response_id(response);
            let open = egui::Popup::is_id_open(ui.ctx(), id);
            let set = response
                .clicked()
                .then_some(egui::SetOpenCommand::Bool(!open));
            popup.at_pointer_fixed().open_memory(set)
        }
        // Over the middle of the screen, and staying there: a menu the game
        // put in the centre is not one egui may nudge to fit.
        w::CENTER => popup
            .at_position(ui.ctx().viewport_rect().center())
            .align(egui::RectAlign::over_corner(egui::Align2::CENTER_CENTER))
            .align_alternatives(&[]),
        _ => popup,
    };
    if showing {
        popup = popup.open_memory(egui::SetOpenCommand::Bool(true));
    }
    popup
}

/// A menu's rows, drawn inside the popup egui opened for it.
///
/// The popup's rect is only known here, so the subtree is solved against it
/// the way `fold` solves what it opens, then drawn by the same walker every
/// container uses. A row's click is its own, so nothing new comes back.
pub(crate) fn popup_rows(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let room = ui.available_rect_before_wrap();
    let space = crate::widget::taffy::Room::hugging(room);
    let solved = crate::widget::taffy::solve_subtree(
        at.eng,
        at.arena,
        index,
        ui,
        &at.theme,
        &space,
        at.deep(index),
    );
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(room));
    let held = std::mem::replace(&mut at.rects, solved);
    let before = at.clicked.len();
    lay_out(&mut inner, at, index, Axis::Column);
    at.rects = held;
    ui.advance_cursor_after_rect(inner.min_rect());
    let arena = at.arena;
    // A row that opened a submenu chose nothing, so the menu it sits in
    // stays up; every other row closes it unless it says `keep_open`.
    let closes = at.clicked[before..].iter().any(|entity| {
        arena
            .iter()
            .find(|placed| placed.entity == *entity)
            .is_some_and(|placed| !placed.widget.keep_open && placed.widget.kind != w::MENU)
    });
    if closes {
        ui.close();
    }
}

/// A swatch that opens a picker: Godot's `ColorPickerButton`. The colour is
/// the widget's own `picked_color`, not the ink its caption is drawn in.
pub(crate) fn color(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let entity = placed.entity;
    // Unit floats are sRGB here as on every other widget property, so the
    // picker is egui's sRGB one: its linear `Rgba` showed them too light.
    let mut srgba = crate::widget::node::rgba_color(widget.color);
    let want = solved_of(widget, &at.style_of(widget), at.assigned);
    if want.x > 0.0 {
        ui.spacing_mut().interact_size.x = want.x;
    }
    let changed = egui::color_picker::color_edit_button_srgba(
        ui,
        &mut srgba,
        egui::color_picker::Alpha::OnlyBlend,
    )
    .changed();
    let unit = srgba.to_srgba_unmultiplied().map(|c| f32::from(c) / 255.0);
    if changed {
        at.edits.push((entity, Edit::Color(unit)));
    }
    // The picker's drag happens in its popup, so the release is watched for
    // here: a colour committed once the button that was changing it lets go.
    let dragging = egui::Id::new(("balaur-color-drag", entity));
    let (down, released) = ui.input(|i| (i.pointer.any_down(), i.pointer.any_released()));
    let was = ui.data(|d| d.get_temp::<bool>(dragging).unwrap_or(false));
    if changed && down {
        ui.data_mut(|d| d.insert_temp(dragging, true));
    } else if (was && released) || (changed && !down) {
        ui.data_mut(|d| d.insert_temp(dragging, false));
        at.edits.push((entity, Edit::ColorCommitted(unit)));
    }
}

/// A number dragged sideways, or typed into after a click. `SpinBox` in a
/// Godot scene; the control an inspector row is mostly made of.
///
/// `min` and `max` are the slider's, and so default to 0 and 1. A position or
/// a scale is neither, and most of what an inspector shows runs free, so that
/// default pair reads here as no bounds at all: any other pair binds.
pub(crate) fn drag_value(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    font: &egui::FontId,
    color: Color32,
) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let mut value = widget.value;
    let mut drag = egui::DragValue::new(&mut value);
    // The letter a vector row puts before each number, which is the one thing
    // a drag value shows that is not the number itself.
    if !widget.placeholder.is_empty() {
        drag = drag.prefix(format!("{} ", widget.placeholder));
    }
    if !widget.suffix.is_empty() {
        drag = drag.suffix(format!(" {}", widget.suffix));
    }
    let bounded = widget.max > widget.min && (widget.min, widget.max) != (0.0, 1.0);
    if bounded {
        drag = drag.range(widget.min..=widget.max);
    }
    if widget.step > 0.0 {
        drag = drag.speed(widget.step);
    }
    let want = solved_of(widget, &at.style_of(widget), at.assigned);
    if want.x > 0.0 {
        // With arrows, the number is told what is left of the stated box, so
        // the pair sits inside it rather than in the next widget's.
        ui.spacing_mut().interact_size.x = if widget.arrows {
            (want.x - ARROWS).max(24.0)
        } else {
            want.x
        };
    }
    let mut stepped = None;
    let look = at.look(index);
    let response = ui.scope(|ui| {
        ui.style_mut().override_font_id = Some(font.clone());
        crate::widget::theme::dress(ui, &look.style, color);
        if !widget.arrows {
            return ui.add(drag);
        }
        // The box split by hand: egui's layouts size a `DragValue` from the
        // room they have, so in a row the steps land in the next widget's.
        let full = ui.available_rect_before_wrap();
        let high = full.height().min(want.y.max(18.0));
        let wide = ARROWS - 4.0;
        let steps =
            egui::Rect::from_min_size(pos2(full.right() - wide, full.top()), vec2(wide, high));
        let number =
            egui::Rect::from_min_max(full.min, pos2(steps.left() - 4.0, full.top() + high));
        let inner = ui.put(number, drag);
        let each = (high - 1.0) / 2.0;
        let mark = |glyph: &str| egui::Button::new(egui::RichText::new(glyph).size(each - 2.0));
        let up = egui::Rect::from_min_size(steps.min, vec2(wide, each));
        let down = egui::Rect::from_min_size(
            pos2(steps.left(), steps.top() + each + 1.0),
            vec2(wide, each),
        );
        if ui.put(up, mark("⏶")).clicked() {
            stepped = Some(1.0);
        }
        if ui.put(down, mark("⏷")).clicked() {
            stepped = Some(-1.0);
        }
        ui.advance_cursor_after_rect(egui::Rect::from_min_size(
            full.min,
            vec2(full.width(), high),
        ));
        inner
    });
    if let Some(way) = stepped {
        let step = if widget.step > 0.0 { widget.step } else { 1.0 };
        let moved = value + way * step;
        value = if bounded {
            moved.clamp(widget.min, widget.max)
        } else {
            moved
        };
        at.edits.push((placed.entity, Edit::Value(value)));
        at.edits.push((placed.entity, Edit::Committed(value)));
    } else if response.inner.changed() {
        at.edits.push((placed.entity, Edit::Value(value)));
    }
    // A drag lets go, or the number typed in is left.
    if response.inner.drag_stopped() || response.inner.lost_focus() {
        at.edits.push((placed.entity, Edit::Committed(value)));
    }
}

/// A bar filled to `value`, with the caption over it.
pub(crate) fn progress(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
    color: Color32,
) {
    let widget = &at.arena[index].widget;
    let span = (widget.max - widget.min).abs().max(f32::EPSILON);
    let fraction = ((widget.value - widget.min) / span).clamp(0.0, 1.0);
    let want = solved_of(widget, &at.style_of(widget), at.assigned);
    let mut bar = egui::ProgressBar::new(fraction).desired_width(if want.x > 0.0 {
        want.x
    } else {
        ui.available_width().min(160.0)
    });
    if want.y > 0.0 {
        bar = bar.desired_height(want.y);
    }
    let style = at.style_of(widget);
    if let Some(fill) = style.fill {
        bar = bar.fill(fill);
    }
    if !caption.is_empty() {
        bar = bar.text(egui::RichText::new(caption).font(font.clone()).color(color));
    }
    ui.add(bar);
}

/// A line across the parent's direction, in the theme's stroke.
pub(crate) fn separator(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let widget = &at.arena[index].widget;
    let style = at.style_of(widget);
    if let Some(color) = style.stroke {
        ui.visuals_mut().widgets.noninteractive.bg_stroke = Stroke::new(style.stroke_px(), color);
    }
    ui.add(egui::Separator::default().spacing(6.0));
}

/// A header that shows or hides the children under it. The header is a
/// button by another shape: clicking it reports an `Open` edit, and focus
/// lands on it as on a button. A child marked `title_bar` is drawn in the
/// header after the caption, and takes its own clicks first.
pub(crate) fn fold(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
    color: Color32,
) {
    let placed = &at.arena[index];
    let (entity, open) = (placed.entity, placed.widget.open);
    let widget = placed.widget.clone();
    let bar: Vec<usize> = placed
        .children
        .iter()
        .copied()
        .filter(|child| in_title_bar(at.arena, index, *child))
        .collect();
    let style = at.style_of(&widget);
    let pad = padding_of(&widget, &style);
    let mark = if open { "▾" } else { "▸" };
    let label = if caption.is_empty() {
        mark.to_owned()
    } else {
        format!("{mark} {caption}")
    };
    let galley = ui.painter().layout_no_wrap(label, font.clone(), color);
    let sizes: Vec<egui::Vec2> = {
        let mut measure = Measure::new(at.eng, at.arena, ui);
        bar.iter()
            .map(|child| measure.of(*child, &at.theme))
            .collect()
    };
    let room = ui.available_rect_before_wrap();
    let tall = sizes
        .iter()
        .fold(galley.size().y, |most, size| most.max(size.y));
    let strip = Rect::from_min_size(room.min, vec2(room.width(), tall));
    // Sensed before the bar's children are drawn, so theirs sit on top.
    let header = ui.interact(strip, ui.id().with(("fold", entity)), Sense::click());
    if header.clicked() {
        at.edits.push((entity, Edit::Open(!open)));
    }
    let text_at = pos2(strip.min.x, strip.center().y - galley.size().y / 2.0);
    let mut across = strip.min.x + galley.size().x + ui.spacing().item_spacing.x;
    ui.painter().galley(text_at, galley, color);
    for (child, size) in bar.iter().zip(sizes) {
        let rect = Rect::from_min_size(pos2(across, strip.center().y - size.y / 2.0), size);
        in_header(ui, at, *child, rect);
        across = rect.max.x + ui.spacing().item_spacing.x;
    }
    if at.focused == Some(entity) {
        ui.painter().rect_stroke(
            strip.expand(2.0),
            4.0,
            Stroke::new(2.0, color),
            egui::StrokeKind::Outside,
        );
    }
    ui.advance_cursor_after_rect(strip);
    if !open {
        return;
    }
    let top = strip.max.y + ui.spacing().item_spacing.y;
    let body = Rect::from_min_max(pos2(room.min.x + pad.left, top), room.max);
    // Solved on its own: the header is drawn here rather than authored, so
    // what is under it is a subtree of its own from the layout's side.
    let space = crate::widget::taffy::Room::scrolling(body, w::BOTH);
    let solved = crate::widget::taffy::solve_subtree(
        at.eng,
        at.arena,
        index,
        ui,
        &at.theme,
        &space,
        at.deep(index),
    );
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(body));
    let held = std::mem::replace(&mut at.rects, solved);
    lay_out(&mut inner, at, index, Axis::Column);
    at.rects = held;
    ui.advance_cursor_after_rect(inner.min_rect());
}

/// Whether `child` is drawn in its fold's header rather than under it.
pub(crate) fn in_title_bar(
    arena: &[crate::widget::arena::Placed],
    parent: usize,
    child: usize,
) -> bool {
    arena[parent].widget.kind == w::FOLD && arena[child].widget.title_bar
}

/// One title bar child in the box the header gave it, and what it lays out.
fn in_header(ui: &mut egui::Ui, at: &mut Painting<'_>, child: usize, rect: Rect) {
    let entity = at.arena[child].entity;
    record_rect(entity, rect);
    let solved = crate::widget::taffy::solve_subtree(
        at.eng,
        at.arena,
        child,
        ui,
        &at.theme,
        &crate::widget::taffy::Room::fixed(rect),
        at.deep(child),
    );
    let held = std::mem::replace(&mut at.rects, solved);
    let restore = std::mem::replace(&mut at.assigned, rect.size());
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    draw_one(&mut inner, at, child);
    record_measure(entity, inner.min_rect().size());
    at.assigned = restore;
    at.rects = held;
}

/// How many across a `grid` puts its children: what it states, or the two
/// it has always drawn when it states nothing.
pub(crate) fn grid_columns(widget: &crate::widget::node::Widget) -> usize {
    if widget.columns == 0 {
        return 2;
    }
    widget.columns as usize
}

/// Children in rows of `columns`, every cell as big as the biggest child
/// and, given a width, sharing it equally.
pub(crate) fn grid(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let children = placed.children.clone();
    if children.is_empty() {
        return;
    }
    let widget = placed.widget.clone();
    let columns = grid_columns(&widget);
    let gap = widget.gap;
    let style = at.style_of(&widget);
    let pad = padding_of(&widget, &style);
    let box_size = solved_of(&widget, &at.style_of(&widget), at.assigned);
    let mut cell = egui::Vec2::ZERO;
    {
        let mut measure = Measure::new(at.eng, at.arena, ui);
        for child in &children {
            cell = cell.max(measure.of(*child, &at.theme));
        }
    }
    if box_size.x > 0.0 {
        let shared = (box_size.x - pad.taken().x - gap * (columns as f32 - 1.0)) / columns as f32;
        cell.x = shared.max(0.0);
    }
    let origin = pad.origin(ui.available_rect_before_wrap().min);
    let mut extent = egui::Vec2::ZERO;
    for (slot, child) in children.iter().enumerate() {
        let (column, row) = ((slot % columns) as f32, (slot / columns) as f32);
        let min = origin + vec2(column * (cell.x + gap), row * (cell.y + gap));
        let rect = Rect::from_min_size(min, cell);
        place_child(ui, at, *child, rect, cell);
        extent = extent.max(rect.max - origin);
    }
    let taken = pad.around(Rect::from_min_size(origin, extent));
    ui.allocate_rect(taken, Sense::hover());
}

/// Children over one another, each in the whole box or placed in it by its
/// own `anchor`: Godot's MarginContainer, and a Control whose children anchor
/// themselves rather than queue up.
pub(crate) fn stack(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let children = placed.children.clone();
    if children.is_empty() {
        return;
    }
    let widget = placed.widget.clone();
    let style = at.style_of(&widget);
    let pad = padding_of(&widget, &style);
    let box_size = solved_of(&widget, &at.style_of(&widget), at.assigned);
    // The box this widget was handed, not what is left after the cursor: a
    // root reserves its box up front, and a stack fills what it was given.
    let room = ui.max_rect();
    let outer = Rect::from_min_size(
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
    // The frame first, under everything the stack holds, as a container's is.
    if style.fill.is_some() || style.stroke.is_some() {
        ui.painter().rect(
            outer,
            egui::CornerRadius::same((style.radius.unwrap_or(0.0)) as u8),
            style.fill.unwrap_or(Color32::TRANSPARENT),
            style
                .stroke
                .map_or(Stroke::NONE, |c| Stroke::new(style.stroke_px(), c)),
            egui::StrokeKind::Inside,
        );
    }
    let area = pad.inside(outer);
    for child in &children {
        let want = {
            let mut measure = Measure::new(at.eng, at.arena, ui);
            measure.of(*child, &at.theme)
        };
        let kid = at.arena[*child].widget.clone();
        let rect = anchored_in(area, &kid, &at.style_of(&kid), want);
        // This kind placed the child, so taffy has not solved what is under
        // it: its subtree is solved against the box it was just given.
        let space = crate::widget::taffy::Room::fixed(rect);
        let solved = crate::widget::taffy::solve_subtree(
            at.eng,
            at.arena,
            *child,
            ui,
            &at.theme,
            &space,
            at.deep(*child),
        );
        let held = std::mem::replace(&mut at.rects, solved);
        place_child(ui, at, *child, rect, rect.size());
        at.rects = held;
    }
    ui.allocate_rect(outer, Sense::hover());
}

/// Where one child of a stack sits: its `anchor` decides whether each axis
/// stretches or holds the child's own size at an edge or the middle, and `x`
/// and `y` push it in from the edges the anchor names.
fn anchored_in(
    area: Rect,
    widget: &Widget,
    style: &crate::widget::theme::Style,
    want: egui::Vec2,
) -> Rect {
    let (across, down) = crate::widget::anchor::in_box(&widget.anchor);
    let stated = solved_of(widget, style, egui::Vec2::ZERO);
    let size = vec2(
        if stated.x > 0.0 { stated.x } else { want.x },
        if stated.y > 0.0 { stated.y } else { want.y },
    );
    let (left, width) = along(area.min.x, area.width(), size.x, across, widget.x);
    let (top, height) = along(area.min.y, area.height(), size.y, down, widget.y);
    Rect::from_min_size(egui::pos2(left, top), vec2(width, height))
}

/// One axis of a stack placement: where the child starts and how wide it is.
///
/// The offset runs inward from the edge the anchor names, as it does on a
/// root; an axis that stretches or centres names no edge and takes none.
fn along(
    start: f32,
    room: f32,
    want: f32,
    place: crate::widget::anchor::In,
    offset: f32,
) -> (f32, f32) {
    use crate::widget::anchor::In;
    let want = want.min(room);
    match place {
        In::Stretch => (start, room),
        In::Start => (start + offset, want),
        In::Middle => (start + (room - want) / 2.0, want),
        In::End => (start + room - want - offset, want),
    }
}

/// Children left to right at their own size, wrapping to a new line when
/// the next one would run past the box.
pub(crate) fn flow(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let children = placed.children.clone();
    if children.is_empty() {
        return;
    }
    let widget = placed.widget.clone();
    let gap = widget.gap;
    let style = at.style_of(&widget);
    let pad = padding_of(&widget, &style);
    let box_size = solved_of(&widget, &at.style_of(&widget), at.assigned);
    let room = ui.available_rect_before_wrap();
    let width = if box_size.x > 0.0 {
        box_size.x
    } else {
        room.width()
    } - pad.taken().x;
    let sizes: Vec<egui::Vec2> = {
        let mut measure = Measure::new(at.eng, at.arena, ui);
        children
            .iter()
            .map(|child| measure.of(*child, &at.theme))
            .collect()
    };
    let origin = pad.origin(room.min);
    let mut cursor = egui::Vec2::ZERO;
    let mut line_height = 0.0f32;
    let mut extent = egui::Vec2::ZERO;
    for (child, size) in children.iter().zip(sizes) {
        if size == egui::Vec2::ZERO {
            continue;
        }
        if cursor.x > 0.0 && cursor.x + size.x > width {
            cursor = vec2(0.0, cursor.y + line_height + gap);
            line_height = 0.0;
        }
        let rect = Rect::from_min_size(origin + cursor, size);
        place_child(ui, at, *child, rect, egui::Vec2::ZERO);
        cursor.x += size.x + gap;
        line_height = line_height.max(size.y);
        extent = extent.max(rect.max - origin);
    }
    let taken = Rect::from_min_size(room.min, extent + pad.taken());
    ui.allocate_rect(taken, Sense::hover());
}

/// Draw one child of a grid or a flow in the rect it was given.
fn place_child(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    child: usize,
    rect: Rect,
    assigned: egui::Vec2,
) {
    let entity = at.arena[child].entity;
    let restore = at.assigned;
    at.assigned = assigned;
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    draw_one(&mut child_ui, at, child);
    at.assigned = restore;
    record_measure(entity, child_ui.min_rect().size());
    record_rect(entity, rect);
}

/// A picture over a rect with its borders kept at their own size: nine
/// quads, the corners as they are, the edges stretched one way and the
/// middle both. `slice` is in the picture's pixels; the borders are drawn
/// at design scale.
pub(crate) fn nine_patch(
    texture: TextureId,
    native: egui::Vec2,
    rect: Rect,
    slice: [f32; 4],
) -> Vec<egui::Shape> {
    let [left, top, right, bottom] = slice;
    let xs = [
        rect.min.x,
        rect.min.x + left,
        rect.max.x - right,
        rect.max.x,
    ];
    let ys = [
        rect.min.y,
        rect.min.y + top,
        rect.max.y - bottom,
        rect.max.y,
    ];
    let us = [
        0.0,
        left / native.x.max(1.0),
        1.0 - right / native.x.max(1.0),
        1.0,
    ];
    let vs = [
        0.0,
        top / native.y.max(1.0),
        1.0 - bottom / native.y.max(1.0),
        1.0,
    ];
    let mut shapes = Vec::with_capacity(9);
    for row in 0..3 {
        for column in 0..3 {
            let piece =
                Rect::from_min_max(pos2(xs[column], ys[row]), pos2(xs[column + 1], ys[row + 1]));
            if piece.width() <= 0.0 || piece.height() <= 0.0 {
                continue;
            }
            let uv =
                Rect::from_min_max(pos2(us[column], vs[row]), pos2(us[column + 1], vs[row + 1]));
            shapes.push(egui::Shape::image(texture, piece, uv, Color32::WHITE));
        }
    }
    shapes
}

/// Fill a reserved plate with a themed nine-patch; a picture that will not
/// load leaves the plate empty rather than taking the frame down.
pub(crate) fn nine_patch_plate(
    ui: &egui::Ui,
    eng: &Engine,
    plate: egui::layers::ShapeIdx,
    path: &str,
    slice: [f32; 4],
    rect: Rect,
) {
    let ctx = ui.ctx().clone();
    if let Ok(texture) = crate::images::texture_of(eng, &ctx, path) {
        let native = crate::images::native_size(eng, path, &texture);
        let shapes = nine_patch(texture.id(), native, rect, slice);
        ui.painter().set(plate, egui::Shape::Vec(shapes));
    }
}

/// The click sensor a widget with a `context` menu puts under its kind.
///
/// Registered before the kind draws, so the kind's own controls stay on top
/// of it and keep their clicks; what it is for is a long touch, which egui
/// only holds on a widget that senses a click, and a label senses none.
pub(crate) fn context_sensor(ui: &egui::Ui, at: &Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    if placed.widget.context.is_empty() {
        return;
    }
    let rect = at
        .rects
        .get(&index)
        .copied()
        .unwrap_or_else(|| ui.max_rect());
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    ui.interact(
        rect,
        egui::Id::new(("balaur-context-under", placed.entity)),
        egui::Sense::CLICK,
    );
}

/// The `context` menu of the widget just drawn: the rows of the `menu` node
/// it names, opened at the pointer by a secondary click or a long touch.
///
/// Read from the input rather than a response: the kind's own controls take
/// the click first, and a list's row or a check's box hands no response
/// back. The innermost widget under the pointer that names a menu takes the
/// press; a widget's own `on_click` never fires for it, egui counting only
/// the primary button as a click. The menu's own button is not drawn here,
/// so a menu that is only ever a context menu can be `visible = false`.
pub(crate) fn context_menu(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let context = &placed.widget.context;
    if context.is_empty() {
        return;
    }
    let menu = at
        .arena
        .iter()
        .position(|one| one.name == *context && one.widget.kind == w::MENU);
    let Some(menu) = menu else {
        if balaur_core::logbuf::first_time("widget context", context) {
            tracing::warn!("widget context '{context}': no `menu` node by that name");
        }
        return;
    };
    let ctx = ui.ctx().clone();
    let pressed = ui.is_enabled()
        && !at.context_opened
        && ui.rect_contains_pointer(ui.min_rect())
        && (ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Secondary))
            || ctx.interaction_snapshot(|s| s.long_touched.is_some()));
    if pressed {
        at.context_opened = true;
    }
    let id = egui::Id::new(("balaur-context", placed.entity));
    if !pressed && !egui::Popup::is_id_open(&ctx, id) {
        return;
    }
    // The rows draw in the menu's own theme, from its own place in the tree,
    // so they look the same as when its button opens them.
    let outer = at.theme.clone();
    let root = crate::widget::layer::theme_root(at.eng);
    at.theme = crate::widget::arena::theme_at(at.eng, at.arena, menu, &root);
    egui::Popup::new(id, ctx, egui::PopupAnchor::PointerFixed, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .style(egui::containers::menu::menu_style)
        .gap(0.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .open_memory(pressed.then_some(egui::SetOpenCommand::Bool(true)))
        .show(|ui| popup_rows(ui, at, menu));
    at.theme = outer;
    at.shown.push(at.arena[menu].entity);
}

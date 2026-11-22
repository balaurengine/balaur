//! The widget kinds past the first nine: the controls a settings screen is
//! made of, the containers a shop is laid out with, and the nine-patch
//! that dresses both. Each is egui's own widget where egui has one, drawn
//! from the scene's values and reporting back through the frame's edits.
//! The row kinds, `list`, `tree` and `table`, are [`super::rows`].

use balaur_core::Engine;
use egui::{Color32, Rect, Sense, Stroke, TextureId, pos2, vec2};

use crate::vocabulary::words as w;
use crate::widget::arrange::{
    Axis, Pad, lay_out, padding_of, record_measure, record_rect, solved_of, style_padding,
};
use crate::widget::layer::{Edit, Painting, draw_one};
use crate::widget::measure::Measure;
use crate::widget::node::Widget;
use crate::widget::paint::{self, Brush};
use crate::widget::sheet::{on_plate, sheet};
use crate::widget::theme::{Style, WidgetState, rich};

/// One of the widget's `options`, chosen from a list that drops down.
pub(crate) fn dropdown(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    font: &egui::FontId,
    color: Color32,
) {
    let slant = at.slant(index);
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let entity = placed.entity;
    let want = solved_of(widget, &at.style(index), at.assigned);
    let mut chosen = widget.text.clone();
    let picked = rich(chosen.as_str(), font, color, slant, &widget.text_look);
    let fold = if widget.wrap {
        egui::TextWrapMode::Wrap
    } else if widget.truncate {
        egui::TextWrapMode::Truncate
    } else {
        ui.wrap_mode()
    };
    let id = ui.make_persistent_id(("balaur-dropdown", entity));
    let popup = id.with("popup");
    let open = egui::Popup::is_id_open(ui.ctx(), popup);
    // As wide as its widest option at least, so picking one does not move
    // what is beside it.
    let metrics = crate::widget::sizing::Metrics::of(&at.look(index).style, ui.spacing());
    let natural = crate::widget::sizing::dropdown_size(
        ui.ctx(),
        ui.style(),
        widget,
        (font, color, slant),
        &metrics,
    );
    let width = Some(if want.x > 0.0 { want.x } else { natural.x });
    let button = ui
        .horizontal(|ui| {
            let response = picker(ui, id, open, picked, (fold, width), &metrics);
            let enabled = ui.is_enabled();
            response.widget_info(|| {
                let mut info = egui::WidgetInfo::new(egui::Role::ComboBox);
                info.enabled = enabled;
                info.current_text_value = Some(widget.text.to_string());
                info
            });
            response
        })
        .inner;
    let height = if widget.egui.list_height > 0.0 {
        widget.egui.list_height
    } else {
        ui.spacing().combo_height
    };
    let list = egui::Popup::menu(&button)
        .id(popup)
        .width(button.rect.width())
        .close_behavior(closing(&widget.egui.close_on));
    let up = crate::widget::popups::kept_inside(list, egui::RectAlign::BOTTOM_START, &[], 0.0)
        .show(|ui| {
            ui.set_min_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height(height)
                .show(ui, |ui| {
                    // A narrow button would wrap its rows early; they run
                    // as wide as they are.
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    for option in &widget.options {
                        let label = rich(option.as_str(), font, color, slant, &widget.text_look);
                        let on = chosen == *option;
                        if crate::widget::button::text_button(ui, label, Some(on)).clicked() {
                            chosen = option.clone();
                        }
                    }
                });
        })
        .is_some();
    if up {
        at.shown.push(entity);
    }
    if chosen != widget.text {
        at.edits.push((entity, Edit::Choice(chosen.to_string())));
    }
}

/// A dropdown's button, sized and painted as egui's combo box: the picked
/// text, a downward triangle at the far edge, at least `width` or the
/// spacing's `combo_width` across, in the look the pointer leaves or the open
/// look while the list shows.
fn picker(
    ui: &mut egui::Ui,
    id: egui::Id,
    open: bool,
    picked: egui::RichText,
    (fold, width): (egui::TextWrapMode, Option<f32>),
    metrics: &crate::widget::sizing::Metrics,
) -> egui::Response {
    let plate = Brush::of(ui).slot();
    let spacing = ui.spacing().clone();
    let margin = metrics.padding;
    let mut outer = ui.available_rect_before_wrap();
    outer.set_height(outer.height().max(metrics.height));
    let mut inner_ui = ui.new_child(egui::UiBuilder::new().max_rect(outer.shrink2(margin)));
    let mark = egui::Vec2::splat(metrics.mark);
    let least = width.unwrap_or(spacing.combo_width) - 2.0 * margin.x;
    let room = if fold == egui::TextWrapMode::Extend {
        f32::INFINITY
    } else {
        inner_ui.available_width() - metrics.gap - mark.x
    };
    let galley = egui::WidgetText::from(picked).into_galley(
        &inner_ui,
        Some(fold),
        room,
        egui::TextStyle::Button,
    );
    let wide = (galley.size().x + metrics.gap + mark.x).max(least);
    let tall = galley.size().y.max(mark.y);
    let (_, slot) = inner_ui.allocate_space(vec2(wide, tall));
    let mut rect = inner_ui.min_rect().expand2(margin);
    rect.set_height(rect.height().max(metrics.height));
    let response = ui.interact(rect, id, Sense::click());
    if ui.is_rect_visible(rect) {
        let look = if open {
            &ui.visuals().widgets.open
        } else {
            ui.style().interact(&response)
        };
        Brush::of(ui).set(
            plate,
            paint::rect(
                rect.expand(look.expansion),
                look.corner_radius,
                look.weak_bg_fill,
                look.bg_stroke,
                egui::StrokeKind::Inside,
            ),
        );
        let icon = egui::Align2::RIGHT_CENTER
            .align_size_within_rect(mark, slot)
            .expand(look.expansion);
        let arrow = Rect::from_center_size(
            icon.center(),
            vec2(icon.width() * 0.7, icon.height() * 0.45),
        );
        Brush::of(ui).add(paint::polygon(
            vec![arrow.left_top(), arrow.right_top(), arrow.center_bottom()],
            look.fg_stroke.color,
            Stroke::NONE,
        ));
        let at = egui::Align2::LEFT_CENTER
            .align_size_within_rect(galley.size(), slot)
            .min;
        Brush::of(ui).galley(at, galley, look.text_color());
    }
    ui.advance_cursor_after_rect(rect);
    response
}

/// A file being edited, with its gutter and its colouring: Godot's `CodeEdit`
/// as a node.
pub(crate) fn code(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let nodes = at.arena;
    let placed = &nodes[index];
    let (entity, widget) = (placed.entity, &placed.widget);
    let want = solved_of(widget, &at.style(index), at.assigned);
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
    let look = crate::widget::code::CodeLook {
        widget,
        theme: &at.theme,
        project: at.arena[index].project,
    };
    let coded = crate::widget::code::code_editor(at.eng, &mut inner, entity, &look);
    if let Some(text) = coded.typed {
        at.edits.push((entity, Edit::Text(text)));
    }
    if let Some(caret) = coded.caret {
        at.edits
            .push((entity, Edit::Caret(caret.x, caret.y, caret.index)));
    }
    // The line alone: what a click on the gutter means is the script's to
    // decide, as a breakpoint in one editor and a value in another.
    if let Some(line) = coded.gutter {
        at.edits.push((entity, Edit::Gutter(line)));
    }
    record_measure(entity, coded.used);
    ui.advance_cursor_after_rect(egui::Rect::from_min_size(inner.max_rect().min, coded.used));
}

/// A button that drops a list of items: Godot's `MenuButton`, and the same
/// list a `PopupMenu` shows. `options` are the entries and `text` is the
/// button; picking one reports it the way a dropdown reports a choice, so a
/// script hears it as `value_changed`.
pub(crate) fn menu(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
    color: Color32,
) {
    let nodes = at.arena;
    let placed = &nodes[index];
    let entity = placed.entity;
    let widget = &placed.widget;
    let fallbacks = fallbacks_of(widget);
    // A menu drawn inside an open menu is a submenu: egui opens it to the
    // side on hover, keeps one of them open at a time, and shuts it with the
    // menu it hangs off.
    let inner = egui::containers::menu::is_in_menu(ui);
    let sheeted = !inner && widget.placement == w::SHEET;
    // The frame round what it opens, a fold's way: the theme's `body` table.
    let body = at.style(index).body.clone();
    // A menu whose rows are nodes: its button is dressed by the theme like any
    // other, and the rows carry the icons, shortcuts and ticks a flat list of
    // strings cannot.
    if !placed.children.is_empty() {
        let response = crate::widget::button::button(ui, at, index, caption, font, color);
        let up = if inner {
            egui::containers::menu::SubMenu::new()
                .show(ui, &response, |ui| {
                    crate::widget::popups::scrolled(ui, |ui| popup_rows(ui, at, index));
                })
                .is_some()
        } else if sheeted {
            sheet(ui, at, index, &response, |ui, at| {
                rows_in(ui, at, index, true)
            })
        } else {
            // A popup of a stated width is filled across, as a sheet is.
            let fills = widget.egui.popup_width > 0.0;
            dropped(&response, widget, &fallbacks, ui, body.is_some())
                .show(|ui| {
                    on_plate(ui, at.eng, body.as_deref(), None, |ui| {
                        if crate::widget::popups::scrolled(ui, |ui| rows_in(ui, at, index, fills)) {
                            ui.close();
                        }
                    });
                })
                .is_some()
        };
        if up {
            at.shown.push(entity);
        }
        return;
    }
    let options = placed.widget.options.clone();
    let look = std::sync::Arc::clone(&placed.widget.text_look);
    let mut picked = None;
    let slant = at.slant(index);
    let label = rich(caption, font, color, slant, &look);
    // What `Ui::menu_button` does, with the placement this node asked for:
    // the same button and the same menu popup, which is all that call is.
    let response = crate::widget::button::text_button(ui, label, None);
    let pick_from = |ui: &mut egui::Ui| {
        let mut chose = None;
        for option in &options {
            let row = rich(option.as_str(), font, color, slant, &look);
            if crate::widget::button::text_button(ui, row, None).clicked() {
                chose = Some(option.to_string());
            }
        }
        chose
    };
    let mut rows = |ui: &mut egui::Ui| {
        if let Some(chose) = pick_from(ui) {
            picked = Some(chose);
            ui.close();
        }
    };
    let up = if inner {
        egui::containers::menu::SubMenu::new()
            .show(ui, &response, |ui| {
                crate::widget::popups::scrolled(ui, rows);
            })
            .is_some()
    } else if sheeted {
        sheet(ui, at, index, &response, |ui, _| {
            let chose = pick_from(ui);
            let shut = chose.is_some();
            if shut {
                picked = chose;
            }
            shut
        })
    } else {
        dropped(&response, widget, &fallbacks, ui, body.is_some())
            .show(|ui| {
                on_plate(ui, at.eng, body.as_deref(), None, |ui| {
                    crate::widget::popups::scrolled(ui, &mut rows);
                });
            })
            .is_some()
    };
    if up {
        at.shown.push(entity);
    }
    if let Some(choice) = picked {
        at.edits.push((entity, Edit::Choice(choice)));
    }
}

/// What a `close_on` word asks of a popup.
fn closing(word: &str) -> egui::PopupCloseBehavior {
    match word {
        w::CLICK => egui::PopupCloseBehavior::CloseOnClick,
        w::NEVER => egui::PopupCloseBehavior::IgnoreClicks,
        _ => egui::PopupCloseBehavior::CloseOnClickOutside,
    }
}

/// The side a placement word names against the button, or `None` for the
/// two that are no side: the pointer and the middle of the screen.
fn side_of(placement: &str) -> Option<egui::RectAlign> {
    use egui::RectAlign as R;
    Some(match placement {
        w::BELOW => R::BOTTOM_START,
        w::BELOW_CENTER => R::BOTTOM,
        w::BELOW_END => R::BOTTOM_END,
        w::ABOVE => R::TOP_START,
        w::ABOVE_CENTER => R::TOP,
        w::ABOVE_END => R::TOP_END,
        w::RIGHT => R::RIGHT_START,
        w::RIGHT_CENTER => R::RIGHT,
        w::RIGHT_END => R::RIGHT_END,
        w::LEFT => R::LEFT_START,
        w::LEFT_CENTER => R::LEFT,
        w::LEFT_END => R::LEFT_END,
        _ => return None,
    })
}

/// The sides a menu's `placement_fallbacks` names, in order.
fn fallbacks_of(widget: &Widget) -> Vec<egui::RectAlign> {
    widget
        .egui
        .placement_fallbacks
        .iter()
        .filter_map(|word| side_of(word))
        .collect()
}

/// A menu's popup, placed where the node says and held open where the scene
/// says. `below` is egui's own: under the button, flipped above it where
/// there is no room.
fn dropped<'a>(
    response: &egui::Response,
    widget: &Widget,
    fallbacks: &'a [egui::RectAlign],
    ui: &egui::Ui,
    plated: bool,
) -> egui::Popup<'a> {
    let o = &widget.egui;
    // The rows decide for themselves unless `close_on` says otherwise: egui's
    // default closes on any click inside, which would shut the menu under a
    // toggle that keeps it open.
    let mut popup = egui::Popup::menu(response).close_behavior(closing(&o.close_on));
    // The theme draws the frame round the rows itself, shadows and all.
    if plated {
        popup = popup.frame(egui::Frame::NONE);
    }
    if o.popup_gap >= 0.0 {
        popup = popup.gap(o.popup_gap);
    }
    if o.popup_width > 0.0 {
        popup = popup.width(o.popup_width);
    }
    if !fallbacks.is_empty() {
        popup = popup.align_alternatives(fallbacks);
    }
    let placement = widget.placement.as_str();
    popup = match placement {
        w::BELOW => crate::widget::popups::kept_inside(
            popup,
            egui::RectAlign::BOTTOM_START,
            fallbacks,
            o.popup_gap.max(0.0),
        ),
        // Where the pointer was when the menu opened, not where it is now:
        // egui only remembers that for a popup told to open, so the click
        // says so itself rather than leaving it to the toggle.
        w::POINTER => {
            let id = egui::Popup::default_response_id(response);
            let open = egui::Popup::is_id_open(ui.ctx(), id);
            let set = response
                .clicked()
                .then_some(egui::SetOpenCommand::Bool(!open));
            crate::widget::popups::kept_inside(
                popup.at_pointer_fixed().open_memory(set),
                egui::RectAlign::BOTTOM_START,
                fallbacks,
                o.popup_gap.max(0.0),
            )
        }
        // Over the middle of the screen, the viewport's where it sits in one,
        // and staying there: a menu the game put in the centre is not one
        // egui may nudge to fit.
        w::CENTER => popup
            .at_position(
                crate::widget::popups::screen()
                    .unwrap_or_else(|| ui.ctx().viewport_rect())
                    .center(),
            )
            .align(egui::RectAlign::over_corner(egui::Align2::CENTER_CENTER))
            .align_alternatives(&[]),
        side => {
            let asked = side_of(side).unwrap_or(egui::RectAlign::BOTTOM_START);
            let gap = o.popup_gap.max(0.0);
            crate::widget::popups::kept_inside(popup.align(asked), asked, fallbacks, gap)
        }
    };
    if widget.showing {
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
    if rows_in(ui, at, index, false) {
        ui.close();
    }
}

/// A menu's rows laid out in `ui`: hugging what they measure, or `fills`, as
/// wide as the room and free to run down, as a sheet's scroll and a popup of
/// a stated width hold them. Answers whether a row that was clicked shuts the
/// menu.
fn rows_in(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize, fills: bool) -> bool {
    let room = ui.available_rect_before_wrap();
    let space = if fills {
        crate::widget::taffy::Room::scrolling(room, w::VERTICAL)
    } else {
        crate::widget::taffy::Room::hugging(room)
    };
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
    let nodes = at.arena;
    // A row that opened a submenu chose nothing, so the menu it sits in
    // stays up; every other row closes it unless it says `keep_open`.
    at.clicked[before..].iter().any(|entity| {
        nodes
            .iter()
            .find(|placed| placed.entity == *entity)
            .is_some_and(|placed| !placed.widget.keep_open && placed.widget.kind != w::MENU)
    })
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
    let want = solved_of(widget, &at.style(index), at.assigned);
    // The swatch's width; the open picker is laid out by egui alone.
    if want.x > 0.0 && !widget.egui.inline {
        ui.spacing_mut().interact_size.x = want.x;
    }
    let alpha = crate::widget::sizing::alpha_of(widget);
    let changed = if widget.egui.inline {
        egui::color_picker::color_picker_color32(ui, &mut srgba, alpha)
    } else {
        egui::color_picker::color_edit_button_srgba(ui, &mut srgba, alpha).changed()
    };
    let unit = srgba.to_srgba_unmultiplied().map(|c| f32::from(c) / 255.0);
    if changed {
        at.edits.push((entity, Edit::Color(unit)));
    }
    // The picker's drag happens in its popup, so the release is watched for
    // here: a colour committed once the button that was changing it lets go.
    let dragging = egui::Id::unique(("balaur-color-drag", entity));
    let (down, released) = ui.input(|i| (i.pointer.any_down(), i.pointer.any_released()));
    let was = ui.data(|d| d.get_temp::<bool>(dragging).unwrap_or(false));
    if changed && down {
        ui.data_mut(|d| d.insert_temp(dragging, true));
    } else if (was && released) || (changed && !down) {
        ui.data_mut(|d| d.insert_temp(dragging, false));
        at.edits.push((entity, Edit::ColorCommitted(unit)));
    }
}

/// A line across the parent's direction, in the theme's stroke.
pub(crate) fn separator(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let widget = &at.arena[index].widget;
    let style = at.style(index);
    if let Some(color) = style.stroke {
        ui.visuals_mut().widgets.noninteractive.bg_stroke = Stroke::new(style.stroke_px(), color);
    }
    let o = &widget.egui;
    let spacing = if o.spacing >= 0.0 { o.spacing } else { 6.0 };
    let across = match widget.axis.as_str() {
        w::HORIZONTAL => true,
        w::VERTICAL => false,
        _ => !ui.layout().main_dir().is_horizontal(),
    };
    let room = if ui.is_sizing_pass() {
        egui::Vec2::ZERO
    } else {
        ui.available_size_before_wrap()
    };
    let size = if across {
        vec2(room.x, spacing)
    } else {
        vec2(spacing, room.y)
    };
    let (rect, response) = ui.allocate_at_least(size, Sense::hover());
    response.widget_info(|| egui::WidgetInfo::new(egui::Role::Unknown));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
    // An overhang past the box below zero pulls the line in from both ends.
    let grow = o.overhang;
    if across {
        Brush::of(ui).hline(
            (rect.left() - grow)..=(rect.right() + grow),
            rect.center().y,
            stroke,
        );
    } else {
        Brush::of(ui).vline(
            rect.center().x,
            (rect.top() - grow)..=(rect.bottom() + grow),
            stroke,
        );
    }
}

/// A header that shows or hides the children under it. The header is a
/// button by another shape: clicking it reports an `Open` edit, and focus
/// lands on it as on a button. While open it wears its `checked` table, and
/// what it shows sits in the theme's `body` frame.
pub(crate) fn fold(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
    color: Color32,
) {
    let nodes = at.arena;
    let widget = &nodes[index].widget;
    let style = at.style(index);
    let room = ui.available_rect_before_wrap();
    let strip = fold_header(ui, at, index, &style, (caption, font, color), room);
    ui.advance_cursor_after_rect(strip);
    if !widget.open {
        return;
    }
    let top = strip.max.y + ui.spacing().item_spacing.y;
    let frame = Brush::of(ui).slot();
    let pad = style
        .body
        .as_deref()
        .map_or_else(Pad::default, |body| style_padding(body, egui::Vec2::ZERO));
    let area = Rect::from_min_max(pos2(room.min.x, top), room.max);
    let body = pad.inside(area);
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
    let bottom = inner.min_rect().max.y + pad.bottom;
    let used = Rect::from_min_max(area.min, pos2(room.max.x, bottom));
    if let Some(body) = &style.body {
        plate(ui, at.eng, frame, body, used);
    }
    ui.advance_cursor_after_rect(used);
}

/// A fold's header, drawn in the fold's state: its frame, its arrow, the
/// caption and the title bar children. Answers the header's box.
fn fold_header(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    style: &Style,
    (caption, font, color): (&str, &egui::FontId, Color32),
    room: Rect,
) -> Rect {
    let placed = &at.arena[index];
    let (entity, open) = (placed.entity, placed.widget.open);
    let pad = padding_of(&placed.widget, style);
    let bar: Vec<usize> = placed
        .children
        .iter()
        .copied()
        .filter(|child| in_title_bar(at.arena, index, *child))
        .collect();
    let gap = ui.spacing().item_spacing.x;
    let mark = font.size;
    let slant = at.slant(index);
    let galley = (!caption.is_empty())
        .then(|| crate::widget::theme::galley(&Brush::of(ui), caption, font, slant));
    let words = galley.as_ref().map_or(0.0, |galley| galley.size().x + gap);
    let sizes: Vec<egui::Vec2> = {
        let mut measure = Measure::new(at.eng, at.arena, ui);
        bar.iter()
            .map(|child| measure.of(*child, &at.theme))
            .collect()
    };
    let spare = room.width() - pad.left - pad.right - mark - gap - words;
    let sizes = grown(at.arena, &bar, sizes, spare, gap);
    let line = galley
        .as_ref()
        .map_or(mark, |galley| galley.size().y.max(mark));
    let tall = sizes.iter().fold(line, |most, size| most.max(size.y));
    let strip = Rect::from_min_size(room.min, vec2(room.width(), tall + pad.top + pad.bottom));
    let frame = Brush::of(ui).slot();
    // Sensed before the bar's children are drawn, so theirs sit on top.
    let header = ui.interact(strip, ui.scope_id().with((w::FOLD, entity)), Sense::click());
    if header.clicked() {
        at.edits.push((entity, Edit::Open(!open)));
    }
    let state = WidgetState {
        pointer: at.pointer(index, &header),
        disabled: false,
        focused: at.focused == Some(entity),
        checked: open,
    };
    let look = at.dressed(index, style, state);
    plate(ui, at.eng, frame, &look, strip);
    let ink = look.text_color.unwrap_or(color);
    let middle = strip.center().y;
    let arrow = Rect::from_center_size(
        pos2(strip.min.x + pad.left + mark / 2.0, middle),
        vec2(mark, mark),
    );
    fold_arrow(ui, at.eng, look.arrow.as_deref(), open, arrow, (font, ink));
    let mut across = arrow.max.x + gap;
    if let Some(galley) = galley {
        let height = galley.size().y;
        Brush::of(ui).galley(pos2(across, middle - height / 2.0), galley, ink);
        across += words;
    }
    for (child, size) in bar.iter().zip(sizes) {
        let rect = Rect::from_min_size(pos2(across, middle - size.y / 2.0), size);
        in_header(ui, at, *child, rect);
        across = rect.max.x + gap;
    }
    if state.focused && style.focus.is_none() {
        let (gap, width) = style.ring();
        Brush::of(ui).outline(
            strip.expand(gap),
            4.0,
            Stroke::new(width, ink),
            egui::StrokeKind::Outside,
        );
    }
    strip
}

/// The theme's arrow picture, or the glyph pointing the way the fold is.
fn fold_arrow(
    ui: &egui::Ui,
    eng: &Engine,
    picture: Option<&str>,
    open: bool,
    rect: Rect,
    (font, ink): (&egui::FontId, Color32),
) {
    let texture = picture.and_then(|path| crate::images::texture_of(eng, ui.ctx(), path).ok());
    if let Some(texture) = texture {
        let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        Brush::of(ui).picture(texture.id(), rect, whole, Color32::WHITE);
        return;
    }
    let mark = if open { "▾" } else { "▸" };
    Brush::of(ui).text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        mark,
        font.clone(),
        ink,
    );
}

/// A style's frame over `rect`, into the slot reserved for it before what
/// sits on it was drawn: its nine-patch, or its fill and outline.
fn plate(ui: &egui::Ui, eng: &Engine, slot: crate::widget::paint::Slot, style: &Style, rect: Rect) {
    if let Some(path) = style.image.as_ref() {
        nine_patch_plate(ui, eng, slot, path, style.slice, rect);
        return;
    }
    if !style.paints() {
        return;
    }
    let radius = if style.round == Some(true) {
        rect.height() / 2.0
    } else {
        style.radius.unwrap_or(0.0)
    };
    Brush::of(ui).set(
        slot,
        crate::widget::frame::frame_shape(
            rect,
            egui::CornerRadius::same(radius.clamp(0.0, 255.0) as u8),
            style.fill.unwrap_or(Color32::TRANSPARENT),
            style,
        ),
    );
}

/// The title bar children's sizes, those with a `grow` sharing what the
/// header has left after the others, as Godot's title bar expands them.
fn grown(
    arena: &[crate::widget::arena::Placed],
    bar: &[usize],
    sizes: Vec<egui::Vec2>,
    width: f32,
    gap: f32,
) -> Vec<egui::Vec2> {
    let growing: f32 = bar
        .iter()
        .map(|child| arena[*child].widget.grow.max(0.0))
        .sum();
    if growing <= 0.0 {
        return sizes;
    }
    let taken: f32 = bar
        .iter()
        .zip(&sizes)
        .filter(|(child, _)| arena[**child].widget.grow <= 0.0)
        .map(|(_, size)| size.x + gap)
        .sum();
    let left = (width - taken).max(0.0);
    bar.iter()
        .zip(sizes)
        .map(|(child, size)| {
            let share = arena[*child].widget.grow.max(0.0) / growing;
            if share > 0.0 {
                vec2(size.x.max(left * share), size.y)
            } else {
                size
            }
        })
        .collect()
}

/// Whether `child` is drawn in its fold's header rather than under it, or is
/// a menu's sheet head where the menu is not laying its rows out as a sheet
/// (a dropdown, a submenu, a context menu), and is left out.
pub(crate) fn in_title_bar(
    arena: &[crate::widget::arena::Placed],
    parent: usize,
    child: usize,
) -> bool {
    let host = &arena[parent].widget;
    arena[child].widget.title_bar
        && (host.kind == w::FOLD
            || (host.kind == w::MENU && !crate::widget::sheet::sheeting(parent)))
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

/// Children over one another, each in the whole box or placed in it by its
/// own `anchor`: Godot's MarginContainer, and a Control whose children anchor
/// themselves rather than queue up.
pub(crate) fn stack(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let nodes = at.arena;
    let placed = &nodes[index];
    let children = &placed.children;
    if children.is_empty() {
        return;
    }
    let widget = &placed.widget;
    let style = at.style(index);
    let pad = padding_of(widget, &style);
    let box_size = solved_of(widget, &at.style(index), at.assigned);
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
    if style.paints() {
        Brush::of(ui).add(crate::widget::frame::frame_shape(
            outer,
            egui::CornerRadius::same((style.radius.unwrap_or(0.0)) as u8),
            style.fill.unwrap_or(Color32::TRANSPARENT),
            &style,
        ));
    }
    let area = pad.inside(outer);
    for child in children {
        let want = {
            let mut measure = Measure::new(at.eng, at.arena, ui);
            measure.of(*child, &at.theme)
        };
        let nodes = at.arena;
        let rect = anchored_in(area, &nodes[*child].widget, &at.style(*child), want);
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

/// Draw one child of a stack in the rect it was given.
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
) -> Vec<crate::widget::paint::Shape> {
    nine_patch_tinted(texture, native, rect, slice, Color32::WHITE)
}

pub(crate) fn nine_patch_tinted(
    texture: TextureId,
    native: egui::Vec2,
    rect: Rect,
    slice: [f32; 4],
    tint: Color32,
) -> Vec<crate::widget::paint::Shape> {
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
            shapes.push(paint::picture(texture, piece, uv, tint));
        }
    }
    shapes
}

/// Fill a reserved plate with a themed nine-patch; a picture that will not
/// load leaves the plate empty rather than taking the frame down.
pub(crate) fn nine_patch_plate(
    ui: &egui::Ui,
    eng: &Engine,
    plate: crate::widget::paint::Slot,
    path: &str,
    slice: [f32; 4],
    rect: Rect,
) {
    let ctx = ui.ctx().clone();
    if let Ok(texture) = crate::images::texture_of(eng, &ctx, path) {
        let native = crate::images::native_size(eng, path, &texture);
        let shapes = nine_patch(texture.id(), native, rect, slice);
        Brush::of(ui).set(plate, paint::group(shapes));
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
        crate::widget::layer::sensor_id(ui, "balaur-context-under", placed.entity),
        egui::Sense::CLICK,
    );
}

/// The `context` menu of the widget just drawn: the rows of the `menu` node
/// it names, opened at the pointer by a secondary click or a long touch.
///
/// Read from the input rather than a response: the kind's own controls take
/// the click first, and a list's row or a check's box hands no response
/// back. The innermost widget under the pointer that names a menu takes the
/// press; the widget's own `pointer_click` is never said for it, egui
/// counting only the primary button as a click. The menu's own button is not drawn here,
/// so a menu that is only ever a context menu names no caption and no icon.
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
    let id = egui::Id::unique(("balaur-context", placed.entity));
    if !pressed && !egui::Popup::is_id_open(&ctx, id) {
        return;
    }
    // The rows draw in the menu's own theme, from its own place in the tree,
    // so they look the same as when its button opens them.
    let outer = at.theme.clone();
    let root = crate::widget::layer::theme_root(at.eng);
    at.theme = crate::widget::arena::theme_at(at.eng, at.arena, menu, &root);
    // On the menu's `body` plate where its theme draws one, as its dropdown.
    let body = at.style(menu).body.clone();
    let mut popup = egui::Popup::new(id, ctx, egui::PopupAnchor::PointerFixed, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .style(egui::containers::menu::menu_style)
        .gap(0.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .open_memory(pressed.then_some(egui::SetOpenCommand::Bool(true)));
    if body.is_some() {
        popup = popup.frame(egui::Frame::NONE);
    }
    let popup = crate::widget::popups::kept_inside(popup, egui::RectAlign::BOTTOM_START, &[], 0.0);
    popup.show(|ui| {
        let eng = at.eng;
        on_plate(ui, eng, body.as_deref(), None, |ui| {
            crate::widget::popups::scrolled(ui, |ui| popup_rows(ui, at, menu));
        });
    });
    at.theme = outer;
    at.shown.push(at.arena[menu].entity);
}

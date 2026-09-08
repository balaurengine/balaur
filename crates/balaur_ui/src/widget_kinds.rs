//! The widget kinds past the first nine: the controls a settings screen is
//! made of, the containers a shop is laid out with, and the nine-patch
//! that dresses both. Each is egui's own widget where egui has one, drawn
//! from the scene's values and reporting back through the frame's edits.

use balaur_core::Engine;
use balaur_core::hecs::Entity;
use egui::{Color32, Rect, Sense, Stroke, TextureId, pos2, vec2};

use crate::widget_arrange::{
    Axis, box_of, hold_to, lay_out, padding_of, record_measure, record_rect,
};
use crate::widget_layer::{Edit, Painting, draw_one};
use crate::widget_measure::Measure;

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
    let want = box_of(widget, at.assigned, at.scale);
    let mut chosen = widget.text.clone();
    let mut combo = egui::ComboBox::from_id_salt(("balaur-dropdown", entity))
        .selected_text(egui::RichText::new(&chosen).font(font.clone()).color(color));
    if want.x > 0.0 {
        combo = combo.width(want.x);
    }
    combo.show_ui(ui, |ui| {
        for option in &widget.options {
            let label = egui::RichText::new(option).font(font.clone()).color(color);
            ui.selectable_value(&mut chosen, option.clone(), label);
        }
    });
    if chosen != widget.text {
        at.edits.push((entity, Edit::Choice(chosen)));
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
    let want = box_of(widget, at.assigned, at.scale);
    let width = if want.x > 0.0 {
        want.x
    } else {
        ui.available_width().min(160.0 * at.scale)
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
}

/// A scrolling list of `options`, one row each: Godot's `ItemList`. `text` is
/// the row picked, and `row_height` is the pitch.
///
/// Only the rows on screen are built, so a list of the whole document costs
/// its viewport rather than its length. A `tree` is this with a depth read off
/// each row, so both go through here.
fn rows(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    font: &egui::FontId,
    color: Color32,
    indent: bool,
) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let entity = placed.entity;
    let want = box_of(widget, at.assigned, at.scale);
    let row_h = if widget.height > 0.0 && !indent {
        widget.height * at.scale
    } else {
        ui.text_style_height(&egui::TextStyle::Body).max(1.0)
    };
    let items = widget.options.clone();
    let chosen = widget.text.clone();
    let id = egui::Id::new(("balaur-list", entity));

    // A tab a row starts with is one level in, which is how an outline is
    // written down and what keeps a tree inside a list of strings.
    let depth_of = |item: &String| {
        if indent {
            item.len() - item.trim_start_matches('\t').len()
        } else {
            0
        }
    };
    // Folded branches are the tree's own business, so a script hands over the
    // whole outline and never hears about a caret.
    let shut: std::collections::BTreeSet<String> = ui.data(|d| d.get_temp(id).unwrap_or_default());
    let mut open_rows: Vec<usize> = Vec::new();
    let mut hidden_under: Option<usize> = None;
    for (i, item) in items.iter().enumerate() {
        let depth = depth_of(item);
        if hidden_under.is_some_and(|under| depth > under) {
            continue;
        }
        hidden_under = None;
        open_rows.push(i);
        if shut.contains(item) {
            hidden_under = Some(depth);
        }
    }

    // Where each open row's branch still continues, so a guide is drawn only
    // down a level that has another row below this one.
    let trails = branches(&items, &open_rows, &depth_of);
    let mut picked = None;
    let mut toggled = None;
    let mut area = egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false]);
    if want.y > 0.0 {
        area = area.max_height(want.y);
    }
    area.show_rows(ui, row_h, open_rows.len(), |ui, range| {
        for slot in range {
            let Some(&i) = open_rows.get(slot) else {
                continue;
            };
            let item = &items[i];
            let depth = depth_of(item);
            let parent = indent && items.get(i + 1).is_some_and(|next| depth_of(next) > depth);
            ui.horizontal(|ui| {
                if depth > 0 {
                    let head = ui.cursor().min;
                    ui.add_space(row_h * depth as f32);
                    guides(
                        ui,
                        head,
                        row_h,
                        trails.get(slot).map_or(&[][..], Vec::as_slice),
                    );
                }
                if parent {
                    let caret = if shut.contains(item) { "▸" } else { "▾" };
                    let mark = egui::RichText::new(caret).font(font.clone()).color(color);
                    if ui.selectable_label(false, mark).clicked() {
                        toggled = Some(item.clone());
                    }
                } else if indent {
                    ui.add_space(row_h);
                }
                let (icon, label, trailing, tint) = fields(item);
                let color = tint.unwrap_or(color);
                if !icon.is_empty() {
                    // The icon field is a glyph from the project's icon face,
                    // not a character in the UI one.
                    let mark = egui::FontId::new(font.size, crate::theme::family("icon"));
                    ui.label(egui::RichText::new(icon).font(mark).color(color));
                }
                let text = egui::RichText::new(label).font(font.clone()).color(color);
                let hit = ui.selectable_label(*item == chosen, text);
                if !trailing.is_empty() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(trailing)
                                .font(font.clone())
                                .color(color),
                        );
                    });
                }
                if hit.clicked() {
                    picked = Some(item.clone());
                }
            });
        }
    });
    if let Some(item) = toggled {
        let mut next = shut;
        if !next.remove(&item) {
            next.insert(item);
        }
        ui.data_mut(|d| d.insert_temp(id, next));
    }
    if let Some(item) = picked {
        at.clicked.push(entity);
        at.edits.push((entity, Edit::Choice(item)));
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
    let want = box_of(&widget, at.assigned, at.scale);
    let id = format!("balaur-code-{}", entity.to_bits());
    let opts = crate::widgets::code_opts(&widget, at.scale);
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
    let edited = crate::widgets::code_editor(at.eng, &id, &widget.text, &opts);
    crate::bridge::pop();
    match edited {
        Ok((text, changed, _, _)) if changed => at.edits.push((entity, Edit::Text(text))),
        Ok(_) => {}
        Err(err) => warn_code(&err),
    }
    let used = inner.min_rect().size();
    record_measure(entity, used);
    ui.advance_cursor_after_rect(egui::Rect::from_min_size(inner.max_rect().min, used));
}

fn warn_code(err: &anyhow::Error) {
    tracing::warn!("code widget: {err:#}");
}

/// Whether each level above a row still has a row below it, and whether the
/// row itself has a later sibling. One entry a level, plus the row's own.
fn branches(
    items: &[String],
    open: &[usize],
    depth_of: &impl Fn(&String) -> usize,
) -> Vec<Vec<bool>> {
    let depths: Vec<usize> = open.iter().map(|&i| depth_of(&items[i])).collect();
    depths
        .iter()
        .enumerate()
        .map(|(n, &own)| {
            (0..=own)
                .map(|level| {
                    depths[n + 1..]
                        .iter()
                        .find(|later| **later <= level)
                        .is_some_and(|later| *later == level)
                })
                .collect()
        })
        .collect()
}

/// The lines down an outline: a vertical while a branch still has rows below
/// it, and a dash into the row itself. The last child of a branch gets the
/// dash alone, so a run of corners does not read as a ladder.
fn guides(ui: &egui::Ui, head: egui::Pos2, step: f32, trail: &[bool]) {
    let Some(own) = trail.len().checked_sub(1) else {
        return;
    };
    let ink = ui.visuals().weak_text_color().gamma_multiply(0.55);
    let stroke = Stroke::new(1.0, ink);
    let middle = head.y + step / 2.0;
    // One vertical at most: the shallowest level whose branch carries on.
    // Everything inside it is a dash, so a deep row reads `| - -` rather than
    // a wall of pipes.
    let pipe = trail.iter().position(|carries| *carries);
    for level in 0..own {
        let x = head.x + (level as f32 + 0.5) * step;
        if pipe == Some(level) {
            ui.painter()
                .line_segment([pos2(x, head.y), pos2(x, head.y + step)], stroke);
        }
        // The dash leads in: from the pipe, or from where one would be.
        ui.painter()
            .line_segment([pos2(x, middle), pos2(x + step * 0.42, middle)], stroke);
    }
}

/// A row's parts: icon, label, trailing note and an `#rrggbb` of its own,
/// separated by U+001F. `ItemList` carries an icon and a per-item colour the
/// same way, without a second array to keep in step with the first. Leading
/// tabs are the tree's depth and are not a field.
fn fields(item: &str) -> (&str, &str, &str, Option<Color32>) {
    let body = item.trim_start_matches('\t');
    let mut parts = body.split('\u{1f}');
    let (a, b, c, d) = (parts.next(), parts.next(), parts.next(), parts.next());
    let tint = d.and_then(crate::theme::parse_hex);
    match (a, b, c) {
        (Some(icon), Some(label), Some(trailing)) => (icon, label, trailing, tint),
        (Some(icon), Some(label), None) => (icon, label, "", tint),
        (Some(label), None, None) => ("", label, "", tint),
        _ => ("", body, "", tint),
    }
}

/// Godot's `ItemList`, in its line mode and its icon mode: above one
/// `columns` the rows flow into a grid of cards instead of a column of lines.
pub(crate) fn list(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    font: &egui::FontId,
    color: Color32,
) {
    if at.arena[index].widget.columns > 1 {
        cards(ui, at, index, font, color);
        return;
    }
    rows(ui, at, index, font, color, false);
}

/// One card: the icon over the label, in a box the caller sized.
///
/// The same U+001F fields a row splits on, so a view moves between the two
/// modes by setting `columns` and changing nothing else.
fn card(
    ui: &mut egui::Ui,
    item: &str,
    size: egui::Vec2,
    font: &egui::FontId,
    color: Color32,
    on: bool,
    sheet: Option<&egui::TextureHandle>,
) -> bool {
    let (icon, label, trailing, tint) = fields(item);
    let color = tint.unwrap_or(color);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if on {
        ui.visuals().selection.bg_fill
    } else if response.hovered() {
        ui.visuals().widgets.hovered.bg_fill
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(5), fill);
    let mut head = rect.top() + 6.0;
    // A list that names a `source` reads the icon field as `x,y,w,h` in that
    // picture's own pixels, which is how an atlas picker shows its tiles.
    let region = sheet.and_then(|sheet| region_uv(sheet.size_vec2(), icon));
    if let (Some(sheet), Some(uv)) = (sheet, region) {
        let side = (size.y * 0.6).min(size.x * 0.6).max(1.0);
        let face = egui::Rect::from_center_size(
            pos2(rect.center().x, head + side / 2.0),
            egui::Vec2::splat(side),
        );
        ui.painter().image(sheet.id(), face, uv, Color32::WHITE);
        head += side + 4.0;
    } else if !icon.is_empty() {
        // The project's icon face, at the card's own size rather than the
        // label's: an icon mode that drew the glyph at line height is a list.
        let mark = egui::FontId::new(
            (size.y * 0.44).min(size.x * 0.42),
            crate::theme::family("icon"),
        );
        let galley = ui.painter().layout_no_wrap(icon.to_owned(), mark, color);
        ui.painter().galley(
            pos2(rect.center().x - galley.size().x / 2.0, head),
            galley.clone(),
            color,
        );
        head += galley.size().y + 4.0;
    }
    let text = ui.painter().layout(
        label.to_owned(),
        font.clone(),
        color,
        (size.x - 8.0).max(8.0),
    );
    ui.painter().galley(
        pos2(rect.center().x - text.size().x / 2.0, head),
        text,
        color,
    );
    if !trailing.is_empty() {
        response.clone().on_hover_text(trailing);
    }
    response.clicked()
}

/// A row's `x,y,w,h` in the sheet's own pixels, as egui's unit coordinates.
/// `None` for anything that is not four numbers, which is a glyph instead.
fn region_uv(native: egui::Vec2, field: &str) -> Option<Rect> {
    if native.x <= 0.0 || native.y <= 0.0 {
        return None;
    }
    let mut parts = field.split(',').map(|n| n.trim().parse::<f32>());
    let (x, y, w, h) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    if w <= 0.0 || h <= 0.0 || parts.next().is_some() {
        return None;
    }
    Some(Rect::from_min_max(
        pos2(
            (x / native.x).clamp(0.0, 1.0),
            (y / native.y).clamp(0.0, 1.0),
        ),
        pos2(
            ((x + w) / native.x).clamp(0.0, 1.0),
            ((y + h) / native.y).clamp(0.0, 1.0),
        ),
    ))
}

/// The cards, wrapped into rows of `columns` and scrolled a row at a time.
fn cards(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    font: &egui::FontId,
    color: Color32,
) {
    let placed = &at.arena[index];
    let (entity, widget) = (placed.entity, placed.widget.clone());
    let want = box_of(&widget, at.assigned, at.scale);
    let columns = widget.columns.max(1) as usize;
    let items = widget.options.clone();
    let chosen = widget.text.clone();
    let gap = 6.0 * at.scale;
    let room = if want.x > 0.0 {
        want.x
    } else {
        ui.available_width()
    };
    let side = ((room - gap * (columns as f32 - 1.0)) / columns as f32).max(24.0);
    // `row_height` is the pitch of a row, so for a card it is the card's own
    // height; without one a card is a little shorter than it is wide, the
    // icon taking the square and the label sitting under it.
    let tall = if widget.row_height > 0.0 {
        widget.row_height * at.scale
    } else {
        side * 0.86
    };
    let cell = egui::vec2(side, tall);
    let lines = items.len().div_ceil(columns);
    let sheet = (!widget.source.is_empty())
        .then(|| crate::images::texture_of(at.eng, &ui.ctx().clone(), &widget.source).ok())
        .flatten();
    let mut picked = None;
    let mut area = egui::ScrollArea::vertical()
        .id_salt(egui::Id::new(("balaur-cards", entity)))
        .auto_shrink([false, false]);
    if want.y > 0.0 {
        area = area.max_height(want.y);
    }
    area.show_rows(ui, cell.y + gap, lines, |ui, range| {
        for line in range {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
                for slot in 0..columns {
                    let Some(item) = items.get(line * columns + slot) else {
                        break;
                    };
                    if card(ui, item, cell, font, color, *item == chosen, sheet.as_ref()) {
                        picked = Some(item.clone());
                    }
                }
            });
        }
    });
    if let Some(item) = picked {
        at.clicked.push(entity);
        at.edits.push((entity, Edit::Choice(item)));
    }
}

/// Godot's `Tree`, as an outline: a row's leading tabs are its depth.
pub(crate) fn tree(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    font: &egui::FontId,
    color: Color32,
) {
    rows(ui, at, index, font, color, true);
}

/// Godot's `Tree` with named columns: `options` holds the rows, each split on
/// U+001F into one cell a column, and `text` is the row picked.
///
/// The header comes from the widget's own `text` when it names the columns the
/// same way; without one the first row is drawn as the header.
pub(crate) fn table(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    font: &egui::FontId,
    color: Color32,
) {
    let placed = &at.arena[index];
    let (entity, widget) = (placed.entity, placed.widget.clone());
    let want = box_of(&widget, at.assigned, at.scale);
    let heads: Vec<&str> = widget
        .placeholder
        .split('\u{1f}')
        .filter(|head| !head.is_empty())
        .collect();
    let columns = heads.len().max(1);
    let items = widget.options.clone();
    let chosen = widget.text.clone();
    let row_h = if widget.row_height > 0.0 {
        widget.row_height * at.scale
    } else {
        ui.text_style_height(&egui::TextStyle::Body).max(1.0)
    };
    let mut picked = None;
    egui::Grid::new(("balaur-table", entity))
        .num_columns(columns)
        .striped(true)
        .min_row_height(row_h)
        .show(ui, |ui| {
            for head in &heads {
                ui.label(
                    egui::RichText::new(*head)
                        .font(font.clone())
                        .color(color)
                        .strong(),
                );
            }
            if !heads.is_empty() {
                ui.end_row();
            }
            for item in &items {
                for cell in item.split('\u{1f}').take(columns) {
                    if ui
                        .selectable_label(
                            *item == chosen,
                            egui::RichText::new(cell).font(font.clone()).color(color),
                        )
                        .clicked()
                    {
                        picked = Some(item.clone());
                    }
                }
                ui.end_row();
            }
        });
    if want.y > 0.0 {
        hold_to(ui, egui::vec2(want.x, want.y));
    }
    if let Some(item) = picked {
        at.clicked.push(entity);
        at.edits.push((entity, Edit::Choice(item)));
    }
}

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
    let widget = &placed.widget;
    let entity = placed.entity;
    let mut picked = None;
    let label = egui::RichText::new(caption).font(font.clone()).color(color);
    ui.menu_button(label, |ui| {
        for option in &widget.options {
            let item = egui::RichText::new(option).font(font.clone()).color(color);
            if ui.button(item).clicked() {
                picked = Some(option.clone());
                ui.close();
            }
        }
    });
    if let Some(choice) = picked {
        at.edits.push((entity, Edit::Choice(choice)));
    }
}

/// A swatch that opens a picker: Godot's `ColorPickerButton`. The colour is
/// the widget's own `color`, not the ink its caption is drawn in.
pub(crate) fn color(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let entity = placed.entity;
    let [r, g, b, a] = widget.color;
    let mut rgba = egui::Rgba::from_rgba_unmultiplied(r, g, b, a);
    let want = box_of(widget, at.assigned, at.scale);
    if want.x > 0.0 {
        ui.spacing_mut().interact_size.x = want.x;
    }
    if egui::color_picker::color_edit_button_rgba(
        ui,
        &mut rgba,
        egui::color_picker::Alpha::OnlyBlend,
    )
    .changed()
    {
        let [r, g, b, a] = rgba.to_rgba_unmultiplied();
        at.edits.push((entity, Edit::Color([r, g, b, a])));
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
    let bounded = widget.max > widget.min && (widget.min, widget.max) != (0.0, 1.0);
    if bounded {
        drag = drag.range(widget.min..=widget.max);
    }
    if widget.step > 0.0 {
        drag = drag.speed(widget.step);
    }
    let want = box_of(widget, at.assigned, at.scale);
    if want.x > 0.0 {
        ui.spacing_mut().interact_size.x = want.x;
    }
    let response = ui.scope(|ui| {
        ui.style_mut().override_font_id = Some(font.clone());
        ui.visuals_mut().override_text_color = Some(color);
        ui.add(drag)
    });
    if response.inner.changed() {
        at.edits.push((placed.entity, Edit::Value(value)));
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
    let want = box_of(widget, at.assigned, at.scale);
    let mut bar = egui::ProgressBar::new(fraction).desired_width(if want.x > 0.0 {
        want.x
    } else {
        ui.available_width().min(160.0 * at.scale)
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
    ui.add(egui::Separator::default().spacing(6.0 * at.scale));
}

/// A header that shows or hides the children under it. The header is a
/// button by another shape: clicking it reports an `Open` edit, and focus
/// lands on it as on a button.
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
    let style = at.style_of(&widget);
    let scale = at.scale;
    let pad = padding_of(&widget, &style, scale);
    let mark = if open { "▾" } else { "▸" };
    let text = egui::RichText::new(format!("{mark} {caption}"))
        .font(font.clone())
        .color(color);
    let header = ui.add(egui::Label::new(text).sense(Sense::click()));
    if header.clicked() {
        at.edits.push((entity, Edit::Open(!open)));
    }
    if at.focused == Some(entity) {
        ui.painter().rect_stroke(
            header.rect.expand(2.0),
            4.0,
            Stroke::new(2.0, color),
            egui::StrokeKind::Outside,
        );
    }
    if !open {
        return;
    }
    let room = ui.available_rect_before_wrap();
    let body = Rect::from_min_max(pos2(room.min.x + pad, room.min.y), room.max);
    // Solved on its own: the header is drawn here rather than authored, so
    // what is under it is a subtree of its own from the layout's side.
    let space = crate::widget_taffy::Room::scrolling(body);
    let solved = crate::widget_taffy::solve_subtree(
        at.eng, at.arena, index, ui, at.scale, &at.theme, &space,
    );
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(body));
    let held = std::mem::replace(&mut at.rects, solved);
    lay_out(&mut inner, at, index, Axis::Column);
    at.rects = held;
    ui.advance_cursor_after_rect(inner.min_rect());
}

/// How many across a `grid` puts its children: what it states, or the two
/// it has always drawn when it states nothing.
pub(crate) fn grid_columns(widget: &crate::widget_layer::Widget) -> usize {
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
    let scale = at.scale;
    let columns = grid_columns(&widget);
    let gap = widget.gap * scale;
    let style = at.style_of(&widget);
    let pad = padding_of(&widget, &style, scale);
    let box_size = box_of(&widget, at.assigned, scale);
    let mut cell = egui::Vec2::ZERO;
    {
        let mut measure = Measure::new(at.eng, at.arena, ui, scale);
        for child in &children {
            cell = cell.max(measure.of(*child, &at.theme));
        }
    }
    if box_size.x > 0.0 {
        let shared = (box_size.x - 2.0 * pad - gap * (columns as f32 - 1.0)) / columns as f32;
        cell.x = shared.max(0.0);
    }
    let origin = ui.available_rect_before_wrap().min + egui::Vec2::splat(pad);
    let mut extent = egui::Vec2::ZERO;
    for (slot, child) in children.iter().enumerate() {
        let (column, row) = ((slot % columns) as f32, (slot / columns) as f32);
        let min = origin + vec2(column * (cell.x + gap), row * (cell.y + gap));
        let rect = Rect::from_min_size(min, cell);
        place_child(ui, at, *child, rect, cell);
        extent = extent.max(rect.max - origin);
    }
    let taken = Rect::from_min_size(
        origin - egui::Vec2::splat(pad),
        extent + egui::Vec2::splat(pad * 2.0),
    );
    ui.allocate_rect(taken, Sense::hover());
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
    let scale = at.scale;
    let gap = widget.gap * scale;
    let style = at.style_of(&widget);
    let pad = padding_of(&widget, &style, scale);
    let box_size = box_of(&widget, at.assigned, scale);
    let room = ui.available_rect_before_wrap();
    let width = if box_size.x > 0.0 {
        box_size.x
    } else {
        room.width()
    } - 2.0 * pad;
    let sizes: Vec<egui::Vec2> = {
        let mut measure = Measure::new(at.eng, at.arena, ui, scale);
        children
            .iter()
            .map(|child| measure.of(*child, &at.theme))
            .collect()
    };
    let origin = room.min + egui::Vec2::splat(pad);
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
    let taken = Rect::from_min_size(room.min, extent + egui::Vec2::splat(pad * 2.0));
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

/// The dimmed, deaf screen under a dialog: one full-surface area that takes
/// every click so nothing behind the dialog hears them.
pub(crate) fn dialog_backdrop(ctx: &egui::Context, entity: Entity, area: Rect) {
    egui::Area::new(egui::Id::new(("balaur-dialog-backdrop", entity)))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .interactable(true)
        .fade_in(false)
        .show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(area.size(), Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(140));
        });
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
    scale: f32,
) -> Vec<egui::Shape> {
    let [left, top, right, bottom] = slice;
    let xs = [
        rect.min.x,
        rect.min.x + left * scale,
        rect.max.x - right * scale,
        rect.max.x,
    ];
    let ys = [
        rect.min.y,
        rect.min.y + top * scale,
        rect.max.y - bottom * scale,
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
    scale: f32,
) {
    let ctx = ui.ctx().clone();
    if let Ok(texture) = crate::images::texture_of(eng, &ctx, path) {
        let shapes = nine_patch(texture.id(), texture.size_vec2(), rect, slice, scale);
        ui.painter().set(plate, egui::Shape::Vec(shapes));
    }
}

/// The offset a finger past a scroll's deadzone asks for, or `None` while
/// nothing is dragging that far. The press is remembered with the offset
/// the scroll had then, so the content follows the finger from there.
pub(crate) fn deadzone_drag(
    ui: &egui::Ui,
    eng: &Engine,
    entity: Entity,
    dead: f32,
) -> Option<egui::Vec2> {
    let (down, origin, latest) = ui.input(|i| {
        (
            i.pointer.primary_down(),
            i.pointer.press_origin(),
            i.pointer.latest_pos(),
        )
    });
    let state = eng.resource::<crate::UiState>();
    let mut state = state.borrow_mut();
    let key = entity.to_bits().get();
    if !down {
        state.scroll_drags.remove(&key);
        return None;
    }
    let (origin, latest) = (origin?, latest?);
    if let std::collections::hash_map::Entry::Vacant(slot) = state.scroll_drags.entry(key) {
        let inside =
            crate::widget_arrange::drawn_at(entity).is_some_and(|rect| rect.contains(origin));
        if !inside {
            return None;
        }
        let id = ui.make_persistent_id(("balaur-scroll", entity));
        let offset =
            egui::scroll_area::State::load(ui.ctx(), id).map_or(egui::Vec2::ZERO, |s| s.offset);
        slot.insert((origin, offset));
    }
    let (start, base) = state.scroll_drags[&key];
    let travelled = latest - start;
    if travelled.length() < dead {
        return None;
    }
    Some((base - travelled).max(egui::Vec2::ZERO))
}

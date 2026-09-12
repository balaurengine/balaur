//! The row kinds: `list`, `tree` and `table`, their card layout, and the
//! single row they all build on screen.

use egui::{Color32, Rect, Sense, Stroke, pos2, vec2};

use crate::widget::arrange::{box_of, hold_to};
use crate::widget::layer::{Edit, Painting};

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
    // `row_height` is the pitch, and a `list` has always spelled it `height`.
    let stated = match (widget.row_height > 0.0, widget.height > 0.0 && !indent) {
        (true, _) => widget.row_height * at.scale,
        (false, true) => widget.height * at.scale,
        (false, false) => ui.text_style_height(&egui::TextStyle::Body).max(1.0),
    };
    // A row is drawn at the pitch it is placed at, so the pitch has to hold
    // its text: the two drifting apart is a list that scrolls off its own bar.
    let text_h = ui.fonts_mut(|f| f.row_height(font));
    let row_h = stated.max(text_h);
    // The pitch is the row, with nothing between: `show_rows` places by the
    // one and the rows draw at the other, and a gap makes them two numbers.
    ui.spacing_mut().item_spacing.y = 0.0;
    let items: Vec<String> = widget
        .options
        .iter()
        .map(smol_str::SmolStr::to_string)
        .collect();
    let chosen = widget.text.to_string();
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
    // Both ways: a row wider than the list is a log line or a long node
    // name, and a bar to reach the end of it is better than the end being
    // painted over whatever sits beside the list.
    let mut area = egui::ScrollArea::both()
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
            let hit = row(
                ui,
                &Row {
                    item,
                    slot,
                    depth,
                    parent,
                    indent,
                    row_h,
                    trail: trails.get(slot).map_or(&[][..], Vec::as_slice),
                    shut: shut.contains(item),
                    chosen: &chosen,
                    font,
                    color,
                },
            );
            if hit.folded {
                toggled = Some(item.clone());
            }
            if hit.picked {
                picked = Some(item.clone());
            }
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

/// Take the resting stroke off the rows, and its width off their padding.
///
/// egui takes a button's frame margin as its padding less the stroke it would
/// draw, but draws a resting button without that frame and never adds the
/// stroke back: a row would grow 2 px the moment it is hovered or picked, and
/// push every row under it down.
fn steady_height(ui: &mut egui::Ui) {
    let edge = ui.visuals().widgets.inactive.bg_stroke.width;
    if edge <= 0.0 {
        return;
    }
    ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
    ui.spacing_mut().button_padding -= egui::Vec2::splat(edge);
}

/// One row of a list or tree, and what a click on it meant.
struct Row<'a> {
    item: &'a str,
    /// Which row this is in the open walk. Two nodes of the same name at the
    /// same depth write the same string, so the string is not an identity.
    slot: usize,
    depth: usize,
    /// Whether a caret is drawn, because the next row is deeper than this one.
    parent: bool,
    indent: bool,
    row_h: f32,
    /// Where each level above this row still carries on, for the guides.
    trail: &'a [bool],
    shut: bool,
    chosen: &'a str,
    font: &'a egui::FontId,
    color: Color32,
}

struct Hit {
    picked: bool,
    folded: bool,
    /// Whether the pointer is on the caret rather than the row, so the row
    /// still lights up while the branch is being aimed at.
    folded_hovered: bool,
}

/// Draw one row: the guides down its indent, its caret, its icon field, its
/// label, and whatever it trails on the right.
///
/// The whole row is the hit area and paints its own background. Built out of
/// buttons it lit only under the name, so a row had a dead strip over its own
/// icon and its indent, and each part carried a margin the row's height moved
/// with.
fn row(ui: &mut egui::Ui, r: &Row<'_>) -> Hit {
    let mut hit = Hit {
        picked: false,
        folded: false,
        folded_hovered: false,
    };
    let step = r.row_h;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), step), Sense::click());
    let chosen = r.item == r.chosen;
    let radius = egui::CornerRadius::same((step * 0.2) as u8);
    // Reserved, painted once the caret has had its say: the caret is a hit of
    // its own, and a row that went dark under it would blink as it was aimed at.
    let plate = ui.painter().add(egui::Shape::Noop);
    let mut at = rect.min.x;
    if r.depth > 0 {
        guides(ui, rect.min, step, r.trail);
        at += step * r.depth as f32;
    }
    if r.parent {
        // Interacted after the row, so a click on the caret is the caret's:
        // egui gives a tie to the widget registered last.
        let box_ = Rect::from_min_size(pos2(at, rect.min.y), egui::Vec2::splat(step));
        let caret = ui.interact(box_, ui.id().with(("caret", r.slot)), Sense::click());
        hit.folded = caret.clicked();
        hit.folded_hovered = caret.hovered();
        ui.painter().text(
            box_.center(),
            egui::Align2::CENTER_CENTER,
            if r.shut { "\u{25b8}" } else { "\u{25be}" },
            r.font.clone(),
            r.color,
        );
    }
    if r.parent || r.indent {
        at += step;
    }
    let over = response.hovered() || hit.folded_hovered;
    let held = response.is_pointer_button_down_on();
    let lit = if chosen {
        Some(ui.visuals().selection.bg_fill)
    } else if over {
        Some(crate::immediate::wash(ui, held))
    } else {
        None
    };
    if let Some(fill) = lit {
        ui.painter()
            .set(plate, egui::epaint::RectShape::filled(rect, radius, fill));
    }
    // A picked row still answers the pointer: the wash goes over the fill that
    // says it is picked rather than instead of it.
    if chosen && over {
        ui.painter()
            .rect_filled(rect, radius, crate::immediate::wash(ui, held));
    }
    let (icon, label, trailing, tint) = fields(r.item);
    let ink = if chosen {
        ui.visuals().selection.stroke.color
    } else {
        tint.unwrap_or(r.color)
    };
    if !icon.is_empty() {
        // The icon field is a glyph from the project's icon face, not a
        // character in the UI one.
        let mark = egui::FontId::new(r.font.size, crate::theme::family("icon"));
        let drawn = ui.painter().text(
            pos2(at, rect.center().y),
            egui::Align2::LEFT_CENTER,
            icon,
            mark,
            ink,
        );
        at += drawn.width() + step * 0.25;
    }
    ui.painter().text(
        pos2(at, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        r.font.clone(),
        ink,
    );
    if !trailing.is_empty() {
        ui.painter().text(
            pos2(rect.max.x - step * 0.3, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            trailing,
            r.font.clone(),
            ink,
        );
    }
    hit.picked = response.clicked();
    hit
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
        let edge = (size.y * 0.6).min(size.x * 0.6).max(1.0);
        let face = egui::Rect::from_center_size(
            pos2(rect.center().x, head + edge / 2.0),
            egui::Vec2::splat(edge),
        );
        ui.painter().image(sheet.id(), face, uv, Color32::WHITE);
        head += edge + 4.0;
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
    let items: Vec<String> = widget
        .options
        .iter()
        .map(smol_str::SmolStr::to_string)
        .collect();
    let chosen = widget.text.to_string();
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
    let items: Vec<String> = widget
        .options
        .iter()
        .map(smol_str::SmolStr::to_string)
        .collect();
    let chosen = widget.text.to_string();
    let row_h = if widget.row_height > 0.0 {
        widget.row_height * at.scale
    } else {
        ui.text_style_height(&egui::TextStyle::Body).max(1.0)
    };
    let mut picked = None;
    steady_height(ui);
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

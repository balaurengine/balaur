//! The `table` kind: named columns a drag resizes, over the rows the box
//! shows and no others.

use egui::{Align2, Color32, Rect, Sense, Stroke, pos2, vec2};

use crate::vocabulary::words as w;
use crate::widget::arrange::solved_of;
use crate::widget::layer::{Edit, Painting};
use crate::widget::node::Widget;
use crate::widget::rows::{
    Clicked, Ink, SEP, after_click, backdrop, landed, picked_set, shows, strings_of,
};

/// The narrowest a column may be dragged, in design pixels.
const NARROWEST: f32 = 32.0;
/// How wide a grab the seam between two columns gets where the widget states
/// no `handle` of its own.
const SEAM: f32 = 6.0;
/// The air either side of a cell's text, where the theme states none.
const PAD: f32 = 6.0;
/// A header name ending in this is a column drawn against its right edge,
/// which is what a column of numbers wants.
const RIGHT: char = '>';
/// What the column the rows are sorted by wears, one way and the other.
const CARET_DOWN: char = '\u{25be}';
const CARET_UP: char = '\u{25b4}';

/// Godot's `Tree` with named columns: `options` holds the rows, each split on
/// U+001F into one cell a column, and `text` is the row picked.
///
/// `titles` names the columns; a table that names none takes its first row
/// as the names. Only the rows the box shows are built, as a `list`'s are, so
/// a table of a log costs its viewport.
pub(crate) fn table(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    font: &egui::FontId,
    color: Color32,
) {
    let placed = &at.arena[index];
    let (entity, widget) = (placed.entity, placed.widget.clone());
    let want = solved_of(&widget, &at.style_of(&widget), at.assigned);
    let items = strings_of(&widget);
    let (heads, first) = header(&widget, &items);
    // The order the rows are drawn in, which is the order given unless a
    // column was named to sort by, or the whole of it was turned round.
    let walk = ordered(&widget, &items, &heads, first);
    let row_h = row_pitch(ui, &widget, font);
    let id = egui::Id::new(("balaur-table", entity));
    let style = at.style_of(&widget);
    let look = Look {
        row_h,
        font,
        color,
        ink: &Ink::of(ui, &at.theme),
        pad: style.padding_x.unwrap_or(PAD).max(0.0),
        // Where every cell's text sits, which one column overrides by ending
        // its name in `>`.
        align: widget.text_align.clone(),
    };
    backdrop(ui, at, &style, ui.max_rect());
    let mut widths = shares(&widget, heads.len());
    // The pitch is the row, with nothing between: the scroll places by the
    // one and the rows draw at the other, and a gap makes them two numbers.
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut sorted = None;
    if widget.header {
        let asked = head_row(ui, &head_of(&widget, &heads, &look, want.x), &mut widths);
        if asked.moved {
            at.edits.push((entity, Edit::Widths(widths.clone())));
        }
        sorted = asked.sorted;
    }
    if let Some((column, reverse)) = sorted {
        at.edits.push((entity, Edit::Sorted(column, reverse)));
    }
    let picked = picked_set(&widget);
    let mut hit = None;
    let mut aimed = None;
    let mut area = egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false]);
    if want.x > 0.0 {
        area = area.max_width(want.x);
    }
    if want.y > 0.0 {
        area = area.max_height((want.y - row_h).max(row_h));
    }
    area.show_rows(ui, row_h, walk.len(), |ui, range| {
        for slot in range {
            let Some(&row) = walk.get(slot) else {
                continue;
            };
            let on = items.get(row).is_some_and(|item| picked.contains(item));
            let cell = Cells {
                item: &items[row],
                slot,
                widths: &widths,
                heads: &heads,
                on,
                look: &look,
            };
            let struck = cells(ui, &cell);
            if struck.0 {
                hit = Some(row);
            }
            if struck.1 {
                aimed = Some(row);
            }
        }
    });
    // A row already picked keeps the set it is in, so a menu opened over one
    // of several picked rows is opened over all of them.
    let aimed = aimed.filter(|&row| !picked.contains(&items[row]));
    let held = ui.input(|i| i.modifiers);
    if let Some((hit, mods, click)) = landed(hit, aimed, held) {
        let next = after_click(
            &Clicked {
                items: &items,
                walk: &walk,
                picked: &picked,
                hit,
                anchor: widget.text.as_str(),
                multi: widget.multi,
            },
            mods,
        );
        if click {
            at.clicked.push(entity);
        }
        at.edits
            .push((entity, Edit::Picked(items[hit].clone(), next)));
    }
}

/// The pitch of a row: what the table asked for, never less than the line of
/// text it has to hold.
fn row_pitch(ui: &mut egui::Ui, widget: &Widget, font: &egui::FontId) -> f32 {
    let text_h = ui.fonts_mut(|f| f.row_height(font));
    if widget.row_height > 0.0 {
        widget.row_height.max(text_h)
    } else {
        text_h
    }
}

/// The header strip as `head_row` wants it: the names, the face they are
/// drawn with, and how wide a seam answers to the pointer.
fn head_of<'a>(widget: &'a Widget, heads: &'a [String], look: &'a Look<'a>, room: f32) -> Head<'a> {
    Head {
        heads,
        look,
        room,
        grab: if widget.handle > 0.0 {
            widget.handle
        } else {
            SEAM
        },
        sort: widget.sort.as_str(),
        reverse: widget.reverse,
        sortable: widget.sortable,
    }
}

/// How a table draws: the pitch of a row, the face, the ink, and the colours
/// its plates and rules are painted with.
struct Look<'a> {
    row_h: f32,
    font: &'a egui::FontId,
    color: Color32,
    ink: &'a Ink,
    /// The air either side of a cell's text, which the theme spells
    /// `padding_x` as it does for a caption.
    pad: f32,
    /// Where a cell's text sits across its column: the widget's own
    /// `text_align`, which a column overrides for itself.
    align: smol_str::SmolStr,
}

/// The column names, and which row the body starts at.
///
/// A table that states none takes its first row as the header, which is what
/// a table read out of a log or a spreadsheet has. One drawing no header
/// keeps that row: the names are still the column count, and nothing is spent
/// on a strip nobody sees.
fn header(widget: &Widget, items: &[String]) -> (Vec<String>, usize) {
    let named: Vec<String> = widget
        .titles
        .iter()
        .filter(|title| !title.is_empty())
        .map(ToString::to_string)
        .collect();
    if !named.is_empty() {
        return (named, 0);
    }
    match items.first() {
        Some(first) => {
            let heads = first.split(SEP).map(ToString::to_string).collect();
            (heads, usize::from(widget.header))
        }
        None => (vec![String::new()], 0),
    }
}

/// Each column's share of the width: the ones the widget carries, and an even
/// split where it carries none or the column count has changed under them.
///
/// Normalised, so `["2", "1", "1"]` is a half and two quarters and a drag
/// that took from one column and gave to its neighbour still adds to one.
fn shares(widget: &Widget, columns: usize) -> Vec<f32> {
    let columns = columns.max(1);
    let stated = &widget.widths;
    let total: f32 = stated.iter().sum();
    if stated.len() != columns || total <= 0.0 {
        return vec![1.0 / columns as f32; columns];
    }
    stated.iter().map(|share| share / total).collect()
}

/// The rows to draw and their order: every row past the header, sorted by the
/// column the widget names and turned round where it asks.
///
/// A cell that starts with a number sorts as one, so `12 KB` follows `3 KB`
/// rather than leading it; anything else sorts as text, case folded.
fn ordered(widget: &Widget, items: &[String], heads: &[String], first: usize) -> Vec<usize> {
    let mut walk: Vec<usize> = (first..items.len()).collect();
    let column = heads
        .iter()
        .position(|head| name(head) == widget.sort)
        .filter(|_| !widget.sort.is_empty());
    if let Some(column) = column {
        let cell = |row: usize| -> String {
            items[row]
                .split(SEP)
                .nth(column)
                .unwrap_or_default()
                .to_owned()
        };
        walk.sort_by(|a, b| compare(&cell(*a), &cell(*b)));
    }
    if widget.reverse {
        walk.reverse();
    }
    walk
}

/// Two cells in order: as numbers where both begin with one, else as text.
fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    match (leading(a), leading(b)) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    }
}

/// The number a cell starts with, past any space, or `None` where it starts
/// with something else.
fn leading(cell: &str) -> Option<f64> {
    let text = cell.trim_start();
    let end = text
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != '+')
        .unwrap_or(text.len());
    text[..end].parse().ok()
}

/// A header as it is drawn: the names, the face, the column the rows are
/// sorted by and which way, and whether a click sorts at all.
struct Head<'a> {
    heads: &'a [String],
    look: &'a Look<'a>,
    /// The width the table was given, or 0 where it takes what it is in.
    room: f32,
    /// How wide a grab the seams between the columns get, which the widget
    /// spells `handle` as a container does.
    grab: f32,
    sort: &'a str,
    reverse: bool,
    sortable: bool,
}

/// What a pass over the header asked for: a seam moved, and a column clicked
/// to sort by with the direction it should take.
struct Asked {
    moved: bool,
    sorted: Option<(String, bool)>,
}

/// The header row: a name a column, a grab between each pair of them, and the
/// caret on the column the rows are in the order of.
fn head_row(ui: &mut egui::Ui, head: &Head<'_>, widths: &mut [f32]) -> Asked {
    let (heads, look) = (head.heads, head.look);
    let width = if head.room > 0.0 {
        head.room
    } else {
        ui.available_width()
    };
    let (rect, _) = ui.allocate_exact_size(vec2(width, look.row_h), Sense::hover());
    if shows(look.ink.head) {
        ui.painter().rect_filled(rect, 0.0, look.ink.head);
    }
    let ruled = shows(look.ink.rule);
    let line = Stroke::new(1.0, look.ink.rule);
    let mut asked = Asked {
        moved: false,
        sorted: None,
    };
    let mut x = rect.min.x;
    for (n, title) in heads.iter().enumerate() {
        let width = widths[n] * rect.width();
        let cell = Rect::from_min_size(pos2(x, rect.min.y), vec2(width, look.row_h));
        let named = name(title);
        let on = head.sort == named && !named.is_empty();
        // Up for the way a column reads from the top: A before Z and small
        // before large, which is how every file list draws it.
        let text = if on {
            let way = if head.reverse { CARET_DOWN } else { CARET_UP };
            format!("{named} {way}")
        } else {
            named.to_owned()
        };
        let head_cell = Cell {
            text: &text,
            right: right(title),
            look,
        };
        text_in(ui, cell, &head_cell, look.color);
        if head.sortable && !named.is_empty() {
            let name_hit = ui.interact(cell, ui.id().with(("head", n)), Sense::click());
            if name_hit.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if name_hit.clicked() {
                // The same column again is the other way round; another one
                // starts at the top of its own order.
                asked.sorted = Some((named.to_owned(), on && !head.reverse));
            }
        }
        x += width;
        if n + 1 >= heads.len() {
            continue;
        }
        let grab = Rect::from_center_size(pos2(x, rect.center().y), vec2(head.grab, look.row_h));
        let seam = ui.interact(grab, ui.id().with(("seam", n)), Sense::drag());
        if seam.hovered() || seam.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        let dx = seam.drag_delta().x;
        if dx != 0.0 {
            drag_seam(widths, n, dx / rect.width(), rect.width());
            asked.moved = true;
        }
        if ruled {
            ui.painter()
                .line_segment([pos2(x, rect.min.y), pos2(x, rect.max.y)], line);
        }
    }
    if ruled {
        ui.painter()
            .line_segment([rect.left_bottom(), rect.right_bottom()], line);
    }
    asked
}

/// Move one seam, taking from the column on the far side of it so the row
/// still fills its width and no column goes under `NARROWEST`.
fn drag_seam(widths: &mut [f32], n: usize, dx: f32, room: f32) {
    let floor = (NARROWEST / room.max(1.0)).min(1.0 / widths.len() as f32);
    let dx = dx.clamp(floor - widths[n], widths[n + 1] - floor);
    widths[n] += dx;
    widths[n + 1] -= dx;
}

/// One body row: its plate, and a cell a column.
struct Cells<'a> {
    item: &'a str,
    /// Which row this is on screen, for the stripe.
    slot: usize,
    widths: &'a [f32],
    heads: &'a [String],
    on: bool,
    look: &'a Look<'a>,
}

/// Draw one row across the columns, and answer whether it was clicked, and
/// whether a secondary click aimed at it.
fn cells(ui: &mut egui::Ui, c: &Cells<'_>) -> (bool, bool) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), c.look.row_h), Sense::click());
    let radius = egui::CornerRadius::same((c.look.row_h * 0.2) as u8);
    if c.slot % 2 == 1 && shows(c.look.ink.stripe) {
        ui.painter().rect_filled(rect, 0.0, c.look.ink.stripe);
    }
    if c.on {
        ui.painter().rect_filled(rect, radius, c.look.ink.on);
    }
    // A picked row still answers the pointer: the wash goes over the fill
    // that says it is picked rather than instead of it.
    if response.hovered() {
        let held = response.is_pointer_button_down_on();
        ui.painter()
            .rect_filled(rect, radius, c.look.ink.wash(held));
    }
    let (cells, tint) = split(c.item, c.heads.len());
    let ink = if c.on {
        c.look.ink.on_color
    } else {
        tint.unwrap_or(c.look.color)
    };
    let mut x = rect.min.x;
    for (n, head) in c.heads.iter().enumerate() {
        let width = c.widths[n] * rect.width();
        let cell = Rect::from_min_size(pos2(x, rect.min.y), vec2(width, c.look.row_h));
        if let Some(text) = cells.get(n) {
            let one = Cell {
                text,
                right: right(head),
                look: c.look,
            };
            text_in(ui, cell, &one, ink);
        }
        x += width;
    }
    (response.clicked(), response.secondary_clicked())
}

/// A row's cells, and the `#rrggbb` it may carry past its last one, the way a
/// `list` row carries its own colour.
fn split(item: &str, columns: usize) -> (Vec<&str>, Option<Color32>) {
    let mut parts: Vec<&str> = item.split(SEP).collect();
    let tint = (parts.len() > columns)
        .then(|| parts.last().and_then(|last| crate::theme::parse_hex(last)))
        .flatten();
    if tint.is_some() {
        parts.pop();
    }
    (parts, tint)
}

/// One cell's text: what it says, whether its own column pins it to the right
/// edge, and the table's own face, air and alignment.
struct Cell<'a> {
    text: &'a str,
    right: bool,
    look: &'a Look<'a>,
}

/// Where a cell's text is drawn from: its column's own `>`, else the widget's
/// `text_align`, else the left edge as a row of names reads.
fn anchored(cell: &Cell<'_>, box_: Rect, inner: Rect) -> (egui::Pos2, Align2) {
    let end = cell.right || cell.look.align == w::END;
    if end {
        return (pos2(inner.max.x, box_.center().y), Align2::RIGHT_CENTER);
    }
    if cell.look.align == w::CENTER {
        return (box_.center(), Align2::CENTER_CENTER);
    }
    (pos2(inner.min.x, box_.center().y), Align2::LEFT_CENTER)
}

/// One cell's text, clipped to its own column so a long value cannot run into
/// the column beside it.
fn text_in(ui: &egui::Ui, cell: Rect, one: &Cell<'_>, color: Color32) {
    let inner = cell.shrink2(vec2(one.look.pad, 0.0));
    if inner.width() <= 0.0 || one.text.is_empty() {
        return;
    }
    let (at, align) = anchored(one, cell, inner);
    ui.painter()
        .with_clip_rect(inner)
        .text(at, align, one.text, one.look.font.clone(), color);
}

/// Whether a column is drawn against its right edge, which its name says by
/// ending in `>`.
fn right(head: &str) -> bool {
    head.ends_with(RIGHT)
}

/// A column's name without the mark that aligned it.
fn name(head: &str) -> &str {
    head.trim_end_matches(RIGHT)
}

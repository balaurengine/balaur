//! The row kinds: `list`, `tree` and `table`, their card layout, and the
//! single row they all build on screen.

use std::collections::BTreeSet;

use egui::{Color32, Rect, Sense, Stroke, pos2, vec2};

use crate::vocabulary::keys as k;
use crate::vocabulary::words as w;
use crate::widget::arrange::solved_of;
use crate::widget::layer::{Edit, Painting};
use crate::widget::node::Widget;
use crate::widget::theme::{Style, WidgetTheme};

/// What a row view paints its parts with, past the ink and the face every
/// widget has: the theme's `[colors]` where it names them, and the built-in
/// look where it does not. A colour with no alpha draws nothing, which is how
/// a table loses its rules or its striping.
pub(super) struct Ink {
    /// A picked row's plate, and the ink on it.
    pub(super) on: Color32,
    pub(super) on_color: Color32,
    /// What a row takes under the pointer, and while it is held.
    hover: Color32,
    active: Color32,
    /// A `table`'s every other row, its header's plate, and the lines down
    /// its columns.
    pub(super) stripe: Color32,
    pub(super) head: Color32,
    pub(super) rule: Color32,
    /// The lines down a `tree`'s indent.
    pub(super) guide: Color32,
}

impl Ink {
    pub(super) fn of(ui: &egui::Ui, theme: &WidgetTheme) -> Self {
        let named = |key: &str, built_in: Color32| theme.token(key).unwrap_or(built_in);
        let weak = ui.visuals().weak_text_color();
        Self {
            on: named(k::ROW_SELECTED, ui.visuals().selection.bg_fill),
            on_color: named(k::ROW_SELECTED_TEXT, ui.visuals().selection.stroke.color),
            hover: named(k::ROW_HOVER, crate::immediate::wash(ui, false)),
            active: named(k::ROW_ACTIVE, crate::immediate::wash(ui, true)),
            stripe: named(k::ROW_STRIPE, ui.visuals().faint_bg_color),
            head: named(k::HEADER_FILL, ui.visuals().faint_bg_color),
            rule: named(k::COLUMN_RULE, weak.gamma_multiply(0.5)),
            guide: named(k::ROW_GUIDE, weak.gamma_multiply(0.55)),
        }
    }

    /// The wash over a row the pointer is on, held or not.
    pub(super) fn wash(&self, held: bool) -> Color32 {
        if held { self.active } else { self.hover }
    }
}

/// Whether a colour paints anything at all, so a part a theme hid costs no
/// shape rather than an invisible one.
pub(super) fn shows(color: Color32) -> bool {
    color.a() > 0
}

/// The frame a theme gives a row view: the picture it names, else its fill
/// and its outline. Nothing where it names neither, which is the look these
/// kinds have always had.
pub(super) fn backdrop(ui: &egui::Ui, at: &Painting<'_>, style: &Style, rect: Rect) {
    if let Some(path) = style.image.as_ref() {
        let plate = ui.painter().add(egui::Shape::Noop);
        crate::widget::kinds::nine_patch_plate(ui, at.eng, plate, path, style.slice, rect);
        return;
    }
    if style.fill.is_none() && style.stroke.is_none() {
        return;
    }
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(style.radius.unwrap_or(0.0) as u8),
        style.fill.unwrap_or(Color32::TRANSPARENT),
        style
            .stroke
            .map_or(Stroke::NONE, |c| Stroke::new(style.stroke_px(), c)),
        egui::StrokeKind::Inside,
    );
}

/// What a row splits on: icon, label, a trailing note and an `#rrggbb` of its
/// own for a `list` or a `tree`, one cell a column for a `table`.
pub(super) const SEP: char = '\u{1f}';

/// The rows a widget holds picked: the set it carries when it holds many, and
/// `text` alone when it holds one.
pub(super) fn picked_set(widget: &Widget) -> BTreeSet<String> {
    if !widget.selection.is_empty() {
        return widget.selection.iter().map(ToString::to_string).collect();
    }
    // `text` alone where nothing has been picked yet: a scene that states the
    // row it opens on writes the one it knows, as a `dropdown` does.
    std::iter::once(widget.text.to_string())
        .filter(|row| !row.is_empty())
        .collect()
}

/// What a click on `items[hit]` leaves picked, in the order the rows are
/// written down. Empty where the widget holds one row, which `text` is.
///
/// The command key toggles the row it hits and shift takes the run between
/// the last row clicked and this one, along `walk` rather than along `items`:
/// a range over a tree is what the fold left on screen.
pub(super) fn after_click(at: &Clicked<'_>, mods: egui::Modifiers) -> Vec<String> {
    if !at.multi {
        return vec![at.items[at.hit].clone()];
    }
    let mut next = at.picked.clone();
    let row = at.items[at.hit].clone();
    let span = mods
        .shift
        .then(|| run(at.items, at.walk, at.hit, at.anchor))
        .flatten();
    if let Some(span) = span {
        next.extend(span);
    } else if mods.command {
        if !next.remove(&row) {
            next.insert(row);
        }
    } else {
        next.clear();
        next.insert(row);
    }
    at.items
        .iter()
        .filter(|item| next.contains(item.as_str()))
        .cloned()
        .collect()
}

/// A click and what it lands on: every row, the ones the fold left on screen,
/// what is picked now, the row hit and the one picked before it.
pub(super) struct Clicked<'a> {
    pub(super) items: &'a [String],
    pub(super) walk: &'a [usize],
    pub(super) picked: &'a BTreeSet<String>,
    pub(super) hit: usize,
    pub(super) anchor: &'a str,
    pub(super) multi: bool,
}

/// The rows between the last one clicked and this one, along the open walk.
/// `None` where the walk holds neither, which is a shift click with nothing
/// to measure from.
fn run(items: &[String], walk: &[usize], hit: usize, anchor: &str) -> Option<Vec<String>> {
    let from = walk.iter().position(|&i| items[i] == anchor)?;
    let to = walk.iter().position(|&i| i == hit)?;
    let (first, last) = (from.min(to), from.max(to));
    Some(
        walk[first..=last]
            .iter()
            .map(|&i| items[i].clone())
            .collect(),
    )
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
    let want = solved_of(widget, &at.style_of(widget), at.assigned);
    let row_h = pitch(ui, widget, font);
    let ink = Ink::of(ui, &at.theme);
    backdrop(ui, at, &at.style_of(widget), ui.max_rect());
    // The pitch is the row, with nothing between: `show_rows` places by the
    // one and the rows draw at the other, and a gap makes them two numbers.
    ui.spacing_mut().item_spacing.y = 0.0;
    let items = strings_of(widget);
    let picked_rows = picked_set(widget);
    let (anchor, multi) = (widget.text.to_string(), widget.multi);
    let id = egui::Id::new(("balaur-list", entity));
    let depth_of = |item: &String| depth(item, indent);
    // Folded branches are the tree's own business, so a script hands over the
    // whole outline and never hears about a caret.
    let shut: BTreeSet<String> = ui.data(|d| d.get_temp(id).unwrap_or_default());
    let open_rows = open_walk(&items, &shut, &depth_of);
    // Where each open row's branch still continues, so a guide is drawn only
    // down a level that has another row below this one.
    let trails = branches(&items, &open_rows, &depth_of);
    let mut picked = None;
    let mut aimed = None;
    let mut toggled = None;
    // The row a drag has hold of, and where the pass found it would land.
    let held_row: Dragging = ui.data(|d| d.get_temp(id.with("drag")).unwrap_or_default());
    let mut landing = None;
    let mut took = None;
    // Both ways: a row wider than the list is a log line or a long node
    // name, and a bar to reach the end of it is better than the end being
    // painted over whatever sits beside the list.
    let mut area = egui::ScrollArea::both()
        .id_salt(id)
        .auto_shrink([false, false]);
    // The box the layout gave it, both ways: a width stated and not applied
    // is a list that draws past whatever sits beside it.
    if want.x > 0.0 {
        area = area.max_width(want.x);
    }
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
                    chosen: picked_rows.contains(item),
                    font,
                    color,
                    ink: &ink,
                    drag: widget.reorderable.then_some(held_row.row.as_str()),
                },
            );
            if hit.folded {
                toggled = Some(item.clone());
            }
            if hit.picked {
                picked = Some(i);
            }
            if hit.aimed {
                aimed = Some(i);
            }
            if let Some(took_row) = hit.took {
                took = Some(took_row);
            }
            if let Some(side) = hit.landing {
                landing = Some((i, side));
            }
        }
    });
    if widget.reorderable {
        let drag = Drag {
            id,
            entity,
            items: &items,
            held: &held_row,
            took,
            landing,
        };
        settle_drag(ui, at, &drag);
    }
    settle_folds(ui, id, shut, toggled);
    // A row already picked keeps the set it is in: aiming at one of several
    // picked rows is how a menu is opened over all of them.
    let aimed = aimed.filter(|&row| !picked_rows.contains(&items[row]));
    settle_pick(
        at,
        entity,
        &Clicked {
            items: &items,
            walk: &open_rows,
            picked: &picked_rows,
            hit: 0,
            anchor: &anchor,
            multi,
        },
        landed(picked, aimed, ui.input(|i| i.modifiers)),
    );
}

/// What a click on a row left picked, written down for the next tick.
fn settle_pick(
    at: &mut Painting<'_>,
    entity: balaur_core::hecs::Entity,
    at_row: &Clicked<'_>,
    landed: Option<(usize, egui::Modifiers, bool)>,
) {
    let Some((hit, mods, click)) = landed else {
        return;
    };
    let next = after_click(&Clicked { hit, ..*at_row }, mods);
    if click {
        at.clicked.push(entity);
    }
    at.edits
        .push((entity, Edit::Picked(at_row.items[hit].clone(), next)));
}

/// The pitch a row is placed at, which has to hold its own text: the two
/// drifting apart is a list that scrolls off its own bar.
///
/// `row_height` states it and the font's own line decides it otherwise.
/// `height` is the box, as it is on every other kind.
fn pitch(ui: &mut egui::Ui, widget: &Widget, font: &egui::FontId) -> f32 {
    let stated = if widget.row_height > 0.0 {
        widget.row_height
    } else {
        ui.text_style_height(&egui::TextStyle::Body).max(1.0)
    };
    stated.max(ui.fonts_mut(|f| f.row_height(font)))
}

/// How deep a row sits: a tab it starts with is one level in, which is how an
/// outline is written down and what keeps a tree inside a list of strings.
fn depth(item: &str, indent: bool) -> usize {
    if indent {
        item.len() - item.trim_start_matches('\t').len()
    } else {
        0
    }
}

/// The rows a fold left on screen, as indices into `items`: everything under
/// a shut row is skipped until the walk comes back up to its level.
fn open_walk(
    items: &[String],
    shut: &BTreeSet<String>,
    depth_of: &impl Fn(&String) -> usize,
) -> Vec<usize> {
    let mut open = Vec::new();
    let mut hidden_under: Option<usize> = None;
    for (i, item) in items.iter().enumerate() {
        let depth = depth_of(item);
        if hidden_under.is_some_and(|under| depth > under) {
            continue;
        }
        hidden_under = None;
        open.push(i);
        if shut.contains(item) {
            hidden_under = Some(depth);
        }
    }
    open
}

/// The rows a view holds, as the strings its parts are split out of.
pub(super) fn strings_of(widget: &Widget) -> Vec<String> {
    widget
        .options
        .iter()
        .map(smol_str::SmolStr::to_string)
        .collect()
}

/// A fold a caret was clicked on, written down under the widget's own id: a
/// script hands over the whole outline and never hears about a caret.
fn settle_folds(ui: &egui::Ui, id: egui::Id, shut: BTreeSet<String>, toggled: Option<String>) {
    let Some(item) = toggled else {
        return;
    };
    let mut next = shut;
    if !next.remove(&item) {
        next.insert(item);
    }
    ui.data_mut(|d| d.insert_temp(id, next));
}

/// Where a dragged row would land: on the row it is over, or in the gap
/// above or below it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Before,
    Into,
    After,
}

impl Side {
    /// The word the drop reports, which is the one a script matches on.
    fn word(self) -> &'static str {
        match self {
            Self::Before => w::BEFORE,
            Self::Into => w::INTO,
            Self::After => w::AFTER,
        }
    }

    /// Which part of a row the pointer is over: its top eighth is the gap
    /// above, its bottom eighth the gap below, and the rest of it is the row
    /// itself. A list that cannot nest has no middle.
    fn under(rect: Rect, y: f32, nests: bool) -> Self {
        let edge = if nests {
            rect.height() / 4.0
        } else {
            rect.height() / 2.0
        };
        if y < rect.top() + edge {
            Self::Before
        } else if y > rect.bottom() - edge {
            Self::After
        } else if nests {
            Self::Into
        } else {
            Self::After
        }
    }
}

/// The row a drag picked up, kept between passes under the widget's own id.
#[derive(Clone, Default)]
struct Dragging {
    row: String,
}

/// One row of a list or tree.
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per thing a row draws: its caret, its indent, its fold and its pick"
)]
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
    /// Whether this row is one of the picked ones.
    chosen: bool,
    font: &'a egui::FontId,
    color: Color32,
    ink: &'a Ink,
    /// The row a drag has hold of, where this view reorders at all: `Some("")`
    /// is a view that reorders with nothing picked up yet.
    drag: Option<&'a str>,
}

/// What a click on a row meant. One flag a button and a part, which is what
/// the caller answers for separately.
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per thing a press can mean: a pick, an aim, a fold, a caret"
)]
struct Hit {
    picked: bool,
    /// The row a press picked up to drag, and where the row in hand would
    /// land if it were let go over this one.
    took: Option<String>,
    landing: Option<Side>,
    /// Whether a secondary click landed on the row, which picks it without
    /// counting as a click: a `context` menu acts on what it opened over.
    aimed: bool,
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
        took: None,
        landing: None,
        aimed: false,
        folded: false,
        folded_hovered: false,
    };
    let step = r.row_h;
    // A view that reorders senses a drag as well as a click; one that does
    // not keeps the click alone, so a drag over it still scrolls the list.
    let sense = if r.drag.is_some() {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), step), sense);
    let chosen = r.chosen;
    let radius = egui::CornerRadius::same((step * 0.2) as u8);
    // Reserved, painted once the caret has had its say: the caret is a hit of
    // its own, and a row that went dark under it would blink as it was aimed at.
    let plate = ui.painter().add(egui::Shape::Noop);
    let mut at = rect.min.x;
    if r.depth > 0 {
        guides(ui, rect.min, step, r.trail, r.ink.guide);
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
        Some(r.ink.on)
    } else if over {
        Some(r.ink.wash(held))
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
        ui.painter().rect_filled(rect, radius, r.ink.wash(held));
    }
    let (icon, label, trailing, tint) = fields(r.item);
    let ink = if chosen {
        r.ink.on_color
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
    hit.aimed = response.secondary_clicked();
    if r.drag.is_some() {
        let (took, landing) = dragged(ui, r, rect, &response);
        hit.took = took;
        hit.landing = landing;
    }
    hit
}

/// What a drag over this row means: the row it picked up, and where the row
/// already in hand would land if it were let go here.
fn dragged(
    ui: &egui::Ui,
    r: &Row<'_>,
    rect: Rect,
    response: &egui::Response,
) -> (Option<String>, Option<Side>) {
    // A press that moved is a drag: the row it started on is the one in
    // hand, and the row under the pointer is where it would land.
    let took = response.drag_started().then(|| r.item.to_owned());
    let held = r.drag.unwrap_or_default();
    if held.is_empty() || held == r.item || !response.contains_pointer() {
        return (took, None);
    }
    let pointer = ui.ctx().pointer_interact_pos().unwrap_or(rect.center());
    let side = Side::under(rect, pointer.y, r.indent);
    mark_landing(ui, rect, side, r.ink.on);
    (took, Some(side))
}

/// Where a dragged row would land: a line across the gap it would fall into,
/// or a frame around the row it would go inside.
fn mark_landing(ui: &egui::Ui, rect: Rect, side: Side, ink: Color32) {
    let stroke = Stroke::new(2.0, ink);
    match side {
        Side::Into => {
            ui.painter().rect_stroke(
                rect,
                egui::CornerRadius::same(3),
                stroke,
                egui::StrokeKind::Inside,
            );
        }
        Side::Before => {
            ui.painter()
                .line_segment([rect.left_top(), rect.right_top()], stroke);
        }
        Side::After => {
            ui.painter()
                .line_segment([rect.left_bottom(), rect.right_bottom()], stroke);
        }
    }
}

/// A drag as one pass saw it: the view it is in, the rows it is over, what is
/// in hand, what was picked up this pass and where it would land.
struct Drag<'a> {
    id: egui::Id,
    entity: balaur_core::hecs::Entity,
    items: &'a [String],
    held: &'a Dragging,
    took: Option<String>,
    landing: Option<(usize, Side)>,
}

/// Remember what a drag picked up, and report where it was let go.
///
/// The kind moves nothing itself: a row view holds strings, and what they
/// stand for is the script's. `on_move` says what was dropped where, and the
/// rows come back in the order the script decided.
fn settle_drag(ui: &egui::Ui, at: &mut Painting<'_>, drag: &Drag<'_>) {
    let key = drag.id.with("drag");
    if let Some(row) = drag.took.clone() {
        ui.data_mut(|d| d.insert_temp(key, Dragging { row }));
        return;
    }
    if drag.held.row.is_empty() {
        return;
    }
    // A drag ends when the button comes up, wherever the pointer is: let go
    // over nothing and the row stays where it was.
    if ui.ctx().input(|i| i.pointer.any_down()) {
        return;
    }
    ui.data_mut(|d| d.insert_temp(key, Dragging::default()));
    if let Some((row, side)) = drag.landing {
        at.edits.push((
            drag.entity,
            Edit::Dropped(
                drag.held.row.clone(),
                drag.items[row].clone(),
                side.word().to_owned(),
            ),
        ));
    }
}

/// Which row an edit reports, read with which modifiers, and whether it is a
/// click at all.
///
/// A secondary click picks the row it lands on so a `context` menu acts on
/// what it opened over. It takes the row alone whatever is held down, and it
/// is not a click: the widget's own `on_click` stays quiet, which is what the
/// menu's own rows are for.
pub(super) fn landed(
    clicked: Option<usize>,
    aimed: Option<usize>,
    mods: egui::Modifiers,
) -> Option<(usize, egui::Modifiers, bool)> {
    match (clicked, aimed) {
        (Some(hit), _) => Some((hit, mods, true)),
        (None, Some(hit)) => Some((hit, egui::Modifiers::NONE, false)),
        (None, None) => None,
    }
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
fn guides(ui: &egui::Ui, head: egui::Pos2, step: f32, trail: &[bool], ink: Color32) {
    let Some(own) = trail.len().checked_sub(1) else {
        return;
    };
    if !shows(ink) {
        return;
    }
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
/// tabs are the tree's depth and are not a field. A fifth field is the row's
/// key, never drawn: the widget knows a row by its string, so it is what
/// keeps two rows with the same label two rows.
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

/// One card and how it is drawn: the box the caller sized, the face, the
/// palette, whether it is picked, and the sheet its face is cut from.
struct Card<'a> {
    size: egui::Vec2,
    font: &'a egui::FontId,
    color: Color32,
    ink: &'a Ink,
    on: bool,
    /// The sheet and the pixels its regions are counted in.
    sheet: Option<(&'a egui::TextureHandle, egui::Vec2)>,
}

/// One card: the icon over the label, in a box the caller sized. Answers
/// whether it was clicked, and whether a secondary click aimed at it.
///
/// The same U+001F fields a row splits on, so a view moves between the two
/// modes by setting `columns` and changing nothing else.
fn card(ui: &mut egui::Ui, item: &str, c: &Card<'_>) -> (bool, bool) {
    let (size, font) = (c.size, c.font);
    let (icon, label, trailing, tint) = fields(item);
    let color = tint.unwrap_or(c.color);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if c.on {
        c.ink.on
    } else if response.hovered() {
        c.ink.wash(response.is_pointer_button_down_on())
    } else {
        Color32::TRANSPARENT
    };
    let sheet = c.sheet;
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(5), fill);
    let mut head = rect.top() + 6.0;
    // A list that names a `source` reads the icon field as `x,y,w,h` in that
    // picture's own pixels, which is how an atlas picker shows its tiles.
    let region = sheet.and_then(|(_, native)| region_uv(native, icon));
    if let (Some((sheet, _)), Some(uv)) = (sheet, region) {
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
    crate::widget::theme::tip(&response, trailing);
    (response.clicked(), response.secondary_clicked())
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
    let want = solved_of(&widget, &at.style_of(&widget), at.assigned);
    let columns = widget.columns.max(1) as usize;
    let items: Vec<String> = widget
        .options
        .iter()
        .map(smol_str::SmolStr::to_string)
        .collect();
    let picked_rows = picked_set(&widget);
    let ink = Ink::of(ui, &at.theme);
    let style = at.style_of(&widget);
    backdrop(ui, at, &style, ui.max_rect());
    // The air between cards, which the theme spells as it does a container's.
    let gap = style.gap.unwrap_or(6.0).max(0.0);
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
        widget.row_height
    } else {
        side * 0.86
    };
    let cell = egui::vec2(side, tall);
    let lines = items.len().div_ceil(columns);
    let sheet = (!widget.source.is_empty())
        .then(|| crate::images::texture_of(at.eng, &ui.ctx().clone(), &widget.source).ok())
        .flatten()
        .map(|texture| {
            let native = crate::images::native_size(at.eng, &widget.source, &texture);
            (texture, native)
        });
    let mut picked = None;
    let mut aimed = None;
    let mut area = egui::ScrollArea::vertical()
        .id_salt(egui::Id::new(("balaur-cards", entity)))
        .auto_shrink([false, false]);
    if want.x > 0.0 {
        area = area.max_width(want.x);
    }
    if want.y > 0.0 {
        area = area.max_height(want.y);
    }
    area.show_rows(ui, cell.y + gap, lines, |ui, range| {
        for line in range {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
                for slot in 0..columns {
                    let at = line * columns + slot;
                    let Some(item) = items.get(at) else {
                        break;
                    };
                    let on = picked_rows.contains(item);
                    let face = Card {
                        size: cell,
                        font,
                        color,
                        ink: &ink,
                        on,
                        sheet: sheet.as_ref().map(|(texture, native)| (texture, *native)),
                    };
                    let hit = card(ui, item, &face);
                    if hit.0 {
                        picked = Some(at);
                    }
                    if hit.1 {
                        aimed = Some(at);
                    }
                }
            });
        }
    });
    let aimed = aimed.filter(|&card| !picked_rows.contains(&items[card]));
    let held = ui.input(|i| i.modifiers);
    if let Some((hit, mods, click)) = landed(picked, aimed, held) {
        let walk: Vec<usize> = (0..items.len()).collect();
        let next = after_click(
            &Clicked {
                items: &items,
                walk: &walk,
                picked: &picked_rows,
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

#[cfg(test)]
mod tests {
    use super::fields;

    #[test]
    fn a_rows_key_is_never_drawn() {
        let (icon, label, trailing, tint) =
            fields("\t\t▣\u{1f}Lid\u{1f}note\u{1f}#ff0000\u{1f}n_crate_b/n_lid");
        assert_eq!((icon, label, trailing), ("▣", "Lid", "note"));
        assert_eq!(tint, Some(egui::Color32::from_rgb(0xff, 0, 0)));
    }
}

//! Scene-tree UI elements: the `widget` component.
//!
//! A node carrying a `widget` component is a piece of game UI (label,
//! button, or panel) anchored to the screen. The component is registered
//! through the standard component registry, so widgets are addable and
//! editable in the editor and show up in the scene tree like any node.
//! Buttons record clicks into the component (`clicked` in `get_component`,
//! reset each frame).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use balaur_core::Engine;
use balaur_core::hecs::Entity;
// A widget's words are short and cloned once a node a frame; inline they are
// a copy, and as `String` they were an allocation each.
use egui::{Align2, Color32, Stroke, pos2, vec2};
use smol_str::SmolStr;

use crate::theme::family;
use crate::vocabulary::words as w;
use crate::widget_arena::{Begun, begin, keep, stamp_now};
pub(crate) use crate::widget_arrange::drawn_at;
use crate::widget_arrange::{
    Axis, box_of, contain, hold_to, lay_out, padding_of, record_measure, record_rect,
    roll_measurements, scroller, settle_rects, tabs,
};
pub(crate) use crate::widget_schema::{register_widget_component, register_widget_presets};
use crate::widget_theme::{Style, WidgetTheme};

/// A component colour (`[r, g, b, a]` in 0..=1) as egui's 8-bit one.
pub(crate) fn rgba_color(rgba: [f32; 4]) -> Color32 {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(
        channel(rgba[0]),
        channel(rgba[1]),
        channel(rgba[2]),
        channel(rgba[3]),
    )
}

#[derive(Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per scene property, and a scene sets them independently"
)]
pub struct Widget {
    pub kind: SmolStr,
    pub text: SmolStr,
    /// Hidden widgets draw nothing and take no clicks, but keep their state.
    pub visible: bool,
    pub anchor: SmolStr,
    pub x: f32,
    pub y: f32,
    /// Panel size in design pixels; 0 sizes to content. A minimum on buttons.
    pub width: f32,
    pub height: f32,
    /// Height of the widget's text, in design pixels.
    pub font_size: f32,
    /// The text's colour as `[r, g, b, a]` in 0..=1, the same representation
    /// the `color` component uses.
    pub text_color: [f32; 4],
    /// Method on this node's script, called when the widget is clicked.
    /// Empty means nothing is connected. A name rather than a function value:
    /// scene files cannot hold closures, and a name works on any backend.
    pub on_click: SmolStr,
    pub clicked: bool,
    /// Space inside a container's edge, in design pixels.
    pub padding: f32,
    /// Space between a container's children.
    pub gap: f32,
    /// Cross-axis placement of a container's children.
    pub align: SmolStr,
    /// Whether focus may land here, for a widget that could take it.
    pub focusable: bool,
    /// Method on this node's script, called when focus arrives.
    pub on_focus: SmolStr,
    /// A `widget_theme` reference, or empty to take the one above.
    pub theme: SmolStr,
    /// A localization key drawn instead of `text` when it is set.
    pub text_key: SmolStr,
    /// Share of a container's leftover space along its axis; 0 takes only
    /// what `width`/`height` or the content asks for.
    pub grow: f32,
    /// The author's floor, whatever the content measures.
    pub min_width: f32,
    pub min_height: f32,
    /// What fills a `draw` widget's rect: a method on this node's script or
    /// the nearest scripted ancestor's, or `file.rn:function` for a free
    /// function that needs no instance.
    pub draw: SmolStr,
    /// How wide a grab the seams between this container's children get, in
    /// design pixels; 0 leaves them fixed.
    pub handle: f32,
    /// Which child a `tab` shows, by node name; empty shows the first.
    pub active: SmolStr,
    /// The drawing surface a *root* widget belongs to; empty is the default
    /// one. Ignored on a child, which is placed by its parent.
    pub layer: SmolStr,
    /// Whether text breaks to the width it was given rather than running past
    /// it on one line.
    pub wrap: bool,
    /// A menu row that leaves the menu open when clicked, as a toggle does.
    pub keep_open: bool,
    /// Text against a button's far edge: a shortcut, or a menu's caret.
    pub trailing: SmolStr,
    /// A menu held open by the scene rather than by a click.
    pub showing: bool,

    /// Where text sits in the width the widget was given.
    pub text_align: SmolStr,
    /// A project-relative image for an `image` widget.
    pub source: SmolStr,
    /// Whether the text carries inline marks: `[b]`, `[i]`, `[color=#hex]`,
    /// `[center]`, `[wave]`, `[img=path width=N]`.
    pub markup: bool,
    /// Weight on the CSS scale, 100 to 900; 400 is regular, 700 bold.
    pub font_weight: f32,
    /// `normal` or `italic`.
    pub font_style: SmolStr,
    /// What a `field` shows while empty.
    pub placeholder: SmolStr,
    /// The most characters a `field` takes; 0 is no limit.
    pub max_length: f32,
    /// Draw a `field`'s text as dots.
    pub secret: bool,
    /// Keep a `field` to digits, a sign and a point.
    pub numeric: bool,
    /// Method on this node's script, called with the text after every edit.
    pub on_change: SmolStr,
    /// Method on this node's script, called with the text on Enter or when
    /// focus leaves the field.
    pub on_submit: SmolStr,
    /// What a `color` swatch holds, as `[r, g, b, a]` in 0..=1. Separate from
    /// `text_color`, which is the ink a widget draws its caption in.
    pub color: [f32; 4],
    /// The pitch of a `list` or `tree` row, in design pixels; 0 takes the
    /// font's own line height. Separate from `height`, which is the widget's.
    pub row_height: f32,
    /// Which of the theme's families the widget draws in: `ui`, `mono`,
    /// `heading` or `icon`.
    pub font: SmolStr,
    /// Whether a `check` is ticked.
    pub checked: bool,
    /// The name a `check` shares with the checks it is exclusive with: ticking
    /// one unticks the rest, and a ticked one clicked again stays ticked.
    /// Empty leaves the check on its own, flipping with every click.
    pub group: SmolStr,
    /// Where a `slider` or `progress` stands, between `min` and `max`.
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// The grid a `slider` snaps to; 0 is continuous.
    pub step: f32,
    /// What a `dropdown` offers; `text` is the one chosen.
    pub options: Vec<SmolStr>,
    /// How many children a `grid` puts on each row.
    pub columns: u32,
    /// Whether a `fold` shows its children.
    pub open: bool,
    /// Left, top, right and bottom margins a `fill` root keeps from its
    /// surface, in design pixels.
    pub inset: [f32; 4],
    /// A root that measures its bottom from the top of the on-screen
    /// keyboard, so a form stays above it.
    pub avoid_keyboard: bool,
    /// The nine-patch borders of an `image`, in the picture's own pixels.
    pub slice: [f32; 4],
    /// How far a finger drags a `scroll` before it scrolls, in design pixels.
    pub deadzone: f32,
    /// A `[roles.<name>]` entry of the theme, taken over the kind's own style.
    pub role: SmolStr,
    /// Text shown after the pointer rests on the widget.
    pub tooltip: SmolStr,
    /// A glyph from the theme's icon family, drawn before `text`.
    pub icon: SmolStr,
    /// Greyed out, and deaf to clicks.
    pub disabled: bool,
    /// A fill and an outline this one widget states, as `#rrggbb` or a name
    /// from the theme's `[colors]`; empty takes the theme's own.
    pub fill: SmolStr,
    pub stroke: SmolStr,
    /// Corner radius in design pixels; below zero takes the theme's own.
    pub radius: f32,
    /// How a container spreads its children along its own direction.
    pub justify: SmolStr,
    /// The air either side of a caption; below zero takes the theme's.
    pub padding_x: f32,
}

/// Whether focus can land on this widget.
///
/// Derived rather than declared: focus exists to activate something, so a
/// widget with nothing to activate is never a stop on the way to one. The
/// `focusable` flag can only take a candidate out, never put one in.
fn takes_focus(widget: &Widget) -> bool {
    widget.visible
        && widget.focusable
        && (matches!(widget.kind.as_str(), w::BUTTON | w::CHECK | w::FOLD)
            || !widget.on_click.is_empty())
}

/// Whether this kind lays its widget children out rather than ignoring them.
///
/// A `panel` counts: it already draws a frame, and a frame with things in it
/// is what a menu is made of. One with no children behaves exactly as before.
pub(crate) fn lays_out(kind: &str) -> bool {
    matches!(
        kind,
        w::ROW
            | w::COLUMN
            | w::PANEL
            | w::SCROLL
            | w::TAB
            | w::GRID
            | w::FLOW
            | w::FOLD
            | w::DIALOG
            | w::MENU
    )
}

/// Where and whether the widget layer draws. Games leave the default (full
/// window); editors point it at their viewport and enable it during play.
pub struct WidgetLayerConfig {
    pub enabled: bool,
    /// Whether arrows, Tab, Enter and Space move and activate the focus.
    ///
    /// Off by default: a game that moves with the arrows and jumps with Space
    /// would otherwise click its own HUD button. `standard_app` turns it on
    /// for a project that declares the `ui_*` actions, and a script asks for
    /// it with `ui.set_keyboard_focus`.
    pub keyboard: bool,
    /// Design-px rect (x, y, w, h); None = whole screen.
    pub rect: Option<[f32; 4]>,
    /// Where a root that names a `layer` draws instead. A name nothing here
    /// configures takes the default surface, so a host that confines the
    /// default confines every layer it was never told about.
    pub layers: HashMap<String, Surface>,
}

/// One drawing surface: whether roots on it draw, and where.
#[derive(Clone, Copy)]
pub struct Surface {
    pub enabled: bool,
    pub rect: Option<[f32; 4]>,
}

impl Default for Surface {
    fn default() -> Self {
        Self {
            enabled: true,
            rect: None,
        }
    }
}

impl Default for WidgetLayerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            keyboard: false,
            rect: None,
            layers: HashMap::new(),
        }
    }
}

/// Which widget the keyboard and the pad are pointing at.
///
/// One per screen, because that is what focus means: the thing an `accept`
/// would activate. Held as a resource rather than on the widget so that
/// moving it is one write, and so a script can ask without walking the tree.
#[derive(Default)]
pub struct UiFocus {
    /// The focused widget, or `None` before anything has taken focus.
    pub focused: Option<Entity>,
    /// Set by `focus_next` and friends and consumed by the next draw, so a
    /// script can move focus outside the pass that will act on it.
    pub pending: Option<Move>,
}

/// What a script or the keyboard asked focus to do.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Next,
    Previous,
    /// Activate what is focused, as a click would.
    Accept,
}
/// One widget and the widgets laid out inside it, as an arena so the draw can
/// recurse without holding a borrow of the world.
pub(crate) struct Placed {
    pub(crate) entity: Entity,
    /// The arena index of the widget that lays this one out, or `None` for a
    /// root. What lets a patched node work out the theme it inherits without
    /// the walk from the root that put it there.
    pub(crate) parent: Option<usize>,
    /// The node's name: what a tab strip labels a page with when the page
    /// says nothing itself.
    pub(crate) name: SmolStr,
    pub(crate) widget: Widget,
    pub(crate) children: Vec<usize>,
    /// The look resolved for this widget, worked out once a frame.
    ///
    /// Both the measure and the draw ask for it, and each of them more than
    /// once: without this the theme was walked, the role merged and the face
    /// built five times a widget a frame.
    pub(crate) look: RefCell<Option<Rc<Look>>>,
}

/// A widget's resolved look: what to paint it with and what face to draw its
/// caption in, with the theme's roles and its own overrides already applied.
pub(crate) struct Look {
    pub(crate) style: Rc<Style>,
    pub(crate) font: egui::FontId,
    pub(crate) ink: Color32,
}

/// The look of one widget, resolved once and kept for the rest of the frame.
pub(crate) fn look_of(arena: &[Placed], index: usize, theme: &WidgetTheme, scale: f32) -> Rc<Look> {
    let placed = &arena[index];
    if let Some(held) = placed.look.borrow().as_ref() {
        return Rc::clone(held);
    }
    let style = styled(theme, &placed.widget);
    let (ink, font) = face(theme, &style, &placed.widget, scale);
    let made = Rc::new(Look { style, font, ink });
    *placed.look.borrow_mut() = Some(Rc::clone(&made));
    made
}

/// The theme in force at one node, folded down its ancestors.
///
/// The walk from the root does this on the way past; a node patched on its
/// own has to climb to it instead.
pub(crate) fn theme_at(arena: &[Placed], index: usize, base: &Rc<WidgetTheme>) -> Rc<WidgetTheme> {
    let mut chain = Vec::new();
    let mut at = Some(index);
    while let Some(i) = at {
        chain.push(i);
        at = arena[i].parent;
    }
    let mut theme = base.clone();
    for i in chain.into_iter().rev() {
        theme = theme_of_owned(&arena[i].widget.theme, &theme);
    }
    theme
}

/// What the keyboard asked this frame, if the game has not asked already.
///
/// egui is where the widget layer's input comes from — clicks arrive that way
/// — so keys do too, and no dependency on the input plugin is needed for a
/// menu to work with a keyboard. A gamepad reaches focus through
/// `ui.focus_next()` and friends, wired to actions by whoever assembles the
/// plugins, which is the crate that knows about both.
fn keyboard_move(ctx: &egui::Context) -> Option<Move> {
    use egui::Key;
    ctx.input(|i| {
        let shifted_tab = i.key_pressed(Key::Tab) && i.modifiers.shift;
        if i.key_pressed(Key::ArrowUp) || i.key_pressed(Key::ArrowLeft) || shifted_tab {
            return Some(Move::Previous);
        }
        if i.key_pressed(Key::ArrowDown)
            || i.key_pressed(Key::ArrowRight)
            || i.key_pressed(Key::Tab)
        {
            return Some(Move::Next);
        }
        if i.key_pressed(Key::Enter) || i.key_pressed(Key::Space) {
            return Some(Move::Accept);
        }
        None
    })
}

/// Where focus may land, in the order the draw will reach them.
///
/// Walked from the roots rather than read off the arena: a button under a
/// hidden panel or on a surface the host turned off is never drawn, and an
/// `accept` on it would fire an `on_click` nobody could have seen to ask for.
fn focus_stops(placed: &[Placed], roots: &[usize], on: &dyn Fn(&str) -> bool) -> Vec<Entity> {
    let mut stops = Vec::new();
    let mut stack: Vec<usize> = roots
        .iter()
        .rev()
        .copied()
        .filter(|&root| on(&placed[root].widget.layer))
        .collect();
    while let Some(index) = stack.pop() {
        let one = &placed[index];
        if !one.widget.visible {
            continue;
        }
        if takes_focus(&one.widget) {
            stops.push(one.entity);
        }
        // Reversed, so the stack pops them in declaration order.
        stack.extend(one.children.iter().rev().copied());
    }
    stops
}

/// Move focus, or say which widget an `accept` activated.
///
/// Order is the order the widgets are drawn in, which is the order the scene
/// declares them — so focus walks a menu the way the tree reads.
fn advance(eng: &Engine, stops: &[Entity], asked: Option<Move>) -> Option<Entity> {
    let focus = eng.try_resource::<UiFocus>()?;
    let mut focus = focus.borrow_mut();
    // A focused widget that was hidden, freed or made unfocusable is no
    // longer a place focus can be.
    if focus.focused.is_some_and(|e| !stops.contains(&e)) {
        focus.focused = None;
    }
    let asked = focus.pending.take().or(asked)?;
    if stops.is_empty() {
        return None;
    }
    let at = focus
        .focused
        .and_then(|e| stops.iter().position(|s| *s == e));
    match asked {
        Move::Accept => return focus.focused,
        // Wraps, because a menu is a ring: past the last entry is the first.
        Move::Next => {
            let next = at.map_or(0, |i| (i + 1) % stops.len());
            focus.focused = Some(stops[next]);
        }
        Move::Previous => {
            let previous = at.map_or(stops.len() - 1, |i| (i + stops.len() - 1) % stops.len());
            focus.focused = Some(stops[previous]);
        }
    }
    None
}

/// Where a root's own `anchor`, `x` and `y` put it inside its surface. Only a
/// root is placed this way; every other widget is placed by its container.
fn root_placement(widget: &Widget, area: egui::Rect, scale: f32) -> (egui::Pos2, Align2) {
    let align = match widget.anchor.as_str() {
        w::TOP_RIGHT => Align2::RIGHT_TOP,
        w::BOTTOM_LEFT => Align2::LEFT_BOTTOM,
        w::BOTTOM_RIGHT => Align2::RIGHT_BOTTOM,
        w::CENTER => Align2::CENTER_CENTER,
        w::CENTER_LEFT => Align2::LEFT_CENTER,
        w::CENTER_RIGHT => Align2::RIGHT_CENTER,
        w::CENTER_TOP => Align2::CENTER_TOP,
        w::CENTER_BOTTOM => Align2::CENTER_BOTTOM,
        _ => Align2::LEFT_TOP,
    };
    // The offset runs inward from whichever edge the anchor names, so the
    // position falls out of the alignment rather than repeating it per anchor.
    let inward = |edge, min: f32, mid: f32, max: f32, offset: f32| match edge {
        egui::Align::Min => min + offset,
        egui::Align::Center => mid + offset,
        egui::Align::Max => max - offset,
    };
    let (centre, ox, oy) = (area.center(), widget.x * scale, widget.y * scale);
    let pos = pos2(
        inward(align.x(), area.min.x, centre.x, area.max.x, ox),
        inward(align.y(), area.min.y, centre.y, area.max.y, oy),
    );
    (pos, align)
}

/// `area` less the part the on-screen keyboard covers. The keyboard is
/// measured in the window's pixels, which are this pass's units.
fn above_keyboard(eng: &Engine, area: egui::Rect) -> egui::Rect {
    let covered = balaur_core::facts::device(eng).keyboard_height;
    let bottom = (area.max.y - covered).max(area.min.y);
    egui::Rect::from_min_max(area.min, egui::pos2(area.max.x, bottom))
}

/// Where a root goes and what box it is handed: `fill` takes the surface
/// less its insets so a container at the root fills the screen, a dialog
/// sits in the middle over the dimmed screen, the rest anchor as before.
fn root_frame(
    widget: &Widget,
    area: egui::Rect,
    scale: f32,
) -> (egui::Pos2, Align2, egui::Vec2, egui::Order) {
    if widget.anchor == w::FILL {
        let inset = widget.inset.map(|v| v * scale);
        let rect = egui::Rect::from_min_max(
            area.min + vec2(inset[0], inset[1]),
            area.max - vec2(inset[2], inset[3]),
        );
        return (
            rect.min,
            Align2::LEFT_TOP,
            rect.size().max(egui::Vec2::ZERO),
            egui::Order::Middle,
        );
    }
    if widget.kind == w::DIALOG {
        return (
            area.center(),
            Align2::CENTER_CENTER,
            egui::Vec2::ZERO,
            egui::Order::Foreground,
        );
    }
    let (pos, align) = root_placement(widget, area, scale);
    (pos, align, egui::Vec2::ZERO, egui::Order::Middle)
}

/// Draw every widget entity. Runs inside the frame's egui pass, after the
/// scripts' `draw_ui`.
pub(crate) fn draw(eng: &Engine, ctx: &egui::Context, scale: f32) {
    let Some(layer) = eng.try_resource::<WidgetLayerConfig>() else {
        return;
    };
    let (default, surfaces, keyboard) = {
        let layer = layer.borrow();
        (
            Surface {
                enabled: layer.enabled,
                rect: layer.rect,
            },
            layer.layers.clone(),
            layer.keyboard,
        )
    };
    let screen = ctx.viewport_rect();
    let stamp = stamp_now(eng);
    let Begun {
        placed,
        roots,
        index_of,
        fresh,
        touched,
    } = begin(eng, stamp);
    // Nothing to draw and nothing to focus: a scene with no widgets pays for
    // the resource lookup and no more.
    if placed.is_empty() {
        return;
    }
    // A layer nothing configured takes the default surface, so a host that
    // confines the default confines everything it was not told about.
    let surface_of = |name: &str| {
        if name.is_empty() {
            default
        } else {
            surfaces.get(name).copied().unwrap_or(default)
        }
    };
    let was_focused = eng
        .try_resource::<UiFocus>()
        .and_then(|f| f.borrow().focused);
    // A field being typed into owns the keys, arrows included.
    let asked = (keyboard && !ctx.egui_wants_keyboard_input())
        .then(|| keyboard_move(ctx))
        .flatten();
    let stops = focus_stops(&placed, &roots, &|name| surface_of(name).enabled);
    let accepted = advance(eng, &stops, asked);
    let focused = eng
        .try_resource::<UiFocus>()
        .and_then(|f| f.borrow().focused);
    let mut painting = Painting {
        eng,
        arena: &placed,
        scale,
        focused,
        theme: theme_root(),
        assigned: egui::Vec2::ZERO,
        bounds: egui::Vec2::ZERO,
        edits: Vec::new(),
        rects: crate::widget_taffy::Rects::default(),
        fresh,
        touched,
        // An `accept` is a click by another name: same `clicked`, same
        // `on_click`, so it starts the frame's list rather than a second one.
        clicked: accepted.into_iter().collect(),
        state: (false, false),
    };
    for root in &roots {
        let root = *root;
        let widget = &placed[root].widget;
        if !widget.visible {
            continue;
        }
        // Each root draws on the surface it names.
        let surface = surface_of(&widget.layer);
        if !surface.enabled {
            continue;
        }
        let area = match surface.rect {
            Some([x, y, w, h]) => {
                egui::Rect::from_min_size(pos2(x * scale, y * scale), vec2(w * scale, h * scale))
            }
            None => screen,
        };
        draw_root(ctx, &mut painting, root, area);
    }
    // Published at the end of the draw, not the start of the next one: a
    // script's `draw_ui` runs after this and reads this frame's rects.
    settle_rects();
    roll_measurements();
    crate::widget_taffy::sweep(eng);
    let edits = std::mem::take(&mut painting.edits);
    let clicked = std::mem::take(&mut painting.clicked);
    // Dropped before the arena moves: `Painting` borrows it for the draw.
    drop(painting);
    keep(placed, roots, index_of, stamp);
    // Only on the change: a handler firing every frame focus merely *stayed*
    // would be a different event, and not a useful one.
    let arrived = (focused != was_focused).then_some(focused).flatten();
    crate::widget_input::record(eng, &clicked, edits, arrived);
}

/// Draw one root into the area its surface gives it, and record where it
/// landed. Split from [`draw`] under `MAX_FN_LINES`; the seam is one root's
/// own placement and pass, which needs nothing from the loop around it.
fn draw_root(ctx: &egui::Context, painting: &mut Painting<'_>, root: usize, area: egui::Rect) {
    let (eng, placed, scale) = (painting.eng, painting.arena, painting.scale);
    let entity = placed[root].entity;
    let widget = &placed[root].widget;
    if widget.kind == w::DIALOG {
        crate::widget_kinds::dialog_backdrop(ctx, entity, area);
    }
    let area = if widget.avoid_keyboard {
        above_keyboard(eng, area)
    } else {
        area
    };
    let (pos, align, assigned, order) = root_frame(widget, area, scale);
    painting.assigned = assigned;
    painting.rects = place_root(eng, ctx, painting, root, area, (pos, align, assigned));
    let shown = egui::Area::new(egui::Id::new(("balaur-widget", entity)))
        .order(order)
        .pivot(align)
        .fixed_pos(pos)
        // A widget appears when the scene says so, at the alpha its own
        // theme sets; egui's fade would override both.
        .fade_in(false)
        .show(ctx, |ui| {
            // A root handed a box reserves it before anything draws: its
            // children are placed at absolute rects and report nothing
            // back, so the area would otherwise hug the first of them.
            if assigned != egui::Vec2::ZERO {
                ui.set_max_size(assigned);
                ui.advance_cursor_after_rect(egui::Rect::from_min_size(pos, assigned));
            }
            draw_one(ui, painting, root);
        });
    painting.assigned = egui::Vec2::ZERO;
    // A root is placed by nobody, so it records its own rect, from egui's
    // memory: the response's rect can lag it by a frame.
    let drawn = ctx
        .memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entity))))
        .unwrap_or(shown.response.rect);
    record_rect(entity, drawn);
}

/// Where everything in one root goes, decided before a pixel is drawn.
///
/// A root that fills is solved against its own box. One on a corner takes
/// what it measures, and only then is it known where its corner puts it, so
/// it is solved at the origin and moved once the size is out.
fn place_root(
    eng: &Engine,
    ctx: &egui::Context,
    painting: &mut Painting<'_>,
    root: usize,
    area: egui::Rect,
    frame: (egui::Pos2, Align2, egui::Vec2),
) -> crate::widget_taffy::Rects {
    let (pos, align, assigned) = frame;
    let hugs = assigned == egui::Vec2::ZERO;
    let space = if hugs {
        crate::widget_taffy::Room::hugging(egui::Rect::from_min_size(egui::Pos2::ZERO, area.size()))
    } else {
        crate::widget_taffy::Room::fixed(egui::Rect::from_min_size(pos, assigned))
    };
    let probe = root_ui(ctx);
    let touched = painting.touched.clone();
    let mut rects = crate::widget_taffy::solve(
        eng,
        painting.arena,
        root,
        &probe,
        painting.scale,
        &theme_root(),
        &space,
        painting.fresh,
        &touched,
    );
    if hugs {
        let size = rects.get(&root).map_or(egui::Vec2::ZERO, egui::Rect::size);
        let shift = align.anchor_size(pos, size).min.to_vec2();
        for rect in rects.values_mut() {
            *rect = rect.translate(shift);
        }
    }
    rects
}

/// A `Ui` over the viewport, for a pass that has to measure before it draws.
fn root_ui(ctx: &egui::Context) -> egui::Ui {
    egui::Ui::new(
        ctx.clone(),
        egui::Id::new("balaur-layout-probe"),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    )
}

thread_local! {
    /// The empty theme, made once. A theme remembers the styles it has
    /// resolved, so one built fresh every frame remembers nothing.
    static BARE: Rc<WidgetTheme> = Rc::new(WidgetTheme::default());
}

/// The theme a root starts from, before it names one of its own.
fn theme_root() -> Rc<WidgetTheme> {
    BARE.with(Rc::clone)
}

/// What one draw pass carries down the widget tree.
///
/// A struct rather than six arguments: the recursion is three functions deep
/// and every one of them was growing a parameter per feature.
pub(crate) struct Painting<'a> {
    pub(crate) eng: &'a Engine,
    pub(crate) arena: &'a [Placed],
    pub(crate) scale: f32,
    pub(crate) focused: Option<Entity>,
    /// The theme in force here, inherited unless a widget names its own.
    pub(crate) theme: Rc<WidgetTheme>,
    /// The box the parent container handed this widget, per axis; 0 on an
    /// axis the parent left free, and on both for a root.
    pub(crate) assigned: egui::Vec2,
    /// The box the container now laying out children holds, per axis; 0 where
    /// it is free to grow, in which case children hug rather than fill.
    pub(crate) bounds: egui::Vec2,
    pub(crate) clicked: Vec<Entity>,
    pub(crate) edits: Vec<(Entity, Edit)>,
    /// Where the layout pass put every widget in the subtree being drawn.
    pub(crate) rects: crate::widget_taffy::Rects,
    /// Whether the arena was rebuilt this pass. False means the tree taffy
    /// holds already describes it, so a solve restyles the root and no more.
    pub(crate) fresh: bool,
    /// The slots re-read this pass, for a solve to push into taffy. Taken by
    /// the first solve: the tree is shared, so once is enough.
    pub(crate) touched: Vec<usize>,
    /// Whether the pointer is over the widget being drawn, and whether it is
    /// held there. Set by the draw and never by the measure: a size that
    /// followed the pointer would move whatever sits beside it.
    pub(crate) state: (bool, bool),
}

impl Painting<'_> {
    /// Whether a subtree has to be walked again rather than taken as taffy
    /// already holds it: because the whole arena was rebuilt, or because one
    /// of this pass's writes landed inside this subtree. A kind that places
    /// its own children solves them apart from the root, so it has to ask.
    pub(crate) fn deep(&self, root: usize) -> bool {
        self.fresh
            || self.touched.iter().any(|&at| {
                let mut node = Some(at);
                while let Some(index) = node {
                    if index == root {
                        return true;
                    }
                    node = self.arena[index].parent;
                }
                false
            })
    }

    /// The style a widget is drawn with, in the theme in force here.
    pub(crate) fn style_of(&self, widget: &Widget) -> Rc<Style> {
        self.in_state(styled(&self.theme, widget))
    }

    /// The look of the widget at `index`, resolved once a frame.
    pub(crate) fn look(&self, index: usize) -> Rc<Look> {
        let look = look_of(self.arena, index, &self.theme, self.scale);
        let styled = self.in_state(Rc::clone(&look.style));
        if Rc::ptr_eq(&styled, &look.style) {
            return look;
        }
        let (ink, font) = face(&self.theme, &styled, &self.arena[index].widget, self.scale);
        Rc::new(Look {
            style: styled,
            font,
            ink,
        })
    }

    /// The look with no state on it, for a kind that answers the pointer with
    /// its own response rather than with the box the layout gave it.
    pub(crate) fn resting(&self, index: usize) -> Rc<Look> {
        look_of(self.arena, index, &self.theme, self.scale)
    }

    /// A style with its `hover` or `active` table over it, where the pointer
    /// put the widget in one. A style with neither answers with itself.
    fn in_state(&self, style: Rc<Style>) -> Rc<Style> {
        let (hovered, held) = self.state;
        if !(hovered || held) || (style.hover.is_none() && style.active.is_none()) {
            return style;
        }
        Rc::new(style.in_state(hovered, held))
    }
}

/// The style a widget is drawn with: its kind's, the `role` it names over
/// that, and the `fill`, `stroke` and `radius` it states over both.
///
/// The measure pass calls this too, so a row is sized at the face it draws at.
pub(crate) fn styled(theme: &WidgetTheme, widget: &Widget) -> Rc<Style> {
    let settled = theme.resolved(&widget.kind, &widget.role);
    // The theme's own answer, shared, unless this widget overrides part of
    // it — which most do not, and a screen of widgets is mostly one of a few
    // styles repeated.
    if widget.fill.is_empty()
        && widget.stroke.is_empty()
        && widget.radius < 0.0
        && widget.padding_x < 0.0
    {
        return settled;
    }
    let mut style = (*settled).clone();
    if !widget.fill.is_empty() {
        style.fill = theme.token(&widget.fill);
    }
    if !widget.stroke.is_empty() {
        style.stroke = theme.token(&widget.stroke);
    }
    if widget.radius >= 0.0 {
        style.radius = Some(widget.radius);
    }
    if widget.padding_x >= 0.0 {
        style.padding_x = Some(widget.padding_x);
    }
    Rc::new(style)
}

/// The near-white a caption takes when neither the widget nor its theme says.
pub(crate) const DEFAULT_INK: Color32 = Color32::from_rgb(238, 241, 244);

/// The theme family a widget draws in: the one it names, else its role's,
/// else `ui`. The shaper needs the name as well as the face.
pub(crate) fn family_of<'a>(style: &'a Style, widget: &'a Widget) -> &'a str {
    // Unset is empty before the schema's default lands and `ui` after it, and
    // both mean the same: whatever the role or the kind asked for.
    if widget.font.is_empty() || widget.font == w::UI {
        style.font.as_deref().unwrap_or(w::UI)
    } else {
        widget.font.as_str()
    }
}

/// The ink and the face a widget draws its caption in.
///
/// A property left at its default is the widget saying nothing, so the theme
/// answers: a transparent `text_color`, a `font_size` of 0, the `ui` family
/// and a weight of 400 each take what the role or the kind carries.
pub(crate) fn face(
    theme: &WidgetTheme,
    style: &Style,
    widget: &Widget,
    scale: f32,
) -> (Color32, egui::FontId) {
    // The theme's own text colour last, not a constant: a widget with no role
    // drew in near-white, which is invisible on a light theme.
    let ink = if widget.text_color[3] > 0.0 {
        rgba_color(widget.text_color)
    } else {
        style
            .text_color
            .or_else(|| theme.token("text"))
            .unwrap_or(DEFAULT_INK)
    };
    let size = if widget.font_size > 0.0 {
        widget.font_size
    } else {
        style.font_size.unwrap_or(16.0)
    };
    (
        ink,
        egui::FontId::new(size * scale, family(family_of(style, widget))),
    )
}

/// The weight a widget draws at, the theme answering for one left at 400.
pub(crate) fn weight_of(style: &Style, widget: &Widget) -> f32 {
    if (widget.font_weight - 400.0).abs() > f32::EPSILON {
        return widget.font_weight;
    }
    style.weight.unwrap_or(400.0)
}

/// A change a container made while drawing — a dragged seam, a chosen tab.
///
/// Applied after the pass: the tree the draw walked is a snapshot, and
/// writing to the world mid-walk would mean the rest of the frame laid out
/// against numbers half of it had never seen.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum Edit {
    Width(f32),
    Height(f32),
    Active(String),
    /// A field's text as typed so far.
    Text(String),
    /// A field's text when Enter was pressed or focus left it.
    Submit(String),
    /// A slider's number.
    Value(f32),
    /// A fold shown or hidden.
    Open(bool),
    /// A dropdown's pick.
    Choice(String),
    /// A swatch's colour.
    Color([f32; 4]),
}

/// The theme a widget's own subtree is drawn with, for a caller that holds no
/// `Engine` handy — the layout pass, which walks the same tree the draw does.
pub(crate) fn theme_of_owned(reference: &str, inherited: &Rc<WidgetTheme>) -> Rc<WidgetTheme> {
    if reference.is_empty() {
        return inherited.clone();
    }
    THEMES.with(|held| {
        held.borrow()
            .get(reference)
            .cloned()
            .unwrap_or_else(|| inherited.clone())
    })
}

thread_local! {
    /// Every theme the draw has resolved this session, by asset path, so the
    /// layout pass can reach one without an `Engine`.
    static THEMES: RefCell<HashMap<String, Rc<WidgetTheme>>> = RefCell::new(HashMap::new());
}

/// The theme in force for a widget: its own, or the nearest ancestor's.
///
/// Resolved once per frame per root rather than per widget, because a screen
/// has one look and walking up the tree for every button to find it out would
/// be work with a known answer.
pub(crate) fn theme_of(
    eng: &Engine,
    reference: &str,
    inherited: &Rc<WidgetTheme>,
) -> Rc<WidgetTheme> {
    if reference.is_empty() {
        return inherited.clone();
    }
    match balaur_core::assets::load_typed::<WidgetTheme>(eng, reference) {
        Ok(theme) => {
            THEMES.with(|held| {
                held.borrow_mut()
                    .insert(reference.to_string(), theme.clone());
            });
            theme
        }
        Err(err) => {
            // Once per reference: a missing theme is a typo in a scene file,
            // and repeating it sixty times a second buries everything else.
            static WARNED: std::sync::Mutex<Option<std::collections::BTreeSet<String>>> =
                std::sync::Mutex::new(None);
            if let Ok(mut seen) = WARNED.lock() {
                let seen = seen.get_or_insert_with(std::collections::BTreeSet::new);
                if seen.insert(reference.to_string()) {
                    tracing::warn!("widget theme '{reference}': {err:#}");
                }
            }
            inherited.clone()
        }
    }
}

/// Draw one widget and, when it is a container, what is laid out inside it.
///
/// A child's `anchor`, `x` and `y` are ignored: it is placed by its parent,
/// and a menu that moved when you nudged one entry would not be a menu.
pub(crate) fn draw_one(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    if !widget.visible {
        return;
    }
    // Restored before returning, so a themed subtree does not leak its look
    // onto whatever the caller draws next.
    let outer = at.theme.clone();
    at.theme = theme_of(at.eng, &widget.theme, &outer);
    draw_themed(ui, at, index);
    at.theme = outer;
}

/// What a widget shows: its key, translated in the locale in force, or its
/// literal text. Resolved every frame, which is why a locale switch shows on
/// the next one without anything having to be told.
pub(crate) fn caption(eng: &Engine, widget: &Widget) -> SmolStr {
    if widget.text_key.is_empty() {
        return widget.text.clone();
    }
    balaur_core::strings::tr(eng, &widget.text_key, &[]).into()
}

/// Everything a widget kind draws, with the theme already resolved.
fn draw_themed(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let disabled = at.arena[index].widget.disabled;
    let outer = std::mem::replace(&mut at.state, pointer_state(ui, disabled));
    draw_kind(ui, at, index);
    at.state = outer;
}

/// Whether the pointer is over the box this widget was given, and whether it
/// is held there. The box, not a response: every kind gets the same answer
/// this way, including the ones egui draws and the ones that only paint.
fn pointer_state(ui: &egui::Ui, disabled: bool) -> (bool, bool) {
    if disabled || !ui.rect_contains_pointer(ui.max_rect()) {
        return (false, false);
    }
    (true, ui.ctx().input(|i| i.pointer.primary_down()))
}

fn draw_kind(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let caption = caption(at.eng, widget);
    let scale = at.scale;
    let look = at.look(index);
    let (color, font) = (look.ink, look.font.clone());
    let (tooltip, entity) = (widget.tooltip.clone(), placed.entity);
    if widget.disabled {
        ui.disable();
    }
    match widget.kind.as_str() {
        w::BUTTON => {
            crate::widget_button::button(ui, at, index, &caption, &font, color);
        }
        // A line the player types into. The text lives on the widget; the
        // draw only reports what was typed, and the next tick writes it.
        w::FIELD => crate::widget_text::field(ui, at, index, &font, color),
        w::TEXT_AREA => crate::widget_text::text_area(ui, at, index, &font, color),
        // A dialog is a panel drawn over a dimmed screen; the dimming is the
        // root draw's, so here it is the panel.
        w::PANEL | w::DIALOG => panel(ui, at, index, &caption, &font, color),
        w::CHECK => crate::widget_kinds::check(ui, at, index, &caption, &font, color),
        w::COLOR => crate::widget_kinds::color(ui, at, index),
        w::DROPDOWN => crate::widget_kinds::dropdown(ui, at, index, &font, color),
        w::MENU => crate::widget_kinds::menu(ui, at, index, &caption, &font, color),
        w::LIST => crate::widget_kinds::list(ui, at, index, &font, color),
        w::TREE => crate::widget_kinds::tree(ui, at, index, &font, color),
        w::TABLE => crate::widget_kinds::table(ui, at, index, &font, color),
        // The file being edited, with the gutter and the colouring the script
        // call has always had.
        w::CODE => crate::widget_kinds::code(ui, at, index),
        w::SLIDER => crate::widget_kinds::slider(ui, at, index),
        w::DRAG_VALUE => crate::widget_kinds::drag_value(ui, at, index, &font, color),
        w::PROGRESS => crate::widget_kinds::progress(ui, at, index, &caption, &font, color),
        w::SEPARATOR => crate::widget_kinds::separator(ui, at, index),
        w::GRID => crate::widget_kinds::grid(ui, at, index),
        w::FLOW => crate::widget_kinds::flow(ui, at, index),
        w::FOLD => crate::widget_kinds::fold(ui, at, index, &caption, &font, color),
        // A picture from the project, sized by what it states or by itself.
        w::IMAGE => image(ui, at, index),
        w::ROW => contain(ui, at, index, Axis::Row),
        w::COLUMN => contain(ui, at, index, Axis::Column),
        // A box that clips, with its children free to run past it.
        w::SCROLL => scroller(ui, at, index),
        // One child at a time, with a strip of the rest above it. The strip is
        // drawn here rather than authored, so adding a page is adding a node.
        w::TAB => tabs(ui, at, index),
        // The rect a script fills. The node owns the placement, the script
        // owns everything inside it, and neither has to know the other.
        w::DRAW => {
            if widget.draw.is_empty() {
                return;
            }
            let want = box_of(widget, at.assigned, scale);
            let room = ui.max_rect();
            let size = vec2(
                if want.x > 0.0 { want.x } else { room.width() },
                if want.y > 0.0 { want.y } else { room.height() },
            );
            let rect = egui::Rect::from_min_size(room.min, size);
            let entity = placed.entity;
            let target = widget.draw.clone();
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            inner.set_clip_rect(rect.intersect(ui.clip_rect()));
            crate::bridge::scoped_named(
                at.eng,
                &mut inner,
                balaur_core::node_id_of(entity),
                &target,
            );
            // What the script painted, not the box it was handed: a hatch
            // given the whole sheet would ask for it ever after.
            let used = inner.min_rect().size();
            record_measure(entity, used);
            ui.advance_cursor_after_rect(egui::Rect::from_min_size(
                rect.min,
                vec2(
                    if want.x > 0.0 { want.x } else { used.x },
                    if want.y > 0.0 { want.y } else { used.y },
                ),
            ));
        }
        _ => {
            if !crate::widget_text::shaped_label(ui, at, index, widget, &caption, color, &font) {
                let mut label = egui::Label::new(
                    egui::RichText::new(caption.as_str())
                        .font(font)
                        .color(color),
                );
                // A stated width is a column, so the text is cut to it rather
                // than run past into whatever sits beside it. Without one,
                // `extend` is the old behaviour: one line, however wide.
                label = if widget.wrap {
                    label.wrap()
                } else if widget.width > 0.0 {
                    label.truncate()
                } else {
                    label.extend()
                };
                ui.with_layout(egui::Layout::top_down(across(&widget.text_align)), |ui| {
                    ui.add(label);
                });
            }
        }
    }
    tip(ui, entity, &tooltip);
}

/// Hover text over whatever the kind just drew, from the rect it took.
///
/// Applied here rather than inside twenty draws: every kind ends up with a
/// rect, and a disabled widget keeps its tooltip because that is where it
/// says why it is off.
fn tip(ui: &egui::Ui, entity: Entity, tooltip: &str) {
    if tooltip.is_empty() {
        return;
    }
    let rect = ui.min_rect();
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let id = egui::Id::new(("balaur-tip", entity));
    let response = ui.interact(rect, id, egui::Sense::hover());
    if ui.is_enabled() {
        response.on_hover_text(tooltip);
    } else {
        response.on_disabled_hover_text(tooltip);
    }
}

/// Where text sits in the width the widget was given.
pub(crate) fn across(align: &str) -> egui::Align {
    match align {
        w::CENTER => egui::Align::Center,
        w::END => egui::Align::Max,
        _ => egui::Align::Min,
    }
}

/// The box an image takes: what it states, else its own size, keeping the
/// aspect where only one axis is given.
pub(crate) fn image_size(stated: egui::Vec2, native: egui::Vec2) -> egui::Vec2 {
    let aspect = if native.y > 0.0 {
        native.x / native.y
    } else {
        1.0
    };
    match (stated.x > 0.0, stated.y > 0.0) {
        (true, true) => stated,
        (true, false) => vec2(stated.x, stated.x / aspect),
        (false, true) => vec2(stated.y * aspect, stated.y),
        (false, false) => native,
    }
}

/// A framed box that lays its children out, with the padding taken off in
/// floats: `egui::Margin` is whole device pixels, and 10 design px at the
/// editor's 1.25 scale is not one.
///
/// The background is reserved before the children and filled in afterwards,
/// which is how it can be sized to content it has not drawn yet.
fn panel(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
    color: Color32,
) {
    let scale = at.scale;
    let widget = &at.arena[index].widget;
    let style = at.style_of(widget);
    let pad = padding_of(widget, &style, scale);
    let box_size = box_of(widget, at.assigned, scale);
    let plate = ui.painter().add(egui::Shape::Noop);
    let min = (box_size - egui::Vec2::splat(pad * 2.0)).max(egui::Vec2::ZERO);
    let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(ui.max_rect().shrink(pad)));
    hold_to(&mut inner, min);
    if !caption.is_empty() {
        inner.label(egui::RichText::new(caption).font(font.clone()).color(color));
    }
    // A panel with nothing in it is the panel it always was.
    let held = std::mem::replace(&mut at.bounds, min);
    lay_out(&mut inner, at, index, Axis::Column);
    at.bounds = held;
    let background = inner.min_rect().expand(pad);
    if let Some(path) = style.image.as_ref() {
        crate::widget_kinds::nine_patch_plate(
            ui,
            at.eng,
            plate,
            path,
            style.slice,
            background,
            scale,
        );
    } else {
        ui.painter().set(
            plate,
            egui::epaint::RectShape::new(
                background,
                egui::CornerRadius::same(style.radius.map_or(8.0, |r| r * scale) as u8),
                style.fill.unwrap_or(Color32::from_black_alpha(96)),
                style
                    .stroke
                    .map_or(Stroke::NONE, |c| Stroke::new(style.stroke_px(), c)),
                egui::StrokeKind::Inside,
            ),
        );
    }
    ui.advance_cursor_after_rect(background);
}

/// Draw a project image. A source that will not load is reported once and
/// draws nothing: a missing picture must not take the frame down.
fn image(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    if widget.source.is_empty() {
        return;
    }
    // A picture that names a handler is a button made of a picture: it senses
    // the click and `settle_clicks` calls `on_click` as it does for any other.
    let entity = placed.entity;
    let sense = if widget.on_click.is_empty() {
        egui::Sense::hover()
    } else {
        egui::Sense::click()
    };
    let ctx = ui.ctx().clone();
    match crate::images::texture_of(at.eng, &ctx, &widget.source) {
        Ok(texture) => {
            let size = image_size(box_of(widget, at.assigned, at.scale), texture.size_vec2());
            if widget.slice.iter().any(|v| *v > 0.0) {
                // The borders stay the picture's own size; only the middle
                // stretches to the box.
                let (rect, response) = ui.allocate_exact_size(size, sense);
                let shapes = crate::widget_kinds::nine_patch(
                    texture.id(),
                    texture.size_vec2(),
                    rect,
                    widget.slice,
                    at.scale,
                );
                ui.painter().add(egui::Shape::Vec(shapes));
                if response.clicked() {
                    at.clicked.push(entity);
                }
            } else if ui
                .add(egui::Image::new((texture.id(), size)).sense(sense))
                .clicked()
            {
                at.clicked.push(entity);
            }
        }
        Err(err) => warn_once(&widget.source, &err),
    }
}

/// Report a source once. Repeating it sixty times a second buries everything
/// else in the log.
fn warn_once(source: &str, err: &anyhow::Error) {
    static WARNED: std::sync::Mutex<Option<std::collections::BTreeSet<String>>> =
        std::sync::Mutex::new(None);
    if let Ok(mut seen) = WARNED.lock() {
        let seen = seen.get_or_insert_with(std::collections::BTreeSet::new);
        if seen.insert(source.to_string()) {
            tracing::warn!("widget image '{source}': {err:#}");
        }
    }
}

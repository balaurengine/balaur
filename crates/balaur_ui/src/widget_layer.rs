//! Scene-tree UI elements: the `widget` component.
//!
//! A node carrying a `widget` component is a piece of game UI (label,
//! button, or panel) anchored to the screen. The component is registered
//! through the standard component registry, so widgets are addable and
//! editable in the editor and show up in the scene tree like any node.
//! Buttons record clicks into the component (`clicked` in `get_component`,
//! reset each frame).

use std::collections::HashMap;
use std::rc::Rc;

use balaur_core::hecs::Entity;
use balaur_core::Engine;
use egui::{pos2, vec2, Align2, Color32, Stroke};

use crate::theme::family;
pub(crate) use crate::widget_arrange::drawn_at;
use crate::widget_arrange::{
    box_of, contain, hold_to, lay_out, padding_of, record_rect, roll_measurements, scroller,
    settle_rects, tabs, Axis,
};
pub(crate) use crate::widget_schema::{register_widget_component, register_widget_presets};
use crate::widget_theme::WidgetTheme;

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
    pub kind: String,
    pub text: String,
    /// Hidden widgets draw nothing and take no clicks, but keep their state.
    pub visible: bool,
    pub anchor: String,
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
    pub on_click: String,
    pub clicked: bool,
    /// Space inside a container's edge, in design pixels.
    pub padding: f32,
    /// Space between a container's children.
    pub gap: f32,
    /// Cross-axis placement of a container's children.
    pub align: String,
    /// Whether focus may land here, for a widget that could take it.
    pub focusable: bool,
    /// Method on this node's script, called when focus arrives.
    pub on_focus: String,
    /// A `widget_theme` reference, or empty to take the one above.
    pub theme: String,
    /// A localization key drawn instead of `text` when it is set.
    pub text_key: String,
    /// Share of a container's leftover space along its axis; 0 takes only
    /// what `width`/`height` or the content asks for.
    pub grow: f32,
    /// The author's floor, whatever the content measures.
    pub min_width: f32,
    pub min_height: f32,
    /// What fills a `draw` widget's rect: a method on this node's script or
    /// the nearest scripted ancestor's, or `file.rn:function` for a free
    /// function that needs no instance.
    pub draw: String,
    /// How wide a grab the seams between this container's children get, in
    /// design pixels; 0 leaves them fixed.
    pub handle: f32,
    /// Which child a `tab` shows, by node name; empty shows the first.
    pub active: String,
    /// The drawing surface a *root* widget belongs to; empty is the default
    /// one. Ignored on a child, which is placed by its parent.
    pub layer: String,
    /// Whether text breaks to the width it was given rather than running past
    /// it on one line.
    pub wrap: bool,

    /// Where text sits in the width the widget was given.
    pub text_align: String,
    /// A project-relative image for an `image` widget.
    pub source: String,
    /// Whether the text carries inline marks: `[b]`, `[i]`, `[color=#hex]`,
    /// `[center]`, `[wave]`, `[img=path width=N]`.
    pub markup: bool,
    /// Weight on the CSS scale, 100 to 900; 400 is regular, 700 bold.
    pub font_weight: f32,
    /// `normal` or `italic`.
    pub font_style: String,
    /// What a `field` shows while empty.
    pub placeholder: String,
    /// The most characters a `field` takes; 0 is no limit.
    pub max_length: f32,
    /// Draw a `field`'s text as dots.
    pub secret: bool,
    /// Keep a `field` to digits, a sign and a point.
    pub numeric: bool,
    /// Method on this node's script, called with the text after every edit.
    pub on_change: String,
    /// Method on this node's script, called with the text on Enter or when
    /// focus leaves the field.
    pub on_submit: String,
    /// Whether a `check` is ticked.
    pub checked: bool,
    /// Where a `slider` or `progress` stands, between `min` and `max`.
    pub value: f32,
    pub min: f32,
    pub max: f32,
    /// The grid a `slider` snaps to; 0 is continuous.
    pub step: f32,
    /// What a `dropdown` offers; `text` is the one chosen.
    pub options: Vec<String>,
    /// How many children a `grid` puts on each row.
    pub columns: u32,
    /// Whether a `fold` shows its children.
    pub open: bool,
    /// Left, top, right and bottom margins a `fill` root keeps from its
    /// surface, in design pixels.
    pub inset: [f32; 4],
    /// The nine-patch borders of an `image`, in the picture's own pixels.
    pub slice: [f32; 4],
    /// How far a finger drags a `scroll` before it scrolls, in design pixels.
    pub deadzone: f32,
}

/// Whether focus can land on this widget.
///
/// Derived rather than declared: focus exists to activate something, so a
/// widget with nothing to activate is never a stop on the way to one. The
/// `focusable` flag can only take a candidate out, never put one in.
fn takes_focus(widget: &Widget) -> bool {
    widget.visible
        && widget.focusable
        && (matches!(widget.kind.as_str(), "button" | "check" | "fold")
            || !widget.on_click.is_empty())
}

/// Whether this kind lays its widget children out rather than ignoring them.
///
/// A `panel` counts: it already draws a frame, and a frame with things in it
/// is what a menu is made of. One with no children behaves exactly as before.
pub(crate) fn lays_out(kind: &str) -> bool {
    matches!(
        kind,
        "row" | "column" | "panel" | "scroll" | "tab" | "grid" | "flow" | "fold" | "dialog"
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
    /// The node's name: what a tab strip labels a page with when the page
    /// says nothing itself.
    pub(crate) name: String,
    pub(crate) widget: Widget,
    pub(crate) children: Vec<usize>,
}

/// The widget forest, in scene-tree order.
///
/// A widget's parent is its nearest *widget* ancestor, not its parent node:
/// a menu is usually a panel with an empty grouping node or two inside it,
/// and the layout should not care. Tree order is sibling order, which is what
/// makes a row read left to right the way the scene reads top to bottom.
fn forest(eng: &Engine) -> (Vec<Placed>, Vec<usize>) {
    use balaur_core::scene::Children;
    let world = eng.world();
    let mut arena: Vec<Placed> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    // (node, the arena index of the widget laying it out, if any)
    let mut stack: Vec<(Entity, Option<usize>)> = vec![(eng.root(), None)];
    while let Some((entity, owner)) = stack.pop() {
        let mut next_owner = owner;
        if let Ok(widget) = world.get::<&Widget>(entity) {
            let index = arena.len();
            let widget = Widget::clone(&widget);
            let name = world
                .get::<&balaur_core::scene::Name>(entity)
                .map_or_else(|_| String::new(), |n| n.0.clone());
            arena.push(Placed {
                entity,
                name,
                widget: widget.clone(),
                children: Vec::new(),
            });
            match owner {
                Some(parent) => arena[parent].children.push(index),
                None => roots.push(index),
            }
            // Only a container adopts what is under it; a label with nodes
            // beneath it leaves them to be anchored on their own.
            next_owner = lays_out(&widget.kind).then_some(index);
        }
        if let Ok(children) = world.get::<&Children>(entity) {
            // Pushed in reverse so the stack pops them in declaration order.
            for child in children.0.iter().rev() {
                stack.push((*child, next_owner));
            }
        }
    }
    (arena, roots)
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
    let ox = widget.x * scale;
    let oy = widget.y * scale;
    let pos = match widget.anchor.as_str() {
        "top_right" => pos2(area.max.x - ox, area.min.y + oy),
        "bottom_left" => pos2(area.min.x + ox, area.max.y - oy),
        "bottom_right" => pos2(area.max.x - ox, area.max.y - oy),
        "center" => pos2(area.center().x + ox, area.center().y + oy),
        _ => pos2(area.min.x + ox, area.min.y + oy),
    };
    let align = match widget.anchor.as_str() {
        "top_right" => Align2::RIGHT_TOP,
        "bottom_left" => Align2::LEFT_BOTTOM,
        "bottom_right" => Align2::RIGHT_BOTTOM,
        "center" => Align2::CENTER_CENTER,
        _ => Align2::LEFT_TOP,
    };
    (pos, align)
}

/// Where a root goes and what box it is handed: `fill` takes the surface
/// less its insets so a container at the root fills the screen, a dialog
/// sits in the middle over the dimmed screen, the rest anchor as before.
fn root_frame(
    widget: &Widget,
    area: egui::Rect,
    scale: f32,
) -> (egui::Pos2, Align2, egui::Vec2, egui::Order) {
    if widget.anchor == "fill" {
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
    if widget.kind == "dialog" {
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
    let (placed, roots) = forest(eng);
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
        theme: Rc::new(WidgetTheme::default()),
        assigned: egui::Vec2::ZERO,
        bounds: egui::Vec2::ZERO,
        edits: Vec::new(),
        // An `accept` is a click by another name: same `clicked`, same
        // `on_click`, so it starts the frame's list rather than a second one.
        clicked: accepted.into_iter().collect(),
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
        let entity = placed[root].entity;
        if widget.kind == "dialog" {
            crate::widget_kinds::dialog_backdrop(ctx, entity, area);
        }
        let (pos, align, assigned, order) = root_frame(widget, area, scale);
        painting.assigned = assigned;
        let shown = egui::Area::new(egui::Id::new(("balaur-widget", entity)))
            .order(order)
            .pivot(align)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                if assigned != egui::Vec2::ZERO {
                    ui.set_max_size(assigned);
                }
                draw_one(ui, &mut painting, root);
            });
        painting.assigned = egui::Vec2::ZERO;
        // A root is placed by nobody, so it records its own rect, from egui's
        // memory: the response's rect can lag it by a frame.
        let drawn = ctx
            .memory(|m| m.area_rect(egui::Id::new(("balaur-widget", entity))))
            .unwrap_or(shown.response.rect);
        record_rect(entity, drawn);
    }
    // Published at the end of the draw, not the start of the next one: a
    // script's `draw_ui` runs after this and reads this frame's rects.
    settle_rects();
    roll_measurements();
    let edits = std::mem::take(&mut painting.edits);
    let clicked = std::mem::take(&mut painting.clicked);
    // Only on the change: a handler firing every frame focus merely *stayed*
    // would be a different event, and not a useful one.
    let arrived = (focused != was_focused).then_some(focused).flatten();
    crate::widget_input::record(eng, &clicked, edits, arrived);
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
        Ok(theme) => theme,
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
pub(crate) fn caption(eng: &Engine, widget: &Widget) -> String {
    if widget.text_key.is_empty() {
        return widget.text.clone();
    }
    balaur_core::strings::tr(eng, &widget.text_key, &[])
}

/// Everything a widget kind draws, with the theme already resolved.
fn draw_themed(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let caption = caption(at.eng, widget);
    let scale = at.scale;
    let color = rgba_color(widget.text_color);
    let font = egui::FontId::new(widget.font_size * scale, family("ui"));
    match widget.kind.as_str() {
        "button" => button(ui, at, index, &caption, &font, color),
        // A line the player types into. The text lives on the widget; the
        // draw only reports what was typed, and the next tick writes it.
        "field" => crate::widget_text::field(ui, at, index, &font, color),
        // A dialog is a panel drawn over a dimmed screen; the dimming is the
        // root draw's, so here it is the panel.
        "panel" | "dialog" => panel(ui, at, index, &caption, &font, color),
        "check" => crate::widget_kinds::check(ui, at, index, &caption, &font, color),
        "dropdown" => crate::widget_kinds::dropdown(ui, at, index, &font, color),
        "slider" => crate::widget_kinds::slider(ui, at, index),
        "progress" => crate::widget_kinds::progress(ui, at, index, &caption, &font, color),
        "separator" => crate::widget_kinds::separator(ui, at, index),
        "grid" => crate::widget_kinds::grid(ui, at, index),
        "flow" => crate::widget_kinds::flow(ui, at, index),
        "fold" => crate::widget_kinds::fold(ui, at, index, &caption, &font, color),
        // A picture from the project, sized by what it states or by itself.
        "image" => image(ui, at, index),
        "row" => contain(ui, at, index, Axis::Row),
        "column" => contain(ui, at, index, Axis::Column),
        // A box that clips, with its children free to run past it.
        "scroll" => scroller(ui, at, index),
        // One child at a time, with a strip of the rest above it. The strip is
        // drawn here rather than authored, so adding a page is adding a node.
        "tab" => tabs(ui, at, index),
        // The rect a script fills. The node owns the placement, the script
        // owns everything inside it, and neither has to know the other.
        "draw" => {
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
            // What it takes is its box, or what the script drew where it has
            // none: a `draw` node with no size still has to measure.
            let used = inner.min_rect().size();
            ui.advance_cursor_after_rect(egui::Rect::from_min_size(
                rect.min,
                vec2(
                    if want.x > 0.0 { want.x } else { used.x },
                    if want.y > 0.0 { want.y } else { used.y },
                ),
            ));
        }
        _ => {
            if !crate::widget_text::shaped_label(ui, at, widget, &caption, color) {
                let mut label =
                    egui::Label::new(egui::RichText::new(&caption).font(font).color(color));
                // `extend` is the old behaviour: one line, however wide it runs.
                label = if widget.wrap {
                    label.wrap()
                } else {
                    label.extend()
                };
                ui.with_layout(egui::Layout::top_down(across(&widget.text_align)), |ui| {
                    ui.add(label);
                });
            }
        }
    }
}

/// A pill that reports its click.
fn button(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    caption: &str,
    font: &egui::FontId,
    color: egui::Color32,
) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let style = at.theme.style(&widget.kind);
    let (scale, focused) = (at.scale, at.focused);
    // Without a theme a button is as round as its text is tall,
    // which is the pill the layer has always drawn.
    let radius = egui::CornerRadius::same(
        (style.radius.unwrap_or(widget.font_size) * scale).min(120.0) as u8,
    );
    // The caption is shaped and painted over an empty button, so a button
    // says in Arabic what it says in English.
    let shaped = crate::widget_text::shaped_caption(ui, at, widget, caption);
    let mut button = match &shaped {
        Some((shaped, _)) => egui::Button::new("").min_size(
            (shaped.size + 2.0 * ui.spacing().button_padding)
                .max(vec2(widget.width, widget.height) * scale),
        ),
        None => egui::Button::new(egui::RichText::new(caption).font(font.clone()).color(color))
            .min_size(vec2(widget.width, widget.height) * scale),
    }
    .corner_radius(radius)
    .stroke(Stroke::new(
        style.stroke_width,
        style.stroke.unwrap_or(color),
    ));
    if let Some(fill) = style.fill {
        button = button.fill(fill);
    }
    // A themed picture goes under the button, which then paints nothing of
    // its own; the plate is reserved first so the picture sits below the text.
    let plate = style
        .image
        .as_ref()
        .map(|_| ui.painter().add(egui::Shape::Noop));
    if plate.is_some() {
        button = button.fill(Color32::TRANSPARENT).stroke(Stroke::NONE);
    }
    let response = ui.add(button);
    if let (Some(plate), Some(path)) = (plate, style.image.as_ref()) {
        crate::widget_kinds::nine_patch_plate(
            ui,
            at.eng,
            plate,
            path,
            style.slice,
            response.rect,
            scale,
        );
    }
    if let Some((shaped, texture)) = &shaped {
        let origin = response.rect.center() - shaped.size / 2.0;
        crate::text::paint(ui.painter(), *texture, shaped, origin, color, at.eng.time());
    }
    if response.clicked() {
        at.clicked.push(placed.entity);
    }
    if focused == Some(placed.entity) {
        // Drawn rather than egui's own focus ring: the ring follows
        // egui's keyboard focus, and this follows the scene's.
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            radius,
            Stroke::new(2.0, style.stroke.unwrap_or(color)),
            egui::StrokeKind::Outside,
        );
    }
}

/// Where text sits in the width the widget was given.
pub(crate) fn across(align: &str) -> egui::Align {
    match align {
        "center" => egui::Align::Center,
        "end" => egui::Align::Max,
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
    let style = at.theme.style(&widget.kind);
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
                    .map_or(Stroke::NONE, |c| Stroke::new(style.stroke_width, c)),
                egui::StrokeKind::Inside,
            ),
        );
    }
    ui.advance_cursor_after_rect(background);
}

/// Draw a project image. A source that will not load is reported once and
/// draws nothing: a missing picture must not take the frame down.
fn image(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let widget = &at.arena[index].widget;
    if widget.source.is_empty() {
        return;
    }
    let ctx = ui.ctx().clone();
    match crate::widgets::texture_of(at.eng, &ctx, &widget.source) {
        Ok(texture) => {
            let size = image_size(box_of(widget, at.assigned, at.scale), texture.size_vec2());
            if widget.slice.iter().any(|v| *v > 0.0) {
                // The borders stay the picture's own size; only the middle
                // stretches to the box.
                let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                let shapes = crate::widget_kinds::nine_patch(
                    texture.id(),
                    texture.size_vec2(),
                    rect,
                    widget.slice,
                    at.scale,
                );
                ui.painter().add(egui::Shape::Vec(shapes));
            } else {
                ui.add(egui::Image::new((texture.id(), size)));
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

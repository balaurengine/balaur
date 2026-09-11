//! The widget layer's pass: folds each subtree's theme, moves focus, and
//! draws every widget kind. The component itself is [`super::node::Widget`].

use std::rc::Rc;

use balaur_core::Engine;
use balaur_core::hecs::Entity;
use egui::{Align2, Color32, Stroke, pos2, vec2};
use smol_str::SmolStr;

use crate::vocabulary::words as w;
use crate::widget::arena::{Begun, Look, Placed, begin, keep, look_of, stamp_now};
use crate::widget::arrange::{
    Axis, box_of, contain, hold_to, lay_out, padding_of, record_measure, record_rect,
    roll_measurements, scroller, settle_rects, tabs,
};
use crate::widget::node::{Move, Surface, UiFocus, Widget, WidgetLayerConfig};
use crate::widget::theme::{Style, WidgetTheme, face, styled, theme_of};

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

/// `area` less the part the on-screen keyboard covers. The keyboard is
/// measured in the window's pixels, which are this pass's units.
fn above_keyboard(eng: &Engine, area: egui::Rect) -> egui::Rect {
    let covered = balaur_core::facts::device(eng).keyboard_height;
    let bottom = (area.max.y - covered).max(area.min.y);
    egui::Rect::from_min_max(area.min, egui::pos2(area.max.x, bottom))
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
        theme: theme_root(eng),
        assigned: egui::Vec2::ZERO,
        bounds: egui::Vec2::ZERO,
        edits: Vec::new(),
        rects: crate::widget::taffy::Rects::default(),
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
    crate::widget::taffy::sweep(eng);
    let edits = std::mem::take(&mut painting.edits);
    let clicked = std::mem::take(&mut painting.clicked);
    // Dropped before the arena moves: `Painting` borrows it for the draw.
    drop(painting);
    keep(placed, roots, index_of, stamp);
    // Only on the change: a handler firing every frame focus merely *stayed*
    // would be a different event, and not a useful one.
    let arrived = (focused != was_focused).then_some(focused).flatten();
    crate::widget::input::record(eng, &clicked, edits, arrived);
}

/// Draw one root into the area its surface gives it, and record where it
/// landed. Split from [`draw`] under `MAX_FN_LINES`; the seam is one root's
/// own placement and pass, which needs nothing from the loop around it.
fn draw_root(ctx: &egui::Context, painting: &mut Painting<'_>, root: usize, area: egui::Rect) {
    let (eng, placed, scale) = (painting.eng, painting.arena, painting.scale);
    let entity = placed[root].entity;
    let widget = &placed[root].widget;
    if widget.kind == w::DIALOG {
        crate::widget::kinds::dialog_backdrop(ctx, entity, area);
    }
    let area = if widget.avoid_keyboard {
        above_keyboard(eng, area)
    } else {
        area
    };
    let (pos, align, mut assigned, order) = crate::widget::anchor::root_frame(widget, area, scale);
    // A root spanning one axis states or measures the other.
    if (assigned.x == 0.0) != (assigned.y == 0.0) {
        assigned = measured(eng, ctx, painting, root, area, assigned);
    }
    painting.assigned = assigned;
    painting.rects = place_root(eng, ctx, painting, root, area, (pos, align, assigned));
    // A box of known size is placed by its corner: egui's pivot works from
    // last frame's size, and with none it opens at its default and keeps it.
    let (pos, align) = if assigned == egui::Vec2::ZERO {
        (pos, align)
    } else {
        (align.anchor_size(pos, assigned).min, Align2::LEFT_TOP)
    };
    let mut root_area = egui::Area::new(egui::Id::new(("balaur-widget", entity)))
        .order(order)
        .pivot(align)
        .fixed_pos(pos);
    if assigned != egui::Vec2::ZERO {
        // Egui's first frame otherwise guesses a size and pushes the corner
        // in to keep the guess on screen, and the cursor keeps it there.
        root_area = root_area.default_size(assigned);
    }
    let shown = root_area
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
) -> crate::widget::taffy::Rects {
    let (pos, align, assigned) = frame;
    let hugs = assigned == egui::Vec2::ZERO;
    let space = if hugs {
        crate::widget::taffy::Room::hugging(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            area.size(),
        ))
    } else {
        crate::widget::taffy::Room::fixed(align.anchor_size(pos, assigned))
    };
    let probe = root_ui(ctx);
    let touched = painting.touched.clone();
    let mut rects = crate::widget::taffy::solve(
        eng,
        painting.arena,
        root,
        &probe,
        painting.scale,
        &theme_root(eng),
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

/// A half-known box made whole: the root is measured hugging its content
/// within the axis it spans, and that measure is the axis it did not state.
fn measured(
    eng: &Engine,
    ctx: &egui::Context,
    painting: &mut Painting<'_>,
    root: usize,
    area: egui::Rect,
    assigned: egui::Vec2,
) -> egui::Vec2 {
    let bounds = vec2(
        if assigned.x > 0.0 {
            assigned.x
        } else {
            area.width()
        },
        if assigned.y > 0.0 {
            assigned.y
        } else {
            area.height()
        },
    );
    let space =
        crate::widget::taffy::Room::hugging(egui::Rect::from_min_size(egui::Pos2::ZERO, bounds));
    let touched = painting.touched.clone();
    let rects = crate::widget::taffy::solve(
        eng,
        painting.arena,
        root,
        &root_ui(ctx),
        painting.scale,
        &theme_root(eng),
        &space,
        painting.fresh,
        &touched,
    );
    let size = rects.get(&root).map_or(egui::Vec2::ZERO, egui::Rect::size);
    vec2(
        if assigned.x > 0.0 { assigned.x } else { size.x },
        if assigned.y > 0.0 { assigned.y } else { size.y },
    )
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

/// The theme a root starts from, before it names one of its own: the
/// project's `ui/theme`, or the built-in look.
fn theme_root(eng: &Engine) -> Rc<WidgetTheme> {
    let bare = BARE.with(Rc::clone);
    let project = balaur_core::project::UiSettings::from_settings(eng).theme;
    theme_of(eng, &project, &bare)
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
    pub(crate) rects: crate::widget::taffy::Rects,
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
    /// A window's title bar dragged, in design pixels.
    Moved([f32; 2]),
}

/// Draw one widget and, when it is a container, what is laid out inside it.
///
/// A child's `anchor`, `x` and `y` are ignored: it is placed by its parent,
/// and a menu that moved when you nudged one entry would not be a menu.
pub(crate) fn draw_one(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    if !widget.visible || placed.alpha <= 0.0 {
        return;
    }
    // The node's alpha over its widget parent's, which the parent already
    // applied: `alpha` is inherited, so the ratio is this node's own share.
    let above = placed.parent.map_or(1.0, |p| at.arena[p].alpha);
    let share = if above > 0.0 {
        placed.alpha / above
    } else {
        1.0
    };
    let opacity = ui.opacity();
    ui.multiply_opacity(share);
    // Restored before returning, so a themed subtree does not leak its look
    // onto whatever the caller draws next.
    let outer = at.theme.clone();
    at.theme = theme_of(at.eng, &widget.theme, &outer);
    draw_themed(ui, at, index);
    at.theme = outer;
    ui.set_opacity(opacity);
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
            crate::widget::button::button(ui, at, index, &caption, &font, color);
        }
        // A line the player types into. The text lives on the widget; the
        // draw only reports what was typed, and the next tick writes it.
        w::FIELD => crate::widget::text::field(ui, at, index, &font, color),
        w::TEXT_AREA => crate::widget::text::text_area(ui, at, index, &font, color),
        // A dialog is a panel drawn over a dimmed screen; the dimming is the
        // root draw's, so here it is the panel.
        w::PANEL | w::DIALOG => panel(ui, at, index, &caption, &font, color),
        w::WINDOW => crate::widget::window::window(ui, at, index, &caption, &font, color),
        w::CHECK => crate::widget::kinds::check(ui, at, index, &caption, &font, color),
        w::COLOR => crate::widget::kinds::color(ui, at, index),
        w::DROPDOWN => crate::widget::kinds::dropdown(ui, at, index, &font, color),
        w::MENU => crate::widget::kinds::menu(ui, at, index, &caption, &font, color),
        w::LIST => crate::widget::rows::list(ui, at, index, &font, color),
        w::TREE => crate::widget::rows::tree(ui, at, index, &font, color),
        w::TABLE => crate::widget::rows::table(ui, at, index, &font, color),
        // The file being edited, with the gutter and the colouring the script
        // call has always had.
        w::CODE => crate::widget::kinds::code(ui, at, index),
        w::SLIDER => crate::widget::kinds::slider(ui, at, index),
        w::DRAG_VALUE => crate::widget::kinds::drag_value(ui, at, index, &font, color),
        w::PROGRESS => crate::widget::kinds::progress(ui, at, index, &caption, &font, color),
        w::SEPARATOR => crate::widget::kinds::separator(ui, at, index),
        w::GRID => crate::widget::kinds::grid(ui, at, index),
        w::FLOW => crate::widget::kinds::flow(ui, at, index),
        w::FOLD => crate::widget::kinds::fold(ui, at, index, &caption, &font, color),
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
            if !crate::widget::text::shaped_label(ui, at, index, widget, &caption, color, &font) {
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
        crate::widget::kinds::nine_patch_plate(
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
                let shapes = crate::widget::kinds::nine_patch(
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

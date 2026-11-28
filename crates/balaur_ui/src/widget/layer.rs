//! The widget layer's pass: folds each subtree's theme, moves focus, and
//! draws every widget kind. The component itself is [`super::node::Widget`].

use std::rc::Rc;

use balaur_core::Engine;
use balaur_core::hecs::Entity;
use egui::{Align2, Color32, pos2, vec2};
use smol_str::SmolStr;

use crate::vocabulary::words as w;
use crate::widget::arena::{
    Begun, Look, Placed, begin, keep, look_of, screen_of, stamp_now, takes_pointer,
};
use crate::widget::arrange::{
    Axis, contain, drawn_at, hold_to, lay_out_within, padding_of, placing_at, record_rect,
    roll_measurements, scroller, settle_rects, solved_of, tabs,
};
use crate::widget::focus::{
    advance, focus_before, focus_moved, focus_stops, keyboard_move, keyboard_side, reachable,
    shortcuts,
};
use crate::widget::node::{UiFocus, WidgetLayerConfig};
use crate::widget::paint::Brush;
use crate::widget::theme::{Pointer, Style, WidgetState, WidgetTheme, face, rich, theme_of};

const BALAUR_WIDGET: &str = "balaur-widget";

/// `area` less the part the on-screen keyboard covers. The keyboard is
/// measured in the window's pixels, which are this pass's units.
fn above_keyboard(facts: &balaur_core::facts::DeviceFacts, area: egui::Rect) -> egui::Rect {
    // The fact is physical pixels, because that is what a keyboard covers,
    // and a layout is design pixels.
    let covered = facts.keyboard_height / facts.ui_scale.max(f32::EPSILON);
    let bottom = (area.max.y - covered).max(area.min.y);
    egui::Rect::from_min_max(area.min, egui::pos2(area.max.x, bottom))
}

/// The surface less what a notch, a status bar or a home bar covers.
///
/// The insets are a fact about the display, so they are the same whichever
/// surface a root draws on: a root filling the screen loses them all, and one
/// in a viewport that the notch does not reach loses nothing.
pub(crate) fn inside_safe_area(
    facts: &balaur_core::facts::DeviceFacts,
    area: egui::Rect,
    edges: [bool; 4],
) -> egui::Rect {
    let per_point = facts.ui_scale.max(f32::EPSILON);
    let mut cover = facts.safe_area.map(|edge| edge / per_point);
    for (inset, keep) in cover.iter_mut().zip(edges) {
        if !keep {
            *inset = 0.0;
        }
    }
    let [left, top, right, bottom] = cover;
    // A display with no insets takes nothing off, whatever size it last reported.
    if cover.iter().all(|inset| *inset <= 0.0) {
        return area;
    }
    let screen = facts.design_size();
    // A device that has not reported its size yet answers zero, and an
    // intersection with that is an empty root rather than an uninset one.
    if screen[0] <= 0.0 || screen[1] <= 0.0 {
        return area;
    }
    let safe = egui::Rect::from_min_max(
        egui::pos2(left, top),
        egui::pos2(screen[0] - right, screen[1] - bottom),
    );
    area.intersect(safe)
}

/// A `fill` root's own colour over the part of its surface it keeps its
/// children out of, so a screen's background reaches the edge of the display.
fn paint_to_the_edge(ui: &mut egui::Ui, at: &mut Painting<'_>, root: usize, kept: egui::Rect) {
    let outer = at.theme.clone();
    at.theme = theme_of(at.eng, &at.arena[root].widget.theme, &outer);
    let fill = at.style(root).fill;
    at.theme = outer;
    let Some(fill) = fill else {
        return;
    };
    let edge = at.edge_to_edge;
    let strips = [
        egui::Rect::from_min_max(edge.min, pos2(edge.max.x, kept.min.y)),
        egui::Rect::from_min_max(pos2(edge.min.x, kept.max.y), edge.max),
        egui::Rect::from_min_max(pos2(edge.min.x, kept.min.y), pos2(kept.min.x, kept.max.y)),
        egui::Rect::from_min_max(pos2(kept.max.x, kept.min.y), pos2(edge.max.x, kept.max.y)),
    ];
    for strip in strips.into_iter().filter(egui::Rect::is_positive) {
        Brush::of(ui).fill(strip, egui::CornerRadius::ZERO, fill);
    }
}

/// Every visible root, each on its screen: the window, or the `viewport` it
/// sits in, as that viewport was drawn this pass. A viewport drawn at no size
/// draws nothing in it.
fn draw_roots(ctx: &egui::Context, painting: &mut Painting<'_>, roots: &[usize]) {
    let (placed, window) = (painting.arena, painting.surface);
    for &root in roots {
        if !placed[root].widget.visible {
            continue;
        }
        let area = match placed[root].screen {
            None => window,
            Some(viewport) => match screen_of(placed, viewport, placing_at) {
                Some(rect) => rect,
                None => continue,
            },
        };
        draw_root(ctx, painting, root, area, placed[root].screen.is_some());
    }
}

/// Draw every widget entity. Runs inside the frame's egui pass.
pub(crate) fn draw(eng: &Engine, ctx: &egui::Context) {
    let Some(layer) = eng.try_resource::<WidgetLayerConfig>() else {
        return;
    };
    let keyboard = layer.borrow().keyboard;
    let screen = ctx.viewport_rect();
    crate::widget::input::follow_hearers(eng);
    let stamp = stamp_now(eng);
    let Begun {
        placed,
        roots,
        index_of,
        fresh,
        touched,
    } = begin(eng, stamp);
    // Nothing to draw and nothing to focus: a scene with no widgets pays for
    // the resource lookup and no more, and a reader hears an empty window.
    if placed.is_empty() {
        crate::access::written(eng, ctx, &[], &[], &[]);
        crate::widget::shortcuts::record_alone(eng, ctx);
        return;
    }
    let was_focused = eng
        .try_resource::<UiFocus>()
        .and_then(|f| f.borrow().focused);
    // A field being typed into owns the keys, arrows included.
    let keys = keyboard && !ctx.egui_wants_keyboard_input();
    let asked = keys.then(|| keyboard_move(ctx)).flatten();
    let side = keys.then(|| keyboard_side(ctx)).flatten();
    // Last pass's rects: the keyboard moves before this pass draws anything.
    let live = |root: usize| {
        placed[root]
            .screen
            .is_none_or(|viewport| screen_of(&placed, viewport, drawn_at).is_some())
    };
    let shown = reachable(&placed, &roots, &live);
    let stops = focus_stops(&placed, &shown);
    let accepted = advance(eng, ctx, &stops, (asked, side));
    let turned = turned_fold(&placed, accepted);
    // A chord is a click by another name, as an `accept` is.
    let fired = shortcuts(ctx, &placed, &shown);
    let shut = crate::widget::shortcuts::shut(&placed);
    let pressed = crate::widget::shortcuts::pressed(eng, ctx, &index_of, &shown, &shut);
    let (focused, taking, arriving) = focus_before(eng);
    let mut painting = Painting {
        eng,
        ctx: ctx.clone(),
        arena: &placed,
        focused,
        taking,
        arrived: focused.filter(|_| arriving),
        theme: theme_root(eng),
        assigned: egui::Vec2::ZERO,
        bounds: egui::Vec2::ZERO,
        surface: screen,
        edge_to_edge: screen,
        facts: balaur_core::facts::device(eng),
        edits: turned.into_iter().collect(),
        rects: crate::widget::taffy::Rects::default(),
        fresh,
        touched: Written::new(touched),
        // An `accept` is a click by another name: same `clicked`, same
        // `pointer_click`, so it starts the frame's list rather than a second.
        clicked: accepted.into_iter().chain(fired).collect(),
        state: WidgetState::default(),
        context_opened: false,
        hits: Vec::new(),
        root_layers: Vec::new(),
        under: Vec::new(),
        shown: Vec::new(),
    };
    draw_roots(ctx, &mut painting, &roots);
    crate::widget::window::escape_confined(ctx, &mut painting);
    settle(eng, ctx);
    crate::access::written(eng, ctx, &placed, &roots, &shown);
    let mut edits = std::mem::take(&mut painting.edits);
    edits.extend(
        pressed
            .into_iter()
            .map(|(node, name)| (node, Edit::Shortcut(name))),
    );
    let clicked = std::mem::take(&mut painting.clicked);
    edits.extend(crate::widget::input::pass_edits(eng, ctx, &painting));
    crate::routing::widgets_drew(
        eng,
        std::mem::take(&mut painting.hits),
        std::mem::take(&mut painting.root_layers),
    );
    // Dropped before the arena moves: `Painting` borrows it for the draw.
    drop(painting);
    keep(placed, roots, index_of, stamp);
    let arrived = focus_moved(eng, was_focused, taking, &mut edits);
    crate::widget::input::record(eng, &clicked, edits, arrived);
}

/// What the draw leaves for the next one: this frame's rects published, at
/// the end of the draw rather than the start of the next one, so a script
/// reads them a frame behind at most; the boxes that moved and a paste no
/// field took said.
fn settle(eng: &Engine, ctx: &egui::Context) {
    settle_rects();
    crate::widget::arrange::report_resized(eng);
    crate::widget::input::hear_paste(eng, ctx);
    roll_measurements();
    crate::widget::taffy::sweep(eng);
}

/// The egui area a root draws in: at `pos` by `align`, at its `order`, kept
/// to `clip` where it sits in a viewport.
fn root_area(
    root: &Placed,
    (pos, align, assigned, order): (egui::Pos2, Align2, egui::Vec2, egui::Order),
    clip: Option<egui::Rect>,
) -> egui::Area {
    let mut area = egui::Area::new(egui::Id::unique((BALAUR_WIDGET, root.entity)))
        .order(order)
        .pivot(align)
        .fixed_pos(pos)
        .interactable(takes_pointer(root));
    if assigned != egui::Vec2::ZERO {
        // Egui's first frame otherwise guesses a size and pushes the corner
        // in to keep the guess on screen, and the cursor keeps it there.
        area = area.default_size(assigned);
    }
    if let Some(clip) = clip {
        area = area.constrain_to(clip);
    }
    // A widget appears when the scene says so, at the alpha its own theme
    // sets, unless it asks for egui's fade.
    area.fade_in(root.widget.egui.fade_in)
}

/// Draw one root into the area its screen gives it, and record where it
/// landed. Split from [`draw`] under `MAX_FN_LINES`; the seam is one root's
/// own placement and pass, which needs nothing from the loop around it.
fn draw_root(
    ctx: &egui::Context,
    painting: &mut Painting<'_>,
    root: usize,
    area: egui::Rect,
    confined: bool,
) {
    let (eng, placed) = (painting.eng, painting.arena);
    let entity = placed[root].entity;
    let widget = &placed[root].widget;
    // A dialog is shut by the scene, by Escape or by a click on the dim
    // behind it; shut, it is not drawn and nothing is kept out.
    let modal = widget.kind == w::DIALOG;
    if modal && !widget.open {
        return;
    }
    if modal {
        painting.shown.push(entity);
    }
    // A `fill` root with nothing in it is a backdrop, which reaches the edge;
    // one with children keeps them clear of the notch and the keyboard.
    let fills = widget.anchor == w::FILL;
    painting.edge_to_edge = area;
    // A root in a viewport is kept to it, and so is everything it draws.
    let clip = confined.then_some(area);
    let area = if fills && placed[root].children.is_empty() {
        area
    } else {
        above_keyboard(
            &painting.facts,
            inside_safe_area(&painting.facts, area, widget.safe_area),
        )
    };
    let bled = fills && area != painting.edge_to_edge;
    painting.surface = area;
    let (pos, align, mut assigned, order) = crate::widget::anchor::root_frame(widget, area);
    // A mirrored UI anchors to the mirrored corner: `top_left` is the top
    // right in Arabic, and `x` runs in from that edge.
    let flip = crate::widget::direction::mirrored_above(placed, root);
    let pos = egui::pos2(
        crate::widget::direction::placed_x(flip, pos.x, 0.0, area.x_range()),
        pos.y,
    );
    let align = crate::widget::direction::flip_align2(align, flip);
    // Folded, a window is as tall as its bar whatever height it states.
    if widget.kind == w::WINDOW && crate::widget::window::folded(entity, widget) {
        assigned.y = 0.0;
    }
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
    let area_id = egui::Id::unique((BALAUR_WIDGET, entity));
    let layer = egui::LayerId::new(order, area_id);
    painting.root_layers.push(layer);
    let root_area = root_area(&placed[root], (pos, align, assigned, order), clip);
    let dragged_by_box = widget.egui.movable && widget.kind != w::WINDOW;
    let fill = |ui: &mut egui::Ui, painting: &mut Painting<'_>| {
        crate::widget::popups::confine(ui, clip);
        // A root handed a box reserves it before anything draws: its
        // children are placed at absolute rects and report nothing
        // back, so the area would otherwise hug the first of them.
        if assigned != egui::Vec2::ZERO {
            ui.set_max_size(assigned);
            ui.advance_cursor_after_rect(egui::Rect::from_min_size(pos, assigned));
        }
        if bled {
            paint_to_the_edge(ui, painting, root, area);
        }
        if dragged_by_box {
            drag_root(ui, painting, root);
        }
        draw_one(ui, painting, root);
    };
    let first_pass = crate::widget::anchor::first_pass(
        ctx,
        (area_id, layer, (pos, align)),
        modal,
        painting.rects.get(&root),
        takes_pointer(&placed[root]),
    );
    let shown = if let (true, Some(screen)) = (modal, clip) {
        crate::widget::window::dialog_in(ctx, painting, (root, screen, layer), root_area, fill)
    } else if modal {
        // egui's own modal: it dims the screen, holds the layer above every
        // other, keeps the pointer out of what is behind, and says when
        // Escape or the dim was what asked to close.
        let dismissable = widget.egui.dismissable;
        let dim = crate::widget::window::dim(eng, painting, root);
        let shut = egui::Modal::new(egui::Id::unique(("balaur-dialog", entity)))
            .area(root_area)
            .frame(egui::Frame::NONE)
            .backdrop_color(dim)
            .show(ctx, |ui| fill(ui, painting));
        if shut.should_close() && dismissable {
            painting.edits.push((entity, Edit::Open(false)));
        }
        shut.response
    } else {
        root_area.show(ctx, |ui| fill(ui, painting)).response
    };
    let unlaid = crate::widget::arena::take_lines_unlaid();
    if first_pass || unlaid {
        ctx.request_discard("a root's first pass, or a line read before its room was laid");
    }
    painting.assigned = egui::Vec2::ZERO;
    // A root is placed by nobody, so it records its own rect, from egui's
    // memory: the response's rect can lag it by a frame.
    let drawn = ctx.memory(|m| m.area_rect(area_id)).unwrap_or(shown.rect);
    record_rect(entity, drawn);
}

/// A `movable` root that is not a window: a drag on its box where no child
/// takes the press moves it. Sensed before the children are drawn, so theirs
/// sit on top and keep their own presses.
fn drag_root(ui: &egui::Ui, painting: &mut Painting<'_>, root: usize) {
    let (entity, widget) = (painting.arena[root].entity, &painting.arena[root].widget);
    let Some(rect) = painting.rects.get(&root).copied() else {
        return;
    };
    let held = ui.interact(
        rect,
        egui::Id::unique(("balaur-root-drag", entity)),
        egui::Sense::drag(),
    );
    if held.dragged() {
        let moved = crate::widget::window::constrained(painting, widget, rect, held.drag_delta());
        painting
            .edits
            .push((entity, Edit::Moved([moved.x, moved.y])));
    }
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
    let touched = painting.touched.first_push();
    let mut rects = crate::widget::taffy::solve(
        eng,
        painting.arena,
        root,
        &probe,
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
    let touched = painting.touched.first_push();
    let rects = crate::widget::taffy::solve(
        eng,
        painting.arena,
        root,
        &root_ui(ctx),
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
        egui::Id::unique("balaur-layout-probe"),
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
pub(crate) fn theme_root(eng: &Engine) -> Rc<WidgetTheme> {
    let bare = BARE.with(Rc::clone);
    theme_of(eng, &crate::theme::for_widgets(eng), &bare)
}

/// What one draw pass carries down the widget tree.
///
/// A struct rather than six arguments: the recursion is three functions deep
/// and every one of them was growing a parameter per feature.
pub(crate) struct Painting<'a> {
    pub(crate) eng: &'a Engine,
    /// The pass's context, which eases a widget's change of state.
    pub(crate) ctx: egui::Context,
    pub(crate) arena: &'a [Placed],
    pub(crate) focused: Option<Entity>,
    /// Whether focus was put on `focused` this pass rather than resting there.
    pub(crate) taking: bool,
    /// The widget focus came to this pass by a key, a pad or a script, which
    /// the scroll holding it brings into view.
    pub(crate) arrived: Option<Entity>,
    /// The theme in force here, inherited unless a widget names its own.
    pub(crate) theme: Rc<WidgetTheme>,
    /// The box the parent container handed this widget, per axis; 0 on an
    /// axis the parent left free, and on both for a root.
    pub(crate) assigned: egui::Vec2,
    /// The box the container now laying out children holds, per axis; 0 where
    /// it is free to grow, in which case children hug rather than fill.
    pub(crate) bounds: egui::Vec2,
    /// The surface the root being drawn sits on, which a `constrain`ed drag
    /// keeps it inside.
    pub(crate) surface: egui::Rect,
    /// That root's surface before the notch and the keyboard are taken off,
    /// which is how far its own colour reaches.
    pub(crate) edge_to_edge: egui::Rect,
    /// The display, read once for the frame's roots.
    pub(crate) facts: balaur_core::facts::DeviceFacts,
    pub(crate) clicked: Vec<Entity>,
    pub(crate) edits: Vec<(Entity, Edit)>,
    /// Where this pass drew what takes the pointer: every widget that does
    /// not let it through and every seam, clipped to what shows.
    pub(crate) hits: Vec<egui::Rect>,
    /// The egui layer each root drew on, whose points `hits` answers for.
    pub(crate) root_layers: Vec<egui::LayerId>,
    /// Where the layout pass put every widget in the subtree being drawn.
    pub(crate) rects: crate::widget::taffy::Rects,
    /// Whether the arena was rebuilt this pass. False means the tree taffy
    /// holds already describes it, so a solve restyles the root and no more.
    pub(crate) fresh: bool,
    /// The slots re-read this pass.
    pub(crate) touched: Written,
    /// Whether the pointer is over the widget being drawn, and whether it is
    /// held there. Set by the draw and never by the measure: a size that
    /// followed the pointer would move whatever sits beside it.
    /// What the widget being drawn is being, for the theme's state tables.
    pub(crate) state: WidgetState,
    /// Whether a widget already opened its `context` menu for this pass's
    /// press. Children draw before the parent asks, so the innermost one
    /// under the pointer takes it.
    pub(crate) context_opened: bool,
    /// Every widget the pointer is over this pass, outermost first.
    pub(crate) under: Vec<Entity>,
    /// Every widget whose popup, dialog or window is up this pass.
    pub(crate) shown: Vec<Entity>,
}

/// The slots a pass re-read. The first solve pushes them into taffy, whose
/// tree every solve shares, so once is enough; they stay for
/// [`Painting::deep`].
pub(crate) struct Written {
    slots: Vec<usize>,
    pushed: bool,
}

impl Written {
    fn new(slots: Vec<usize>) -> Self {
        Self {
            slots,
            pushed: false,
        }
    }

    /// This pass's writes the first time a solve asks, nothing after.
    fn first_push(&mut self) -> Vec<usize> {
        if std::mem::replace(&mut self.pushed, true) {
            Vec::new()
        } else {
            self.slots.clone()
        }
    }
}

impl Painting<'_> {
    /// Note that `rect` takes the pointer: a widget that does not let it
    /// through, or a seam. A rect scrolled or clipped out of sight takes none.
    pub(crate) fn take_pointer(&mut self, rect: egui::Rect) {
        if rect.is_positive() {
            self.hits.push(rect);
        }
    }

    /// Whether a subtree has to be walked again rather than taken as taffy
    /// already holds it: because the whole arena was rebuilt, or because its
    /// root was written, whose look its children inherit. A write below the
    /// root was pushed with its own subtree by the pass's first solve. A kind
    /// that places its own children solves them apart from the root, so it
    /// has to ask.
    pub(crate) fn deep(&self, root: usize) -> bool {
        self.fresh || self.touched.slots.contains(&root)
    }

    /// Where the pointer is for the widget at `index`, which answers with the
    /// response it drew: where its `state` holds it, else where `response` says.
    pub(crate) fn pointer(&self, index: usize, response: &egui::Response) -> Pointer {
        let widget = &self.arena[index].widget;
        let free = (!widget.pointer_through).then(|| Pointer::of(response));
        held_pointer(&widget.state).or(free).unwrap_or_default()
    }

    /// The style the widget at `index` is drawn with, in the state it is in here.
    pub(crate) fn style(&self, index: usize) -> Rc<Style> {
        Rc::clone(&self.look(index).style)
    }

    /// Whether egui slants the widget at `index`'s text itself, for want of
    /// an italic face: see [`crate::widget::theme::slanted`].
    pub(crate) fn slant(&self, index: usize) -> bool {
        let look = look_of(self.arena, index, &self.theme);
        crate::widget::theme::slanted(&look.style, (&self.arena[index].widget, look.project))
    }

    /// The look of the widget at `index`, in the state it is in here.
    pub(crate) fn look(&self, index: usize) -> Rc<Look> {
        let look = look_of(self.arena, index, &self.theme);
        if look.style.states_nothing() {
            return look;
        }
        // Partway through a change it is made for this pass and kept nowhere.
        if let Some(style) =
            self.easing(index, &look.style, self.state, crate::widget::ease::BY_BOX)
        {
            return self.wearing(index, &look, style);
        }
        if !self.state.any() {
            return look;
        }
        let placed = &self.arena[index];
        if let Some((_, held)) = placed
            .look
            .borrow()
            .in_state
            .iter()
            .find(|(state, _)| *state == self.state)
        {
            return Rc::clone(held);
        }
        let made = self.wearing(index, &look, look.style.in_states(self.state));
        placed
            .look
            .borrow_mut()
            .in_state
            .push((self.state, Rc::clone(&made)));
        made
    }

    /// `look` with `style` in place of its own, and the face that follows.
    fn wearing(&self, index: usize, look: &Look, style: Style) -> Rc<Look> {
        let placed = &self.arena[index];
        let style = Rc::new(style);
        let (ink, font) = face(&self.theme, &style, (&placed.widget, placed.project));
        Rc::new(Look {
            style,
            font,
            ink,
            icon: look.icon.clone(),
            icon_font: look.icon_font.clone(),
            project: placed.project,
            under: look.under.clone(),
        })
    }

    /// The look with no state on it, for a kind that answers the pointer with
    /// its own response rather than with the box the layout gave it.
    pub(crate) fn resting(&self, index: usize) -> Rc<Look> {
        look_of(self.arena, index, &self.theme)
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
    /// A row of a `list`, `tree` or `table` clicked, and everything the click
    /// leaves picked -- empty where the widget holds one row at a time.
    Picked(String, Vec<String>),
    /// A `table`'s column shares, as a drag on a seam left them.
    Widths(Vec<f32>),
    /// The column a `table`'s header was clicked to sort by, and whether that
    /// click asked for the other way round.
    Sorted(String, bool),
    /// A row dragged over another: the row moved, the row it landed on, and
    /// whether it went before it, after it, or into it.
    Dropped(String, String, String),
    /// A mark at the right of a row clicked: the row, and the mark's name.
    Marked(String, String),
    /// A card a drag carried out of its `list` and let go there.
    Carried(String),
    /// A swatch's colour.
    Color([f32; 4]),
    /// A window's title bar dragged, in design pixels.
    Moved([f32; 2]),
    /// The target of a `[url]` span that was clicked.
    Link(String),
    /// The 1-based line a click on a `code` widget's gutter landed on.
    Gutter(i64),
    /// Where a click on a `view` landed, in the window's design pixels.
    ViewClicked([f32; 2]),
    /// Where a `code` widget's caret moved to: its point in design pixels,
    /// and its index in characters.
    Caret(f32, f32, usize),
    /// The pointer came over the widget, or left it.
    Entered,
    Left,
    /// A button went down or came up over the innermost widget, by name.
    Pressed(String),
    Released(String),
    /// The primary button clicked twice over the innermost widget.
    DoubleClicked,
    /// Focus left the widget.
    Blurred,
    /// A slider's, number field's or colour picker's value, once the drag or
    /// the typing that changed it is over.
    Committed(f32),
    ColorCommitted([f32; 4]),
    /// A row double-clicked in a `list`, `tree` or `table`.
    Activated(String),
    /// A tree row's caret clicked: the row, and whether it is open now.
    Folded(String, bool),
    /// A popup, dialog or window the widget holds came up, or went away.
    Opened,
    Closed,
    /// Where a `scroll` is scrolled to now.
    Scrolled([f32; 2]),
    /// A `window`'s close button was pressed.
    CloseRequested,
    /// A row of a node's `shortcuts` was pressed, by its name.
    Shortcut(String),
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
/// Everything a widget kind draws, with the theme already resolved.
fn draw_themed(ui: &mut egui::Ui, at: &mut Painting<'_>, index: usize) {
    let disabled = at.arena[index].widget.disabled;
    let focused = at
        .eng
        .try_resource::<crate::UiFocus>()
        .is_some_and(|focus| focus.borrow().focused == Some(at.arena[index].entity));
    // A widget the pointer passes through (`interactive = false`) is never
    // hovered or held, so it wears no `hover` or `active` look either: a
    // badge on a card stays the badge while the card answers the pointer.
    let through = at.arena[index].widget.pointer_through;
    let (hovered, held) = pointer_state(ui, disabled || through);
    if hovered {
        at.under.push(at.arena[index].entity);
    }
    let pointer = if held {
        Pointer::Held
    } else if hovered {
        Pointer::Over
    } else {
        Pointer::Away
    };
    let state = held_in(
        &at.arena[index].widget.state,
        WidgetState {
            pointer,
            disabled,
            focused,
            checked: at.arena[index].widget.checked,
        },
    );
    let outer = std::mem::replace(&mut at.state, state);
    let brush = Brush::of(ui);
    let mark = brush.mark();
    crate::widget::kinds::context_sensor(ui, at, index);
    click_sensor(ui, at, index);
    draw_kind(ui, at, index);
    let look = at.look(index);
    // A state's own `outline` rings it as focus does: a picked card's.
    if state.focused || look.style.states_ring() != at.resting(index).style.states_ring() {
        crate::widget::ease::focus_ring(ui, &look.style);
    }
    // Moved and scaled once drawn, children and all, so the layout and what
    // the pointer reaches stay where they were: CSS's `translate`, `scale`.
    let about = ui.max_rect().center();
    let flip = crate::widget::direction::mirrored_above(at.arena, index);
    brush.transform_since(mark, &look.style, (&at.arena[index].widget, flip), about);
    crate::widget::kinds::context_menu(ui, at, index);
    at.state = outer;
}

/// Where a widget's `state` holds the pointer, whatever it does: only the
/// look changes, never what a click does. `None` leaves it to the pointer.
pub(crate) fn held_pointer(word: &str) -> Option<Pointer> {
    use crate::vocabulary::states as st;
    match word {
        st::HOVER => Some(Pointer::Over),
        st::ACTIVE => Some(Pointer::Held),
        st::FOCUS => Some(Pointer::Away),
        _ => None,
    }
}

/// The state a widget's `state` holds it in, over the one the pointer and
/// the keyboard put it in.
fn held_in(word: &str, live: WidgetState) -> WidgetState {
    held_pointer(word).map_or(live, |pointer| WidgetState {
        pointer,
        focused: word == crate::vocabulary::states::FOCUS,
        ..live
    })
}

/// Accept on a fold turns it, as a click on its header does.
fn turned_fold(placed: &[Placed], accepted: Option<Entity>) -> Option<(Entity, Edit)> {
    let entity = accepted?;
    let fold = placed
        .iter()
        .find(|p| p.entity == entity && p.widget.kind == w::FOLD)?;
    Some((entity, Edit::Open(!fold.widget.open)))
}

/// A click on a widget that draws nothing clickable of its own, when its
/// `pointer_click` is heard. Sensed before the kind draws, so a child
/// drawn inside it still takes its own clicks first.
fn click_sensor(ui: &egui::Ui, at: &mut Painting<'_>, index: usize) {
    let placed = &at.arena[index];
    let widget = &placed.widget;
    let passive = matches!(
        widget.kind.as_str(),
        w::LABEL
            | w::PANEL
            | w::ROW
            | w::COLUMN
            | w::GRID
            | w::STACK
            | w::SCROLL
            | w::PROGRESS_BAR
            | w::VIEWPORT
    );
    if !passive || !widget.hears_click || widget.disabled || widget.pointer_through {
        return;
    }
    let id = sensor_id(ui, "balaur-click", placed.entity);
    if ui
        .interact(ui.max_rect(), id, egui::Sense::click())
        .clicked()
    {
        at.clicked.push(placed.entity);
    }
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
    let look = at.look(index);
    let caption = crate::widget::text::shown_caption(at.eng, widget, &look.style);
    let (color, font) = (look.ink, look.font.clone());
    // A caption the theme hides is the tooltip, where none is stated.
    let mut tooltip = crate::widget::text::tooltip_of(at.eng, widget);
    if tooltip.is_empty() && look.style.show_text == Some(false) {
        tooltip = crate::widget::text::caption(at.eng, widget);
    }
    let entity = placed.entity;
    // The pointer over it: the node's own word, else the theme's, none off.
    let themed = look.style.cursor.as_deref();
    let cursor = match (widget.cursor.is_empty(), widget.disabled) {
        (false, _) => widget.cursor.clone(),
        (true, true) => SmolStr::default(),
        (true, false) => themed.map_or_else(SmolStr::default, SmolStr::new),
    };
    let through = widget.pointer_through || widget.kind == w::VIEWPORT;
    if widget.disabled {
        ui.disable();
    }
    match widget.kind.as_str() {
        w::BUTTON => {
            crate::widget::button::button(ui, at, index, &caption, &font, color);
        }
        // A line the player types into. The text lives on the widget; the
        // draw only reports what was typed, and the next tick writes it.
        w::TEXT_FIELD => crate::widget::text::field(ui, at, index, &font, color),
        w::TEXT_AREA => crate::widget::text::text_area(ui, at, index, &font, color),
        // A dialog is a panel drawn over a dimmed screen; the dimming is the
        // root draw's, so here it is the panel.
        w::PANEL | w::DIALOG => panel(ui, at, index, &caption, &font, color),
        w::WINDOW => crate::widget::window::window(ui, at, index, &caption, &font, color),
        // egui draws these three from its own palette: the kind's table, in
        // the state the widget is in, is told to it first, as a field's is.
        w::CHECKBOX => {
            crate::widget::theme::dress(ui, &look.style, color);
            crate::widget::toggles::check(ui, at, index, &caption, &font, color);
        }
        w::SWITCH => crate::widget::toggles::switch(ui, at, index, &caption, &font, color),
        w::SPINNER => crate::widget::numbers::spinner(ui, at, index, color),
        w::COLOR_PICKER => crate::widget::kinds::color(ui, at, index),
        w::DROPDOWN => {
            crate::widget::theme::dress(ui, &look.style, color);
            crate::widget::kinds::dropdown(ui, at, index, &font, color);
        }
        w::MENU => {
            crate::widget::theme::dress(ui, &look.style, color);
            crate::widget::kinds::menu(ui, at, index, &caption, &font, color);
        }
        w::LIST | w::TREE | w::TABLE => rows_down(ui, at, index, &font, color),
        // The file being edited, with the gutter and the colouring the script
        // call has always had.
        w::CODE => crate::widget::kinds::code(ui, at, index),
        w::SLIDER => crate::widget::numbers::slider(ui, at, index),
        w::NUMBER_FIELD => crate::widget::numbers::drag_value(ui, at, index, &font, color),
        w::PROGRESS_BAR => crate::widget::numbers::progress(ui, at, index, &caption, &font, color),
        w::SEPARATOR => crate::widget::kinds::separator(ui, at, index),
        w::STACK => crate::widget::kinds::stack(ui, at, index),
        // A screen for what sits in it: those draw as roots of their own.
        w::VIEWPORT => {
            let size = solved_of(&at.arena[index].widget, &at.style(index), at.assigned);
            ui.allocate_exact_size(size, egui::Sense::hover());
        }
        // A camera's picture, rendered into its own texture before the
        // frame is presented, across the box the layout gives it.
        w::VIEW => crate::widget::view::view(ui, at, index),
        w::FOLD => crate::widget::kinds::fold(ui, at, index, &caption, &font, color),
        // A picture from the project, sized by what it states or by itself.
        w::IMAGE => crate::widget::picture::image(ui, at, index),
        // Taffy placed a grid's children like a row's.
        w::ROW | w::GRID => contain(ui, at, index, Axis::Row),
        w::COLUMN => contain(ui, at, index, Axis::Column),
        // A box that clips, with its children free to run past it.
        w::SCROLL => scroller(ui, at, index),
        // One child at a time, with a strip of the rest above it. The strip is
        // drawn here rather than authored, so adding a page is adding a node.
        w::TABS => tabs(ui, at, index),
        _ => crate::widget::text::label(ui, at, index, (&caption, &font, color), &look.style),
    }
    tip(ui, entity, &tooltip);
    under_pointer(ui, at, &cursor, through);
}

/// A `list`, `tree` or `table`, its rows running down whatever way the parent
/// runs: in a row, a list's lines and cards were laid side by side.
fn rows_down(
    ui: &mut egui::Ui,
    at: &mut Painting<'_>,
    index: usize,
    font: &egui::FontId,
    color: Color32,
) {
    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
        match at.arena[index].widget.kind.as_str() {
            w::LIST => crate::widget::rows::list(ui, at, index, font, color),
            w::TREE => crate::widget::rows::tree(ui, at, index, font, color),
            _ => crate::widget::table::table(ui, at, index, font, color),
        }
    });
}

/// What the pointer meets over the widget just drawn: whether it takes the
/// pointer or lets it through, and the widget's `cursor` shape.
fn under_pointer(ui: &egui::Ui, at: &mut Painting<'_>, cursor: &str, through: bool) {
    if !through {
        at.take_pointer(ui.clip_rect().intersect(ui.min_rect()));
    }
    if !ui.rect_contains_pointer(ui.min_rect()) {
        return;
    }
    if let Some(icon) = pointer_icon(cursor) {
        ui.ctx().set_cursor_icon(icon);
    }
}

/// The platform pointer a `cursor` word names; the arrow is what the pointer
/// already is, and an unknown word is nothing.
pub(crate) fn pointer_icon(cursor: &str) -> Option<egui::CursorIcon> {
    use crate::vocabulary::words::cursor as c;
    use egui::CursorIcon as C;
    Some(match cursor {
        c::NONE => C::None,
        c::HAND => C::PointingHand,
        c::TEXT => C::Text,
        c::VERTICAL_TEXT => C::VerticalText,
        c::CROSS => C::Crosshair,
        c::CELL => C::Cell,
        c::WAIT => C::Wait,
        c::PROGRESS => C::Progress,
        c::HELP => C::Help,
        c::CONTEXT_MENU => C::ContextMenu,
        c::MOVE => C::Move,
        c::GRAB => C::Grab,
        c::GRABBING => C::Grabbing,
        c::ALIAS => C::Alias,
        c::COPY => C::Copy,
        c::NO_DROP => C::NoDrop,
        c::FORBIDDEN => C::NotAllowed,
        c::ALL_SCROLL => C::AllScroll,
        c::RESIZE_X => C::ResizeHorizontal,
        c::RESIZE_Y => C::ResizeVertical,
        c::RESIZE_N => C::ResizeNorth,
        c::RESIZE_E => C::ResizeEast,
        c::RESIZE_S => C::ResizeSouth,
        c::RESIZE_W => C::ResizeWest,
        c::RESIZE_NE => C::ResizeNorthEast,
        c::RESIZE_NW => C::ResizeNorthWest,
        c::RESIZE_SE => C::ResizeSouthEast,
        c::RESIZE_SW => C::ResizeSouthWest,
        c::RESIZE_NESW => C::ResizeNeSw,
        c::RESIZE_NWSE => C::ResizeNwSe,
        c::RESIZE_COL => C::ResizeColumn,
        c::RESIZE_ROW => C::ResizeRow,
        c::ZOOM_IN => C::ZoomIn,
        c::ZOOM_OUT => C::ZoomOut,
        _ => return None,
    })
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
    let response = ui.interact(
        rect,
        sensor_id(ui, "balaur-tip", entity),
        egui::Sense::hover(),
    );
    if ui.is_enabled() {
        crate::widget::theme::tip(&response, tooltip);
    } else {
        crate::widget::popups::disabled_text(&response, tooltip);
    }
}

/// The id of a sensor a node draws, one per layer: a menu's rows draw in each
/// popup that shows them, and egui refuses an id on two layers in a pass.
pub(crate) fn sensor_id(
    ui: &egui::Ui,
    word: &'static str,
    node: impl std::hash::Hash + std::fmt::Debug,
) -> egui::Id {
    egui::Id::unique((word, node, ui.layer_id()))
}

/// A framed box that lays its children out, with the padding taken off in
/// floats: `egui::Margin` is whole points, and a theme is free to ask for
/// half of one.
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
    let widget = &at.arena[index].widget;
    let style = at.style(index);
    let pad = padding_of(widget, &style);
    let box_size = solved_of(widget, &at.style(index), at.assigned);
    let plate = Brush::of(ui).slot();
    let min = (box_size - pad.taken()).max(egui::Vec2::ZERO);
    // Top down whatever the parent runs: a caption in a centred row was
    // drawn halfway down the box, over the children placed under it.
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(pad.inside(ui.max_rect()))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    hold_to(&mut inner, min);
    if !caption.is_empty() {
        inner.label(rich(
            caption,
            font,
            color,
            at.slant(index),
            &widget.text_look,
        ));
    }
    // A panel with nothing in it is the panel it always was.
    let held = std::mem::replace(&mut at.bounds, min);
    let under = inner.available_rect_before_wrap();
    lay_out_within(&mut inner, at, index, Axis::Column, under);
    at.bounds = held;
    // Measured from the children where there are only children: a root that
    // hugs at a right or bottom corner sits in an `Area` whose own corner is
    // far from it, and the `Ui` would stretch the plate back to that corner.
    let content = at.arena[index]
        .children
        .iter()
        .filter_map(|child| at.rects.get(child).copied())
        .reduce(egui::Rect::union)
        .filter(|_| caption.is_empty() && box_size == egui::Vec2::ZERO);
    let background = pad.around(content.unwrap_or_else(|| inner.min_rect()));
    if let Some(path) = style.image.as_ref() {
        crate::widget::kinds::nine_patch_plate(ui, at.eng, plate, path, style.slice, background);
    } else {
        Brush::of(ui).set(
            plate,
            crate::widget::frame::frame_shape(
                background,
                egui::CornerRadius::same(style.radius.unwrap_or(8.0) as u8),
                style.fill.unwrap_or(Color32::from_black_alpha(96)),
                &style,
            ),
        );
    }
    ui.advance_cursor_after_rect(background);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cursor_word_names_a_pointer_and_no_word_names_none() {
        for word in w::cursor::ALL {
            assert_eq!(
                pointer_icon(word).is_some(),
                *word != w::cursor::ARROW,
                "{word}"
            );
        }
        assert_eq!(pointer_icon(""), None);
        assert_eq!(pointer_icon("banana"), None);
    }
}

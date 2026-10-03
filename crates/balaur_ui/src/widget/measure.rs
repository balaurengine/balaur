//! What a widget needs before anything draws it.
//!
//! egui measures while drawing, which is a frame too late for a container
//! deciding where its children go. This walks the same tree the draw will and
//! asks the font atlas instead, so a row sizes itself to a label that changed
//! this frame rather than to the one it showed last.

use crate::theme::family;
use crate::vocabulary::words as w;
use crate::widget::arena::Placed;
use crate::widget::arrange::padding_of;
use crate::widget::layer::caption;
use crate::widget::node::{Widget, lays_out};
use crate::widget::theme::WidgetTheme;
use balaur_core::Engine;
use egui::vec2;
use rustc_hash::FxHashMap;
use std::rc::Rc;

/// A measure over one tree, memoised within itself: a container asks each
/// child for a whole subtree, and its own parent will ask again. egui caches
/// galleys by text and font, so the repeat is a hash lookup rather than a
/// re-layout.
pub(crate) struct Measure<'a> {
    pub(super) eng: &'a Engine,
    arena: &'a [Placed],
    /// The painter is how text is measured without drawing it, and the
    /// padding is what egui will put around a button's own text.
    painter: egui::Painter,
    padding: egui::Vec2,
    seen: FxHashMap<usize, egui::Vec2>,
    /// What `leaf` answered, which is not what `of` answers: no stated size
    /// and no floor applied. Asked twice a leaf a pass — once to see whether
    /// the content moved, once by taffy solving the node.
    leaves: FxHashMap<usize, egui::Vec2>,
    /// What a wrapping leaf answered at one width, keyed by both: taffy asks
    /// the same block at several widths while it settles a row.
    wraps: FxHashMap<(usize, u32), egui::Vec2>,
}

impl<'a> Measure<'a> {
    pub(crate) fn new(eng: &'a Engine, arena: &'a [Placed], ui: &egui::Ui) -> Self {
        Self {
            eng,
            arena,
            painter: ui.painter().clone(),
            padding: ui.spacing().button_padding * 2.0,
            seen: FxHashMap::default(),
            leaves: FxHashMap::default(),
            wraps: FxHashMap::default(),
        }
    }

    /// What one leaf's content asks for, with no recursion into children:
    /// what the layout tree calls back for, since it owns every container
    /// itself. Its box less what taffy keeps inside its edge, which taffy adds
    /// back, so the padding is counted once.
    pub(crate) fn leaf(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        if let Some(size) = self.leaves.get(&index) {
            return *size;
        }
        let widget = &self.arena[index].widget;
        if !widget.visible {
            return egui::Vec2::ZERO;
        }
        // The whole chain, not this widget's own `theme`: a measure caches the
        // look it resolves, and a solve that starts below the node carrying
        // the theme would cache one dressed by no theme at all.
        let theme = crate::widget::arena::theme_at(self.eng, self.arena, index, theme);
        let size = (self.natural(index, &theme) - self.inset(index, &theme)).max(egui::Vec2::ZERO);
        self.leaves.insert(index, size);
        size
    }

    /// What taffy keeps clear inside a node's edge, across and down: its
    /// padding, its `border`, and a scroll's bar.
    fn inset(&self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let widget = &self.arena[index].widget;
        let bar = if widget.layout.overflow == crate::vocabulary::words::overflow::SCROLL {
            widget.layout.scrollbar_width.get()
        } else {
            0.0
        };
        self.pad(index, theme).taken() + egui::Vec2::splat(bar)
    }

    /// The padding and border the widget at `index` keeps inside its edge.
    fn pad(&self, index: usize, theme: &Rc<WidgetTheme>) -> crate::widget::arrange::Pad {
        let look = crate::widget::arena::look_of(self.arena, index, theme);
        padding_of(&self.arena[index].widget, &look.style)
    }

    /// The smallest box `index` can be drawn in, in design pixels.
    ///
    /// Zero on an axis nothing can answer for: a `draw` node is a script's to
    /// fill, and a `scroll` exists to be smaller than what is in it.
    pub(crate) fn of(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        if let Some(size) = self.seen.get(&index) {
            return *size;
        }
        // Guard against a cycle before recursing: a scene is a tree, but the
        // arena is built from one and a bad one should not hang the frame.
        self.seen.insert(index, egui::Vec2::ZERO);
        let widget = &self.arena[index].widget;
        let theme = crate::widget::arena::theme_at(self.eng, self.arena, index, theme);
        let size = if widget.visible {
            self.natural(index, &theme)
        } else {
            egui::Vec2::ZERO
        };
        let floor = vec2(widget.min_width, widget.min_height);
        let stated = vec2(widget.width, widget.height);
        let size = vec2(
            if stated.x > 0.0 { stated.x } else { size.x }.max(floor.x),
            if stated.y > 0.0 { stated.y } else { size.y }.max(floor.y),
        );
        self.seen.insert(index, size);
        size
    }

    /// The widget's whole box: its content, and the padding round it.
    fn natural(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let widget = &self.arena[index].widget;
        let bare = self.arena[index].children.is_empty();
        match widget.kind.as_str() {
            // These measure their own padding: a caption's floor, a title bar
            // or a header sits inside it.
            w::BUTTON => self.button(index, widget, theme),
            w::MENU if !bare => self.button(index, widget, theme),
            w::FOLD => self.fold(index, theme),
            w::STACK => self.stack(index, theme),
            w::GRID => self.grid(index, theme),
            w::FLOW => self.flow(index, theme),
            w::WINDOW => self.window(index, theme),
            kind if lays_out(kind) && !matches!(kind, w::SCROLL | w::TABS | w::MENU) => {
                self.container(index, theme)
            }
            _ => self.content(index, theme) + self.pad(index, theme).taken(),
        }
    }

    /// What a kind that draws inside its padding needs, without it.
    fn content(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let widget = &self.arena[index].widget;
        match widget.kind.as_str() {
            // A scroll is meant to clip, so it answers with its stated size
            // or with nothing. A script's rect can only be remembered: what
            // it drew last frame is the one thing anything knows about it.
            w::SCROLL => egui::Vec2::ZERO,
            w::DRAW => crate::widget::arrange::measured_of(self.arena[index].entity),
            // A picture with a `fit` is sized by the box it is given, so it
            // measures only what it states: Godot's expand modes.
            w::IMAGE if !widget.fit.is_empty() => vec2(widget.width, widget.height),
            // A picture knows its own size, so a row can divide by it.
            // A picture that will not load is the `alt_text` drawn instead.
            w::IMAGE => {
                crate::images::texture_of(self.eng, &self.painter.ctx().clone(), &widget.source)
                    .map_or_else(
                        |_| self.alt_text(index, widget, theme),
                        |texture| {
                            let native =
                                crate::images::native_size(self.eng, &widget.source, &texture);
                            let (_, drawn) = crate::images::region_uv(native, widget.egui.region);
                            crate::widget::layer::image_size(
                                vec2(widget.width, widget.height),
                                drawn,
                            )
                        },
                    )
            }
            // Room for a dozen wide letters: what a field takes before a
            // container or a `width` says otherwise.
            w::TEXT_FIELD => {
                let line = self.galley(index, "MMMMMMMMMMMM", widget, theme);
                line + self.padding
            }
            w::TABS => {
                let strip = self.strip(index, theme);
                let pages = self.widest_child(index, theme);
                let gap = self.gap(index, theme).y;
                vec2(strip.x.max(pages.x), strip.y + gap + pages.y)
            }
            // A box the height of the text, then the caption.
            w::CHECKBOX => {
                let text = self.text(index, widget, theme);
                let line = self.line(index, theme);
                vec2(text.x + line + self.padding.x, text.y.max(line))
            }
            // A track and its knob: as tall as the role asks, and most of twice
            // that across.
            w::SWITCH => {
                let look = crate::widget::arena::look_of(self.arena, index, theme);
                let height = look.style.height.unwrap_or(18.0);
                vec2(height * 1.75, height)
            }
            // The widest option, and room for the arrow.
            w::DROPDOWN => {
                let mut widest = self.text(index, widget, theme);
                for option in &widget.options {
                    widest = widest.max(self.galley(index, option, widget, theme));
                }
                widest + self.padding + vec2(20.0, 0.0)
            }
            // As tall as a line of the face it draws in, so a caption on the
            // bar is not cut: the theme's size where the widget states none.
            w::SLIDER | w::PROGRESS_BAR => {
                let line = self.line(index, theme);
                let caption = if widget.kind == w::PROGRESS_BAR {
                    self.text(index, widget, theme).x + self.padding.x
                } else {
                    0.0
                };
                let shown = crate::widget::numbers::value_room(&self.painter, widget);
                vec2(caption.max(160.0) + shown, line + self.padding.y)
            }
            w::SEPARATOR => {
                let spacing = widget.egui.spacing;
                egui::Vec2::splat(if spacing >= 0.0 { spacing } else { 6.0 })
            }
            // A menu of strings is egui's own button, measured by its caption;
            // one whose rows are nodes is drawn as a button, and measured as one.
            _ => self.text(index, widget, theme),
        }
    }

    /// A window: its title bar over its children, or the bar alone while it
    /// is folded or holds none, inside its padding; nothing while it is shut.
    fn window(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let widget = &self.arena[index].widget;
        if !widget.open {
            return egui::Vec2::ZERO;
        }
        let bar = self.title_bar(index, theme);
        let folded = crate::widget::window::folded(self.arena[index].entity, widget);
        if folded || self.arena[index].children.is_empty() {
            return bar + self.pad(index, theme).taken();
        }
        let body = self.container(index, theme);
        vec2(body.x.max(bar.x), body.y)
    }

    /// A fold: its header, the arrow a square as tall as the caption's line
    /// and the title bar children beside it, then what it shows while open.
    fn fold(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let widget = &self.arena[index].widget;
        let text = self.text(index, widget, theme);
        let mut head = vec2(text.x + text.y + 8.0, text.y);
        let arena = self.arena;
        for child in &arena[index].children {
            if crate::widget::kinds::in_title_bar(arena, index, *child) {
                let size = self.of(*child, theme);
                head = vec2(head.x + size.x + 8.0, head.y.max(size.y));
            }
        }
        let look = crate::widget::arena::look_of(arena, index, theme);
        let head = head + padding_of(widget, &look.style).taken();
        if !widget.open {
            return head;
        }
        let frame = look.style.body.as_deref().map_or(egui::Vec2::ZERO, |body| {
            crate::widget::arrange::style_padding(body, egui::Vec2::ZERO).taken()
        });
        // The fold's own padding is its header's; what it shows sits in the
        // body's frame alone.
        let body = self.inner(index, theme) + frame;
        vec2(head.x.max(body.x), head.y + 8.0 + body.y)
    }

    /// A row or column: its children end to end along its axis, the widest
    /// across, plus the gaps between them and its own padding.
    fn container(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        self.inner(index, theme) + self.pad(index, theme).taken()
    }

    /// The same without the padding.
    fn inner(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let placed = &self.arena[index];
        let widget = &placed.widget;
        let row = widget.kind == w::ROW;
        let children = placed.children.clone();
        let caption = if widget.kind == w::PANEL {
            let text = self.text(index, widget, theme);
            vec2(text.x, self.band(index, theme))
        } else if widget.kind == w::WINDOW {
            vec2(0.0, self.band(index, theme))
        } else {
            egui::Vec2::ZERO
        };
        let gap = self.gap(index, theme);
        let gap = if row { gap.x } else { gap.y };
        let mut along = 0.0f32;
        let mut across: f32 = 0.0;
        let mut drawn = 0usize;
        for child in &children {
            if crate::widget::kinds::in_title_bar(self.arena, index, *child) {
                continue;
            }
            let size = self.of(*child, theme);
            if size == egui::Vec2::ZERO {
                continue;
            }
            let (a, c) = if row {
                (size.x, size.y)
            } else {
                (size.y, size.x)
            };
            along += a;
            across = across.max(c);
            drawn += 1;
        }
        along += gap * (drawn.saturating_sub(1) as f32);
        if row {
            vec2(along + caption.x, across.max(caption.y))
        } else {
            // A panel's caption sits above its children, so it adds a row.
            vec2(across.max(caption.x), along + caption.y)
        }
    }

    /// A stack: its biggest child on each axis, plus its own padding, since
    /// every child is laid over the same box.
    fn stack(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let placed = &self.arena[index];
        let widget = placed.widget.clone();
        let children = placed.children.clone();
        let mut want = egui::Vec2::ZERO;
        for child in &children {
            want = want.max(self.of(*child, theme));
        }
        let pad = padding_of(&widget, &crate::widget::theme::styled(theme, &widget));
        want + pad.taken()
    }

    /// A grid, estimated for a kind that measures it rather than solving it:
    /// the biggest child's cell, tiled as many wide as `grid_columns` names.
    fn grid(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let placed = &self.arena[index];
        let widget = placed.widget.clone();
        let children = placed.children.clone();
        let columns = match crate::widget::grid::track_count(&widget.layout.grid_columns) {
            0 => 2,
            named => named,
        };
        let gap = self.gap(index, theme);
        let mut cell = egui::Vec2::ZERO;
        let mut count = 0usize;
        for child in &children {
            let size = self.of(*child, theme);
            if size == egui::Vec2::ZERO {
                continue;
            }
            cell = cell.max(size);
            count += 1;
        }
        if count == 0 {
            return egui::Vec2::ZERO;
        }
        let rows = count.div_ceil(columns);
        let across = columns.min(count);
        let inner = vec2(
            across as f32 * cell.x + gap.x * (across as f32 - 1.0),
            rows as f32 * cell.y + gap.y * (rows as f32 - 1.0),
        );
        let pad = padding_of(&widget, &crate::widget::theme::styled(theme, &widget));
        inner + pad.taken()
    }

    /// A flow: its children on one line, or wrapped to a stated width.
    fn flow(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let placed = &self.arena[index];
        let widget = placed.widget.clone();
        let children = placed.children.clone();
        let gap = self.gap(index, theme);
        let pad = padding_of(&widget, &crate::widget::theme::styled(theme, &widget));
        let limit = if widget.width > 0.0 {
            widget.width - pad.taken().x
        } else {
            f32::INFINITY
        };
        let mut cursor = egui::Vec2::ZERO;
        let mut line = 0.0f32;
        let mut extent = egui::Vec2::ZERO;
        for child in &children {
            let size = self.of(*child, theme);
            if size == egui::Vec2::ZERO {
                continue;
            }
            if cursor.x > 0.0 && cursor.x + size.x > limit {
                cursor = vec2(0.0, cursor.y + line + gap.y);
                line = 0.0;
            }
            extent = extent.max(cursor + size);
            cursor.x += size.x + gap.x;
            line = line.max(size.y);
        }
        if extent == egui::Vec2::ZERO {
            return extent;
        }
        extent + pad.taken()
    }

    /// A tab's strip: every page's label side by side, as buttons.
    fn strip(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let placed = &self.arena[index];
        let widget = placed.widget.clone();
        let gap = self.gap(index, theme).x.max(4.0);
        let padding = self.padding;
        let mut width = 0.0f32;
        let mut height: f32 = 0.0;
        for (slot, child) in placed.children.iter().enumerate() {
            let page = &self.arena[*child];
            let label = if page.widget.text.is_empty() {
                page.name.as_str()
            } else {
                page.widget.text.as_str()
            };
            let size = self.galley(*child, label, &widget, theme) + padding;
            width += size.x + if slot > 0 { gap } else { 0.0 };
            height = height.max(size.y);
        }
        vec2(width, height)
    }

    /// The biggest page, so a tab does not resize as it is clicked through.
    fn widest_child(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let children = self.arena[index].children.clone();
        let mut size = egui::Vec2::ZERO;
        for child in &children {
            size = size.max(self.of(*child, theme));
        }
        size
    }

    /// A button's box: the icon, the caption, its padding, and the floor its
    /// role carries. The same arithmetic the draw does, or a strip of buttons
    /// is handed less room than it paints into.
    fn button(&self, index: usize, widget: &Widget, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        self.button_in(index, widget, theme, 0.0)
    }

    /// The same box at a `width` the layout settled on, padding included,
    /// where a `wrap` caption breaks into more lines; zero is no width, the
    /// caption on one line.
    fn button_in(
        &self,
        index: usize,
        widget: &Widget,
        theme: &Rc<WidgetTheme>,
        width: f32,
    ) -> egui::Vec2 {
        let look = crate::widget::arena::look_of(self.arena, index, theme);
        let (style, font) = (&look.style, look.font.clone());
        let mut text = self.text(index, widget, theme);
        let mark = if widget.icon.is_empty() {
            egui::Vec2::ZERO
        } else {
            let face = egui::FontId::new(
                crate::widget::text::icon_px(widget, font.size),
                family(w::ICON),
            );
            self.painter
                .layout_no_wrap(widget.icon.to_string(), face, egui::Color32::WHITE)
                .size()
        };
        let pic = self.picture(widget, style, font.size);
        let tail = if widget.trailing.is_empty() {
            egui::Vec2::ZERO
        } else {
            self.painter
                .layout_no_wrap(
                    widget.trailing.to_string(),
                    font.clone(),
                    egui::Color32::WHITE,
                )
                .size()
        };
        // The draw's arithmetic: a gap between each pair of parts that are
        // there, and a wider one before the trailing text.
        let tail_gap = if tail.x > 0.0 { font.size } else { 0.0 };
        let pad = padding_of(widget, style).taken();
        let gap = crate::widget::button::gap_of(widget, style, font.size);
        if text.x > 0.0 {
            let before = [pic.x, mark.x].iter().filter(|w| **w > 0.0).count() as f32;
            let taken = pic.x + mark.x + gap * before + tail_gap + tail.x;
            let inside = if width > 0.0 { width - pad.x } else { 0.0 };
            if let Some(room) = crate::widget::text::caption_room(widget, text.x, inside, taken) {
                let caption = caption(self.eng, widget);
                text = self.caption_in(index, &caption, widget, theme, room);
            }
        }
        let parts = [pic.x, mark.x, text.x].iter().filter(|w| **w > 0.0).count();
        let between = gap * parts.saturating_sub(1) as f32;
        let floor = vec2(style.width.unwrap_or(0.0), style.height.unwrap_or(0.0));
        let face = vec2(
            pic.x + mark.x + text.x + between + tail_gap + tail.x,
            pic.y.max(mark.y).max(text.y).max(tail.y),
        );
        (face + pad).max(floor)
    }

    /// A button's picture at its caption's height, with the disc a role may
    /// put under it.
    fn picture(
        &self,
        widget: &Widget,
        style: &crate::widget::theme::Style,
        line: f32,
    ) -> egui::Vec2 {
        if widget.source.is_empty() {
            return egui::Vec2::ZERO;
        }
        let Ok(texture) = crate::images::texture_of(self.eng, self.painter.ctx(), &widget.source)
        else {
            return egui::Vec2::ZERO;
        };
        let native = crate::images::native_size(self.eng, &widget.source, &texture);
        let aspect = if native.y > 0.0 {
            native.x / native.y
        } else {
            1.0
        };
        let plate = if style.plate.is_some() { 4.0 } else { 0.0 };
        vec2(line * aspect + plate, line + plate)
    }

    /// An image's `alt_text`, in the face its draw puts it in.
    fn alt_text(&self, index: usize, widget: &Widget, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        if widget.egui.alt_text.is_empty() {
            return egui::Vec2::ZERO;
        }
        let look = crate::widget::arena::look_of(self.arena, index, theme);
        self.painter
            .layout_no_wrap(
                widget.egui.alt_text.to_string(),
                look.font.clone(),
                egui::Color32::WHITE,
            )
            .size()
    }

    /// A container's gap, across and down, as its theme resolves it.
    fn gap(&self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let look = crate::widget::arena::look_of(self.arena, index, theme);
        crate::widget::arrange::gap_of(&self.arena[index].widget, &look.style)
    }

    /// How tall a line of the widget's face is.
    fn line(&self, index: usize, theme: &Rc<WidgetTheme>) -> f32 {
        let look = crate::widget::arena::look_of(self.arena, index, theme);
        self.painter
            .ctx()
            .fonts_mut(|fonts| fonts.row_height(&look.font))
    }

    /// A window's title bar: its arrow where it folds, its title, and its
    /// cross, one line of its face tall with a button's air above and below.
    /// The draw lays the bar out from this, so the two cannot disagree.
    pub(crate) fn title_bar(&self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let widget = &self.arena[index].widget;
        if !widget.header {
            return egui::Vec2::ZERO;
        }
        let look = crate::widget::arena::look_of(self.arena, index, theme);
        let line = self
            .painter
            .ctx()
            .fonts_mut(|fonts| fonts.row_height(&look.font));
        let title = caption(self.eng, widget);
        let slant = crate::widget::theme::slanted(&look.style, widget);
        let words = crate::widget::theme::galley(&self.painter, &title, &look.font, slant).size();
        let marks = [widget.egui.collapsible, widget.egui.closable]
            .iter()
            .filter(|on| **on)
            .count() as f32;
        // The arrow and the cross each take a square as tall as the bar.
        let tall = line.max(words.y) + self.padding.y;
        vec2(words.x + marks * tall, tall)
    }

    /// What a container keeps clear above its children for what it draws
    /// there itself, gap included: a panel's caption, a window's title bar.
    pub(crate) fn band(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> f32 {
        let widget = &self.arena[index].widget;
        let gap = self.gap(index, theme).y;
        match widget.kind.as_str() {
            w::PANEL => {
                let title = caption(self.eng, widget);
                if title.is_empty() {
                    return 0.0;
                }
                let look = crate::widget::arena::look_of(self.arena, index, theme);
                let slant = crate::widget::theme::slanted(&look.style, widget);
                crate::widget::theme::galley(&self.painter, &title, &look.font, slant)
                    .size()
                    .y
                    + gap
            }
            w::WINDOW if widget.header => self.title_bar(index, theme).y + gap,
            _ => 0.0,
        }
    }

    fn text(&self, index: usize, widget: &Widget, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let caption = caption(self.eng, widget);
        if caption.is_empty() {
            return egui::Vec2::ZERO;
        }
        self.galley(index, &caption, widget, theme)
    }

    /// One line of text, unwrapped: what the widget needs to show it whole.
    /// Shaped once the fonts are up; egui's own layout stands in before.
    ///
    /// The face comes from the theme the same way the draw resolves it, or a
    /// row under a role would be measured at a size it never draws at.
    fn galley(
        &self,
        index: usize,
        text: &str,
        widget: &Widget,
        theme: &Rc<WidgetTheme>,
    ) -> egui::Vec2 {
        self.galley_in(index, text, widget, theme, None)
    }

    /// The same line inside a box: `None` is no box, which is what everything
    /// but a wrapping block asks for.
    fn galley_in(
        &self,
        index: usize,
        text: &str,
        widget: &Widget,
        theme: &Rc<WidgetTheme>,
        width: Option<f32>,
    ) -> egui::Vec2 {
        let look = crate::widget::arena::look_of(self.arena, index, theme);
        let (style, font) = (&look.style, look.font.clone());
        if let Some(state) = balaur_text::state(self.eng) {
            let request = crate::widget::text::text_request(widget, text, width, &font, style);
            return state
                .borrow_mut()
                .shape_for_egui(&self.painter.ctx().clone(), &request)
                .size;
        }
        match width {
            Some(width) => self
                .painter
                .layout(text.to_owned(), font, egui::Color32::WHITE, width)
                .size(),
            None => self
                .painter
                .layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)
                .size(),
        }
    }

    /// A button's caption shaped in `room`, the way the draw shapes it.
    fn caption_in(
        &self,
        index: usize,
        text: &str,
        widget: &Widget,
        theme: &Rc<WidgetTheme>,
        room: f32,
    ) -> egui::Vec2 {
        let look = crate::widget::arena::look_of(self.arena, index, theme);
        let Some(state) = balaur_text::state(self.eng) else {
            return self.galley_in(index, text, widget, theme, Some(room));
        };
        let request =
            crate::widget::text::caption_request(widget, text, Some(room), &look.font, &look.style);
        state
            .borrow_mut()
            .shape_for_egui(&self.painter.ctx().clone(), &request)
            .size
    }

    /// What a wrapping block's content needs in a box of `width`, padding
    /// included in the box and left out of the answer as [`Self::leaf`]
    /// leaves it out: `None` for anything that does not wrap.
    pub(crate) fn wrapped(
        &mut self,
        index: usize,
        width: f32,
        theme: &Rc<WidgetTheme>,
    ) -> Option<egui::Vec2> {
        // Lifted off `self` so the cache below may be written: the arena
        // outlives the measure that borrows it.
        let arena = self.arena;
        let widget = &arena[index].widget;
        let kind = widget.kind.as_str();
        if !widget.visible || !widget.wrap || !matches!(kind, w::LABEL | w::BUTTON) || width <= 0.0
        {
            return None;
        }
        let key = (index, width.to_bits());
        if let Some(size) = self.wraps.get(&key) {
            return Some(*size);
        }
        let caption = caption(self.eng, widget);
        if caption.is_empty() {
            return None;
        }
        let theme = crate::widget::arena::theme_at(self.eng, arena, index, theme);
        let inset = self.inset(index, &theme);
        let size = if kind == w::BUTTON {
            let boxed = self.button_in(index, widget, &theme, width);
            vec2(width, boxed.y) - inset
        } else {
            self.galley_in(
                index,
                &caption,
                widget,
                &theme,
                Some((width - inset.x).max(1.0)),
            )
        };
        let size = size.max(egui::Vec2::ZERO);
        self.wraps.insert(key, size);
        Some(size)
    }
}

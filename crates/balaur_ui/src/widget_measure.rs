//! What a widget needs before anything draws it.
//!
//! egui measures while drawing, which is a frame too late for a container
//! deciding where its children go. This walks the same tree the draw will and
//! asks the font atlas instead, so a row sizes itself to a label that changed
//! this frame rather than to the one it showed last.

use crate::theme::family;
use crate::vocabulary::words as w;
use crate::widget_arrange::padding_of;
use crate::widget_layer::{Placed, Widget, caption, lays_out, theme_of};
use crate::widget_theme::WidgetTheme;
use balaur_core::Engine;
use egui::vec2;
use rustc_hash::FxHashMap;
use std::rc::Rc;

/// A measure over one tree, memoised within itself: a container asks each
/// child for a whole subtree, and its own parent will ask again. egui caches
/// galleys by text and font, so the repeat is a hash lookup rather than a
/// re-layout.
pub(crate) struct Measure<'a> {
    eng: &'a Engine,
    arena: &'a [Placed],
    /// The painter is how text is measured without drawing it, and the
    /// padding is what egui will put around a button's own text.
    painter: egui::Painter,
    padding: egui::Vec2,
    scale: f32,
    seen: FxHashMap<usize, egui::Vec2>,
    /// What `leaf` answered, which is not what `of` answers: no stated size
    /// and no floor applied. Asked twice a leaf a pass — once to see whether
    /// the content moved, once by taffy solving the node.
    leaves: FxHashMap<usize, egui::Vec2>,
}

impl<'a> Measure<'a> {
    pub(crate) fn new(eng: &'a Engine, arena: &'a [Placed], ui: &egui::Ui, scale: f32) -> Self {
        Self {
            eng,
            arena,
            painter: ui.painter().clone(),
            padding: ui.spacing().button_padding * 2.0,
            scale,
            seen: FxHashMap::default(),
            leaves: FxHashMap::default(),
        }
    }

    /// What one leaf asks for, with no recursion into children: what the
    /// layout tree calls back for, since it owns every container itself.
    pub(crate) fn leaf(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        if let Some(size) = self.leaves.get(&index) {
            return *size;
        }
        let widget = &self.arena[index].widget;
        if !widget.visible {
            return egui::Vec2::ZERO;
        }
        let theme = theme_of(self.eng, &widget.theme, theme);
        let size = self.natural(index, &theme);
        self.leaves.insert(index, size);
        size
    }

    /// The smallest box `index` can be drawn in, in device pixels.
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
        let theme = theme_of(self.eng, &widget.theme, theme);
        let size = if widget.visible {
            self.natural(index, &theme)
        } else {
            egui::Vec2::ZERO
        };
        let floor = vec2(widget.min_width, widget.min_height) * self.scale;
        let stated = vec2(widget.width, widget.height) * self.scale;
        let size = vec2(
            if stated.x > 0.0 { stated.x } else { size.x }.max(floor.x),
            if stated.y > 0.0 { stated.y } else { size.y }.max(floor.y),
        );
        self.seen.insert(index, size);
        size
    }

    fn natural(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let widget = &self.arena[index].widget;
        match widget.kind.as_str() {
            // A scroll is meant to clip, so it answers with its stated size
            // or with nothing. A script's rect can only be remembered: what
            // it drew last frame is the one thing anything knows about it.
            w::SCROLL => egui::Vec2::ZERO,
            w::DRAW => crate::widget_arrange::measured_of(self.arena[index].entity),
            // A picture knows its own size, so a row can divide by it.
            w::IMAGE => {
                crate::images::texture_of(self.eng, &self.painter.ctx().clone(), &widget.source)
                    .map_or(egui::Vec2::ZERO, |texture| {
                        crate::widget_layer::image_size(
                            vec2(widget.width, widget.height) * self.scale,
                            texture.size_vec2(),
                        )
                    })
            }
            w::BUTTON => self.button(index, widget, theme),
            w::LABEL => self.text(index, widget, theme),
            // Room for a dozen wide letters: what a field takes before a
            // container or a `width` says otherwise.
            w::FIELD => {
                let line = self.galley(index, "MMMMMMMMMMMM", widget, theme);
                line + self.padding
            }
            w::TAB => {
                let strip = self.strip(index, theme);
                let pages = self.widest_child(index, theme);
                let gap = widget.gap * self.scale;
                vec2(strip.x.max(pages.x), strip.y + gap + pages.y)
            }
            // A box the height of the text, then the caption.
            w::CHECK => {
                let text = self.text(index, widget, theme);
                let line = widget.font_size * self.scale;
                vec2(text.x + line + self.padding.x, text.y.max(line))
            }
            // The widest option, and room for the arrow.
            w::DROPDOWN => {
                let mut widest = self.text(index, widget, theme);
                for option in &widget.options {
                    widest = widest.max(self.galley(index, option, widget, theme));
                }
                widest + self.padding + vec2(20.0 * self.scale, 0.0)
            }
            w::SLIDER | w::PROGRESS => vec2(
                160.0 * self.scale,
                widget.font_size * self.scale + self.padding.y,
            ),
            w::SEPARATOR => egui::Vec2::splat(6.0 * self.scale),
            w::WINDOW if !widget.open => egui::Vec2::ZERO,
            w::FOLD => {
                let head = self.text(index, widget, theme) + vec2(20.0 * self.scale, 0.0);
                if !widget.open {
                    return head;
                }
                let body = self.container(index, theme);
                vec2(head.x.max(body.x), head.y + body.y)
            }
            // Drawn as a button once its rows are nodes, so measured as one; a
            // menu of strings is egui's own button, measured by its caption.
            w::MENU => {
                if self.arena[index].children.is_empty() {
                    self.text(index, widget, theme)
                } else {
                    self.button(index, widget, theme)
                }
            }
            w::GRID => self.grid(index, theme),
            w::FLOW => self.flow(index, theme),
            _ if lays_out(&widget.kind) => self.container(index, theme),
            _ => self.text(index, widget, theme),
        }
    }

    /// A row or column: its children end to end along its axis, the widest
    /// across, plus the gaps between them and its own padding.
    fn container(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let placed = &self.arena[index];
        let widget = &placed.widget;
        let row = widget.kind == w::ROW;
        let children = placed.children.clone();
        let caption = if widget.kind == w::PANEL {
            self.text(index, widget, theme)
        } else if widget.kind == w::WINDOW {
            // The title and its cross, on one bar.
            self.text(index, widget, theme) + egui::vec2(widget.font_size * self.scale * 1.5, 0.0)
        } else {
            egui::Vec2::ZERO
        };
        let gap = widget.gap * self.scale;
        let mut along = 0.0f32;
        let mut across: f32 = 0.0;
        let mut drawn = 0usize;
        for child in &children {
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
        let inner = if row {
            vec2(along + caption.x, across.max(caption.y))
        } else {
            // A panel's caption sits above its children, so it adds a row.
            vec2(across.max(caption.x), along + caption.y)
        };
        let pad = padding_of(
            widget,
            &crate::widget_layer::styled(theme, widget),
            self.scale,
        );
        inner + egui::Vec2::splat(pad * 2.0)
    }

    /// A grid: the biggest child's cell, tiled `columns` wide.
    fn grid(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let placed = &self.arena[index];
        let widget = placed.widget.clone();
        let children = placed.children.clone();
        let columns = crate::widget_kinds::grid_columns(&widget);
        let gap = widget.gap * self.scale;
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
            across as f32 * cell.x + gap * (across as f32 - 1.0),
            rows as f32 * cell.y + gap * (rows as f32 - 1.0),
        );
        let pad = padding_of(
            &widget,
            &crate::widget_layer::styled(theme, &widget),
            self.scale,
        );
        inner + egui::Vec2::splat(pad * 2.0)
    }

    /// A flow: its children on one line, or wrapped to a stated width.
    fn flow(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let placed = &self.arena[index];
        let widget = placed.widget.clone();
        let children = placed.children.clone();
        let gap = widget.gap * self.scale;
        let pad = padding_of(
            &widget,
            &crate::widget_layer::styled(theme, &widget),
            self.scale,
        );
        let limit = if widget.width > 0.0 {
            widget.width * self.scale - 2.0 * pad
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
                cursor = vec2(0.0, cursor.y + line + gap);
                line = 0.0;
            }
            extent = extent.max(cursor + size);
            cursor.x += size.x + gap;
            line = line.max(size.y);
        }
        if extent == egui::Vec2::ZERO {
            return extent;
        }
        extent + egui::Vec2::splat(pad * 2.0)
    }

    /// A tab's strip: every page's label side by side, as buttons.
    fn strip(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let placed = &self.arena[index];
        let widget = placed.widget.clone();
        let gap = (widget.gap * self.scale).max(4.0);
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

    /// A button's box: the icon, the caption, the air either side, and the
    /// floor its role carries. The same arithmetic the draw does, or a strip
    /// of buttons is handed less room than it paints into.
    fn button(&self, index: usize, widget: &Widget, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let look = crate::widget_layer::look_of(self.arena, index, theme, self.scale);
        let (style, font) = (&look.style, look.font.clone());
        let text = self.text(index, widget, theme);
        let mark = if widget.icon.is_empty() {
            egui::Vec2::ZERO
        } else {
            let face = egui::FontId::new(font.size, family(w::ICON));
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
        let parts = [pic.x, mark.x, text.x].iter().filter(|w| **w > 0.0).count();
        let between = font.size * 0.5 * parts.saturating_sub(1) as f32;
        let tail_gap = if tail.x > 0.0 { font.size } else { 0.0 };
        let pad = style
            .padding_x
            .map_or(self.padding.x, |p| p * self.scale * 2.0);
        let floor = vec2(style.width.unwrap_or(0.0), style.height.unwrap_or(0.0)) * self.scale;
        vec2(
            pic.x + mark.x + text.x + between + tail_gap + tail.x + pad,
            pic.y.max(mark.y).max(text.y).max(tail.y) + self.padding.y,
        )
        .max(floor)
    }

    /// A button's picture at its caption's height, with the disc a role may
    /// put under it.
    fn picture(
        &self,
        widget: &Widget,
        style: &crate::widget_theme::Style,
        line: f32,
    ) -> egui::Vec2 {
        if widget.source.is_empty() {
            return egui::Vec2::ZERO;
        }
        let Ok(texture) = crate::images::texture_of(self.eng, self.painter.ctx(), &widget.source)
        else {
            return egui::Vec2::ZERO;
        };
        let native = texture.size_vec2();
        let aspect = if native.y > 0.0 {
            native.x / native.y
        } else {
            1.0
        };
        let plate = if style.plate.is_some() {
            4.0 * self.scale
        } else {
            0.0
        };
        vec2(line * aspect + plate, line + plate)
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
        let look = crate::widget_layer::look_of(self.arena, index, theme, self.scale);
        let (style, font) = (&look.style, look.font.clone());
        if let Some(state) = crate::text::state(self.eng) {
            let request = crate::widget_text::text_request(widget, text, None, &font, style);
            return state
                .borrow_mut()
                .shape_for_egui(&self.painter.ctx().clone(), &request)
                .size;
        }
        self.painter
            .layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)
            .size()
    }
}

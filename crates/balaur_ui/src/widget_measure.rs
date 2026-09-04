//! What a widget needs before anything draws it.
//!
//! egui measures while drawing, which is a frame too late for a container
//! deciding where its children go. This walks the same tree the draw will and
//! asks the font atlas instead, so a row sizes itself to a label that changed
//! this frame rather than to the one it showed last.

use crate::theme::family;
use crate::widget_arrange::padding_of;
use crate::widget_layer::{caption, lays_out, theme_of, Placed, Widget};
use crate::widget_theme::WidgetTheme;
use balaur_core::Engine;
use egui::vec2;
use std::collections::HashMap;
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
    seen: HashMap<usize, egui::Vec2>,
}

impl<'a> Measure<'a> {
    pub(crate) fn new(eng: &'a Engine, arena: &'a [Placed], ui: &egui::Ui, scale: f32) -> Self {
        Self {
            eng,
            arena,
            painter: ui.painter().clone(),
            padding: ui.spacing().button_padding * 2.0,
            scale,
            seen: HashMap::new(),
        }
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
        let kind = widget.kind.clone();
        match kind.as_str() {
            // A script fills its own rect, and a scroll is meant to clip: both
            // answer with their stated size or with nothing.
            "draw" | "scroll" => egui::Vec2::ZERO,
            // A picture knows its own size, so a row can divide by it.
            "image" => {
                crate::widgets::texture_of(self.eng, &self.painter.ctx().clone(), &widget.source)
                    .map_or(egui::Vec2::ZERO, |texture| {
                        crate::widget_layer::image_size(
                            vec2(widget.width, widget.height) * self.scale,
                            texture.size_vec2(),
                        )
                    })
            }
            "button" => {
                let text = self.text(widget);
                // egui's own button padding, which is what it will draw with.
                text + self.padding
            }
            "label" => self.text(widget),
            "tab" => {
                let strip = self.strip(index);
                let pages = self.widest_child(index, theme);
                let gap = widget.gap * self.scale;
                vec2(strip.x.max(pages.x), strip.y + gap + pages.y)
            }
            _ if lays_out(&kind) => self.container(index, theme),
            _ => self.text(widget),
        }
    }

    /// A row or column: its children end to end along its axis, the widest
    /// across, plus the gaps between them and its own padding.
    fn container(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let placed = &self.arena[index];
        let widget = &placed.widget;
        let row = widget.kind == "row";
        let children = placed.children.clone();
        let caption = if widget.kind == "panel" {
            self.text(widget)
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
        let pad = padding_of(widget, &theme.style(&widget.kind), self.scale);
        inner + egui::Vec2::splat(pad * 2.0)
    }

    /// A tab's strip: every page's label side by side, as buttons.
    fn strip(&mut self, index: usize) -> egui::Vec2 {
        let placed = &self.arena[index];
        let widget = placed.widget.clone();
        let gap = (widget.gap * self.scale).max(4.0);
        let padding = self.padding;
        let mut width = 0.0f32;
        let mut height: f32 = 0.0;
        for (slot, child) in placed.children.iter().enumerate() {
            let page = &self.arena[*child];
            let label = if page.widget.text.is_empty() {
                page.name.clone()
            } else {
                page.widget.text.clone()
            };
            let size = self.galley(&label, &widget) + padding;
            width += size.x + if slot > 0 { gap } else { 0.0 };
            height = height.max(size.y);
        }
        vec2(width, height)
    }

    /// The biggest page, so a tab does not resize as it is clicked through.
    fn widest_child(&mut self, index: usize, theme: &Rc<WidgetTheme>) -> egui::Vec2 {
        let children = self.arena[index].children.clone();
        let mut size = egui::Vec2::ZERO;
        for child in children {
            size = size.max(self.of(child, theme));
        }
        size
    }

    fn text(&self, widget: &Widget) -> egui::Vec2 {
        let caption = caption(self.eng, widget);
        if caption.is_empty() {
            return egui::Vec2::ZERO;
        }
        self.galley(&caption, widget)
    }

    /// One line of text, unwrapped: what the widget needs to show it whole.
    fn galley(&self, text: &str, widget: &Widget) -> egui::Vec2 {
        let font = egui::FontId::new(widget.font_size * self.scale, family("ui"));
        self.painter
            .layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)
            .size()
    }
}

//! Immediate-mode UI for Luau scripts, rendered with egui.
//!
//! The plugin exposes a `ui` Lua module. Scripts implement a `draw_ui`
//! lifecycle method; every frame the windowed backend calls [`run_pass`],
//! which invokes `draw_ui` on every instance (in entity order) inside the
//! frame's egui pass. The module's functions build panels and widgets
//! against the pass's current `egui::Ui`, so Lua composes interfaces exactly
//! the way Rust egui code does:
//!
//! ```luau
//! function Editor:draw_ui()
//!     ui.top_panel("bar", { height = 56, fill = "#20242a" }, function()
//!         if ui.pill("Scene", { active = true }) then ... end
//!     end)
//! end
//! ```
//!
//! Widgets take their colors per call (usually from a Lua-side token table),
//! so entire themes live in scripts and hot reload with them.

mod bridge;
mod theme;
mod widget_bindings;
mod widget_layer;
mod widgets;

use anyhow::Result;
use balaur_core::{App, Engine, Plugin};
use std::collections::{HashMap, HashSet};

pub use theme::ThemeTokens;
pub use widget_layer::{Widget, WidgetLayer};

/// Per-engine UI state: pending theme, persistent text-edit buffers, and
/// the global UI scale (all widget metrics multiply by it, so scripts keep
/// authoring in design pixels).
pub struct UiState {
    pub theme: ThemeTokens,
    pub theme_dirty: bool,
    pub fonts_installed: bool,
    pub scale: f32,
    pub text_buffers: HashMap<String, String>,
    pub focused_once: HashSet<String>,
    pub textures: HashMap<String, egui::TextureHandle>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            theme: ThemeTokens::default(),
            theme_dirty: false,
            fonts_installed: false,
            scale: 1.0,
            text_buffers: HashMap::new(),
            focused_once: HashSet::new(),
            textures: HashMap::new(),
        }
    }
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn name(&self) -> &'static str {
        "ui"
    }

    fn build(&mut self, app: &mut App) -> Result<()> {
        app.engine.insert_resource(UiState::default());
        app.engine.insert_resource(WidgetLayer::default());
        widgets::install(app)?;
        widget_layer::register(app);
        Ok(())
    }
}

/// Run one UI pass: apply pending theme changes, then let scripts draw.
/// Called by a windowed backend once per frame with the frame's egui
/// context. Does nothing when the `UiPlugin` is not installed.
pub fn run_pass(engine: &Engine, ctx: &egui::Context) {
    let Some(state) = engine.try_resource::<UiState>() else {
        return;
    };
    {
        let mut state = state.borrow_mut();
        if !state.fonts_installed {
            // Fonts registered mid-pass only take effect next pass; skip one
            // frame of drawing so widgets never see unbound families.
            theme::install_fonts(engine, ctx);
            state.fonts_installed = true;
            return;
        }
        if state.theme_dirty {
            theme::apply(&state.theme, ctx);
            state.theme_dirty = false;
        }
    }
    let scale = state.borrow().scale;
    bridge::enter_pass(ctx, scale);
    if let Some(host) = engine.scripts() {
        host.call_all("draw_ui", ());
    }
    widget_layer::draw(engine, ctx, scale);
    bridge::leave_pass();
}

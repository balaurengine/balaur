//! Immediate-mode UI for scripts, rendered with egui.
//!
//! The plugin exposes a `ui` module. Scripts implement a `draw_ui`
//! lifecycle method; every frame the windowed backend calls [`run_pass`],
//! which invokes `draw_ui` on every instance (in entity order) inside the
//! frame's egui pass. The module's functions build panels and widgets
//! against the pass's current `egui::Ui`, so a script composes interfaces
//! exactly the way Rust egui code does (shown here in Luau):
//!
//! ```luau
//! function Editor:draw_ui()
//!     ui.top_panel("bar", { height = 56, fill = "#20242a" }, function()
//!         if ui.pill("Scene", { active = true }) then ... end
//!     end)
//! end
//! ```
//!
//! Widgets take their colors per call (usually from a script-side token
//! table), so entire themes live in scripts and hot reload with them.

mod bridge;
mod theme;
mod widget_bindings;
mod widget_layer;
mod widgets;

use anyhow::Result;
use balaur_core::{App, Engine, Plugin};
use std::collections::{HashMap, HashSet};

pub use theme::ThemeTokens;
pub use widget_layer::{Widget, WidgetLayerConfig};
pub use widgets::{ANCHORS, FONTS, MODIFIERS, WIDGET_KINDS};

/// What scripts ask the UI to look like: the theme tokens `ui.set_theme`
/// writes, and the global UI scale (all widget metrics multiply by it, so
/// scripts keep authoring in design pixels).
///
/// [`run_pass`] applies a pending theme and clears `changed`; nothing else
/// owns these values.
pub struct UiConfig {
    pub theme: ThemeTokens,
    /// Set by `ui.set_theme`, cleared once the theme reaches egui.
    pub changed: bool,
    pub scale: f32,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: ThemeTokens::default(),
            changed: false,
            scale: 1.0,
        }
    }
}

/// The plugin's own cache, all of it derived from what it has already drawn:
/// whether fonts are bound to the egui context yet, the persistent text-edit
/// buffers behind `ui.text_field` and `ui.code_editor`, which of them have
/// taken focus once, and the textures `ui.image` has uploaded.
///
/// Split from [`UiConfig`] by ownership: nothing outside this crate writes
/// any of it, whereas every field of `UiConfig` comes from a script.
#[derive(Default)]
pub struct UiState {
    pub fonts_installed: bool,
    pub text_buffers: HashMap<String, String>,
    pub focused_once: HashSet<String>,
    pub textures: HashMap<String, egui::TextureHandle>,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn name(&self) -> &'static str {
        "ui"
    }

    fn build(&mut self, app: &mut App) -> Result<()> {
        app.engine.insert_resource(UiConfig::default());
        app.engine.insert_resource(UiState::default());
        app.engine.insert_resource(WidgetLayerConfig::default());
        widgets::install_ui_api(app)?;
        widget_layer::register_widget_component(app);
        Ok(())
    }
}

/// Run one UI pass: apply pending theme changes, then let scripts draw.
/// Called by a windowed backend once per frame with the frame's egui
/// context. Does nothing when the `UiPlugin` is not installed.
pub fn run_pass(eng: &Engine, ctx: &egui::Context) {
    let Some(state) = eng.try_resource::<UiState>() else {
        return;
    };
    {
        let mut state = state.borrow_mut();
        if !state.fonts_installed {
            // Fonts registered mid-pass only take effect next pass; skip one
            // frame of drawing so widgets never see unbound families.
            theme::load_fonts(eng, ctx);
            state.fonts_installed = true;
            return;
        }
    }
    // A second lookup and borrow, deliberately: the pending theme is the
    // script's to set and this crate's cache is not, so they are two entries.
    let Some(config) = eng.try_resource::<UiConfig>() else {
        return;
    };
    {
        let mut config = config.borrow_mut();
        if config.changed {
            theme::apply(&config.theme, ctx);
            config.changed = false;
        }
    }
    let scale = config.borrow().scale;
    bridge::enter_pass(ctx, scale);
    if let Some(host) = eng.script_host() {
        host.call_all("draw_ui");
    }
    widget_layer::draw(eng, ctx, scale);
    bridge::leave_pass();
}

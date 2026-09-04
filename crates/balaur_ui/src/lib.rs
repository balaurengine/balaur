//! Immediate-mode UI for scripts, rendered with egui.
//!
//! The plugin exposes a `ui` module. Scripts implement a `draw_ui`
//! lifecycle method; every frame the windowed backend calls [`run_pass`],
//! which invokes `draw_ui` on every instance (in entity order) inside the
//! frame's egui pass. The module's functions build panels and widgets
//! against the pass's current `egui::Ui`, so a script composes interfaces
//! exactly the way Rust egui code does:
//!
//! ```rune
//! pub fn draw_ui(this) {
//!     ui::top_panel("bar", #{ height: 56.0, fill: "#20242a" }, || {
//!         if ui::pill("Scene", #{ active: true }) { /* ... */ }
//!     });
//! }
//! ```
//!
//! Widgets take their colors per call (usually from a script-side token
//! table), so entire themes live in scripts and hot reload with them.

mod bridge;
mod theme;
mod widget_arrange;
mod widget_bindings;
mod widget_layer;
mod widget_layout;
mod widget_measure;
mod widget_theme;
mod widgets;

use anyhow::Result;
use balaur_core::Engine;
use std::collections::{HashMap, HashSet};

pub use theme::ThemeTokens;
pub use widget_layer::{Move, UiFocus, Widget, WidgetLayerConfig};
pub use widget_theme::WidgetTheme;
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
    /// The value each seeded field was last filled from, so a field re-seeds
    /// when its source changes but not while someone is typing into it.
    pub text_seeds: HashMap<String, String>,
    pub focused_once: HashSet<String>,
    pub textures: HashMap<String, egui::TextureHandle>,
}

pub struct UiPlugin {
    manifest: balaur_plugin::Manifest,
}

impl Default for UiPlugin {
    fn default() -> Self {
        Self {
            manifest: balaur_plugin::Manifest::new("ui", env!("CARGO_PKG_VERSION")),
        }
    }
}

impl balaur_plugin::Plugin for UiPlugin {
    fn manifest(&self) -> &balaur_plugin::Manifest {
        &self.manifest
    }

    fn declare(&mut self, reg: &mut balaur_plugin::Registry<'_>) -> Result<()> {
        let app = reg.app();
        app.engine.insert_resource(UiConfig::default());
        app.engine.insert_resource(UiState::default());
        app.engine.insert_resource(WidgetLayerConfig::default());
        app.engine.insert_resource(UiFocus::default());
        app.register_asset_type(
            widget_theme::ASSET_TYPE,
            "themes",
            widget_theme::ASSET_DOC,
            |value| {
                Ok(std::rc::Rc::new(widget_theme::parse(value)) as std::rc::Rc<dyn std::any::Any>)
            },
        );
        widgets::install_ui_api(app)?;
        widget_layer::register_widget_component(app);
        widget_layer::register_widget_presets(app)?;
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
    let roles = eng.resource::<UiConfig>().borrow().theme.roles.clone();
    bridge::enter_pass(ctx, scale, roles);
    if let Some(host) = eng.script_host() {
        host.call_all("draw_ui");
    }
    widget_layer::draw(eng, ctx, scale);
    bridge::leave_pass();
}

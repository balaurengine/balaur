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
mod splash;
mod text;
mod theme;
mod vocabulary;
mod widget_arrange;
mod widget_bindings;
mod widget_input;
mod widget_kinds;
mod widget_layer;
mod widget_layout;
mod widget_measure;
mod widget_schema;
mod widget_text;
mod widget_theme;
mod widgets;

use anyhow::Result;
use balaur_core::Engine;
use std::collections::{HashMap, HashSet};

pub use theme::ThemeTokens;
pub use widget_input::{WidgetInputBuffer, WidgetInputSnapshot};
pub use widget_layer::{Move, Surface, UiFocus, Widget, WidgetLayerConfig};
pub use widget_theme::WidgetTheme;

/// Where the layer last drew a widget, in device pixels, or `None` for one
/// it did not draw last frame. What `ui.widget_rect` answers a script.
#[must_use]
pub fn widget_rect(entity: balaur_core::hecs::Entity) -> Option<egui::Rect> {
    widget_arrange::drawn_at(entity)
}
pub use widgets::{ALIGNS, ANCHORS, FONT_STYLES, FONTS, MODIFIERS, PILL_ALIGNS, WIDGET_KINDS};

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
    /// A finger down on a `scroll` with a deadzone: where it landed and the
    /// offset the scroll had then, until it lifts.
    pub scroll_drags: HashMap<u64, (egui::Pos2, egui::Vec2)>,
    pub textures: HashMap<String, egui::TextureHandle>,
    /// The asset generation `textures` was filled at: an image reloaded on
    /// disk is a new picture under the same path, so the cache goes with it.
    pub texture_generation: u64,
    /// Set by [`forget_scene`], consumed by the next [`run_pass`]: egui's own
    /// memory is keyed by entity, and dropping it needs the context.
    pub forget_egui: bool,
}

/// Drop everything the plugin cached against a scene that is being rebuilt.
///
/// The textures and the text buffers are keyed by strings a scene chose, and
/// egui's per-widget memory — scroll offsets, area sizes — is keyed by entity,
/// which a respawned node reuses. Nothing inside the engine calls this yet; a
/// host that reloads a scene should.
pub fn forget_scene(eng: &Engine) {
    let Some(state) = eng.try_resource::<UiState>() else {
        return;
    };
    let mut state = state.borrow_mut();
    state.textures.clear();
    state.text_buffers.clear();
    state.text_seeds.clear();
    state.focused_once.clear();
    state.forget_egui = true;
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
        reg.insert_resource(UiConfig::default());
        reg.insert_resource(UiState::default());
        reg.insert_resource(WidgetLayerConfig::default());
        reg.insert_resource(UiFocus::default());
        reg.register_asset_type(
            widget_theme::ASSET_TYPE,
            "themes",
            widget_theme::ASSET_DOC,
            |value| {
                Ok(std::rc::Rc::new(widget_theme::parse(value)) as std::rc::Rc<dyn std::any::Any>)
            },
        );
        widgets::install_ui_api(reg)?;
        widget_input::register(reg);
        widget_layer::register_widget_component(reg);
        widget_layer::register_widget_presets(reg)?;
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
            let faces = theme::font_faces(eng);
            theme::load_fonts(ctx, &faces);
            let locale = balaur_core::strings::locale(eng);
            eng.insert_resource(text::TextState::new(&faces, &locale));
            state.fonts_installed = true;
            // A host that reruns the pass (`Context::will_discard`) draws
            // this frame with the fonts bound.
            ctx.request_discard("fonts installed");
            return;
        }
        if std::mem::take(&mut state.forget_egui) {
            // Scroll offsets and area sizes are keyed by entity, and a
            // respawned node inherits the index the freed one had.
            ctx.memory_mut(|memory| memory.data.clear());
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
    // Painting order is egui's `Order` — widgets are `Middle`, an overlay is
    // `Foreground` — so what is on top does not depend on which ran first.
    widget_layer::draw(eng, ctx, scale);
    if let Some(host) = eng.script_host() {
        host.call_all("draw_ui");
    }
    // Over everything, scripts' overlays included, for as long as it lasts.
    splash::draw(eng, ctx);
    bridge::leave_pass();
}

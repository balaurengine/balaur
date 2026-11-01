//! Game and editor UI as scene nodes, painted with egui.
//!
//! A UI is `widget` components on scene nodes: the widget layer lays each
//! subtree out with taffy, shapes its text with `balaur_text` and paints it
//! through egui once a pass. A script fills and answers nodes; it never
//! draws. The plugin's `ui` module is what a script asks of the UI
//! and tells it: where a node drew, focus, the scale, the theme, the
//! clipboard, when the next pass runs, and the pooled rows a list is built
//! from.
// clippy misreads `thread_local!`'s `const` form on Android's emulated TLS.
#![cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local))]

mod access;
mod api;
mod bridge;
mod chord;
mod clipboard;
pub mod contrast;
mod images;
mod loading;
mod pacing;
pub mod palette;
mod routing;
mod splash;
mod theme;
mod touch;
mod vocabulary;
mod widget;

use anyhow::Result;
use balaur_core::Engine;
use std::collections::HashMap;

pub use access::{AccessTree, act, enable_access, take_access_update};
pub use loading::Loading;
pub use pacing::{NextFrame, Pacing, honour_lazy, next_frame, wants_pass};
pub use routing::{pointer_is_ui, takes_point};
pub use theme::ThemeTokens;
pub use touch::draw_touch_controls;
pub use widget::input::{
    ACTIVATE_EVENT, BLUR_EVENT, CARET_EVENT, CHANGE_EVENT, CLICK_EVENT, CLOSE_REQUEST_EVENT,
    CLOSED_EVENT, COMMIT_EVENT, DOUBLE_CLICK_EVENT, DROP_EVENT, FOCUS_EVENT, FOLD_EVENT,
    GUTTER_EVENT, LINK_EVENT, MARK_EVENT, MOVE_EVENT, OPENED_EVENT, PASTED_EVENT,
    PASTED_IMAGE_EVENT, RESIZED_EVENT, SCROLLED_EVENT, SUBMIT_EVENT, WidgetInputBuffer,
    WidgetInputSnapshot, click, edit, submit,
};
pub use widget::inspector::reread;
pub use widget::node::{Move, Surface, UiFocus, Widget, WidgetLayerConfig};
pub use widget::theme::WidgetTheme;

/// Where the layer last drew a widget, in design pixels, or `None` for one
/// it did not draw last frame. What `ui.widget_rect` answers a script.
#[must_use]
pub fn widget_rect(entity: balaur_core::hecs::Entity) -> Option<egui::Rect> {
    widget::arrange::drawn_at(entity)
}

/// What a widget took when the layer last drew it, in design pixels: the
/// box it was given, unless it drew past it. `None` for one it did not draw.
#[must_use]
pub fn widget_drawn_size(entity: balaur_core::hecs::Entity) -> Option<egui::Vec2> {
    widget::arrange::drawn_size(entity)
}

/// The smallest a control may be where a finger is what reaches it, in the
/// points a platform measures its own guidance in. Apple asks for 44 and
/// Material for 48; 44 is the smaller promise both are met by.
///
/// Reached by the zoom rather than by a floor on each control. A floor grows
/// a control's box without growing its glyph or the bar holding it, so the
/// bar overflows and the icons stop matching; the zoom grows all three.
pub const TOUCH_TARGET: f32 = 44.0;

pub use api::{ALIGNS, ANCHORS, CLASSES, FONT_STYLES, ITEM_ALIGNS, MODIFIERS, WIDGET_KINDS};

/// Where this project puts the lines between the screen classes, and whether
/// the reader's own text size counts. Read once, after the project has
/// loaded, because the plugin is built before there is a project to read.
pub(crate) struct UiProject {
    read: bool,
    pub(crate) classes: balaur_core::facts::ClassLines,
    pub(crate) system_text_size: bool,
}

impl Default for UiProject {
    fn default() -> Self {
        let fallback = balaur_core::project::UiSettings::default();
        Self {
            read: false,
            classes: fallback.classes,
            system_text_size: fallback.system_text_size,
        }
    }
}

/// One long press, not two. egui holds a still touch past its own click
/// length and calls it a secondary click, which is what opens a context menu;
/// the tick derives `input.long_press()` from `[input] long_press_seconds`.
/// Reading that one setting into egui is what keeps the two agreeing.
fn apply_long_press(eng: &balaur_core::Engine, ctx: &egui::Context) {
    let Some(seconds) = balaur_core::settings::get(eng, "input/long_press_seconds")
        .as_ref()
        .and_then(toml::Value::as_float)
    else {
        return;
    };
    ctx.options_mut(|options| {
        if (options.input_options.max_click_duration - seconds).abs() > f64::EPSILON {
            options.input_options.max_click_duration = seconds;
        }
    });
}

/// Whether this project lets the reader's own preferred text size grow its
/// UI. `[ui] system_text_size`, read once with the rest of the table.
#[must_use]
pub fn takes_system_text_size(eng: &balaur_core::Engine) -> bool {
    eng.try_resource::<UiProject>()
        .is_none_or(|held| held.borrow().system_text_size)
}

/// Where this project puts the lines between the screen classes.
pub(crate) fn class_lines(eng: &balaur_core::Engine) -> balaur_core::facts::ClassLines {
    eng.try_resource::<UiProject>()
        .map_or_else(Default::default, |held| held.borrow().classes)
}

/// Take `[ui]` on the first tick that has a project to take it from: the
/// scale seeds the zoom, and the rest is held for whoever asks.
fn read_project_settings_system(eng: &balaur_core::Engine, _dt: f32) {
    let held = eng.resource::<UiProject>();
    if held.borrow().read
        || eng
            .try_resource::<balaur_core::project::ProjectManifest>()
            .is_none()
    {
        return;
    }
    let settings = balaur_core::project::UiSettings::from_settings(eng);
    {
        let mut held = held.borrow_mut();
        held.read = true;
        held.classes = settings.classes;
        held.system_text_size = settings.system_text_size;
    }
    // Seeded, not owned: `ui.set_scale` changes it afterwards, the way
    // `window.set_window_mode` changes what `[window] mode` opened with. And
    // a script that already asked in its `init` is not overwritten by it.
    let config = eng.resource::<UiConfig>();
    let mut config = config.borrow_mut();
    if !config.asked {
        config.scale = settings.scale.clamp(0.25, 3.0);
    }
}

/// What scripts ask the UI to look like: the theme tokens `ui.set_theme`
/// writes, and the UI scale, which is egui's own zoom factor. A design pixel
/// is a point, so nothing multiplies by the scale: the backend hands it to
/// [`egui::Context::set_zoom_factor`] and egui lays the whole pass out in it,
/// its own widgets included.
///
/// [`run_pass`] applies a pending theme and clears `changed`; nothing else
/// owns these values.
pub struct UiConfig {
    pub theme: ThemeTokens,
    /// The document `ui.set_theme` built [`UiConfig::theme`] from, so the
    /// same one again -- each screen of an app setting its theme -- skips
    /// deriving the palette a second time.
    pub theme_doc: Option<toml::Value>,
    /// Set by `ui.set_theme`, cleared once the theme reaches egui.
    pub changed: bool,
    pub scale: f32,
    /// Whether a script has set the scale. `[ui] scale` seeds it on the first
    /// tick, which is after a script's `init` has run, so a seed that did not
    /// ask would overwrite what the script just asked for.
    pub asked: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: ThemeTokens::default(),
            theme_doc: None,
            changed: false,
            scale: 1.0,
            asked: false,
        }
    }
}

/// The plugin's own cache, all of it derived from what it has already drawn:
/// whether fonts are bound to the egui context yet, what each field holds
/// while it is typed into, and the textures `ui.image` has uploaded.
///
/// Split from [`UiConfig`] by ownership: nothing outside this crate writes
/// any of it, whereas every field of `UiConfig` comes from a script.
#[derive(Default)]
pub struct UiState {
    pub fonts_installed: bool,
    /// The shaper's fonts generation egui's faces were taken under: a face
    /// file written since is read again into both.
    fonts_taken_at: u64,
    /// The asset generation the widgets' text styles were last read under.
    text_styles_read_at: u64,
    /// Each `text_field`, `text_area`, `code` node and number being typed:
    /// its text, caret, undo and layout, by its egui id.
    pub(crate) fields: HashMap<egui::Id, widget::typing::Field>,
    /// Text `ui::set_clipboard` asked for, handed to the platform by the next
    /// pass: a script runs between passes, and only a pass holds the context.
    pub clipboard: Option<String>,
    /// The text the last pass saw pasted, empty when it saw none.
    pub pasted: String,
    /// The image the last pass saw pasted where no field took it, until a
    /// script writes it out with `ui::save_pasted_image`.
    pub pasted_image: Option<clipboard::Image>,
    /// Whether a widget held the keyboard when the last pass ended.
    pub wants_keyboard: bool,
    /// A finger down on a `scroll` with a deadzone, until it lifts.
    pub scroll_drags: HashMap<u64, ScrollDrag>,
    /// A `scroll` still moving after the finger lifted: how fast, in points
    /// per second, and where the offset has got to. Dropped once it stops.
    pub scroll_flings: HashMap<u64, (egui::Vec2, egui::Vec2)>,
    pub textures: HashMap<String, egui::TextureHandle>,
    /// Pictures a preloaded scene names, decoding on a worker thread: each
    /// one's answer and the sampler it is uploaded with.
    pub(crate) decoding: HashMap<String, images::Decoding>,
    /// The asset generation `textures` was filled at: an image reloaded on
    /// disk is a new picture under the same path, so the cache goes with it.
    pub texture_generation: u64,
    /// Set by [`forget_scene`], consumed by the next [`run_pass`]: egui's own
    /// memory is keyed by entity, and dropping it needs the context.
    pub forget_egui: bool,
}

/// A finger dragging a `scroll`: where it started, what the offset was then,
/// and how fast it is moving now, so a lift can carry on at that speed.
pub struct ScrollDrag {
    pub from: egui::Pos2,
    pub base: egui::Vec2,
    pub last: egui::Pos2,
    /// Points per second, smoothed: one jittery frame should not decide how
    /// far a flick throws the list.
    pub velocity: egui::Vec2,
}

/// Put a message on screen that stacks in the corner and leaves on its own.
///
/// A `toast` node under the scene root, holding nothing but its text: what
/// the `toast` binding action runs, so a scene shows one with no script.
///
/// # Errors
/// If the widget component refuses the node, which means the plugin that
/// declares it is not in this build.
pub fn toast(eng: &Engine, text: &str) -> anyhow::Result<()> {
    let root = eng.root();
    let entity = eng.spawn_node("Toast", root);
    let params = toml::toml! { kind = "toast" text = text anchor = "top_right" x = 16.0 y = 16.0 };
    balaur_core::components::add(eng, entity, "widget", Some(&params.into()))
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
    state.fields.clear();
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
        reg.insert_resource(UiProject::default());
        reg.add_system(balaur_core::Stage::First, read_project_settings_system);
        reg.insert_resource(UiState::default());
        reg.insert_resource(Pacing::default());
        reg.insert_resource(WidgetLayerConfig::default());
        reg.insert_resource(widget::pool::PoolState::default());
        reg.insert_resource(widget::inspector::InspectorState::default());
        reg.insert_resource(UiFocus::default());
        reg.insert_resource(AccessTree::default());
        reg.insert_resource(routing::UiHits::default());
        reg.insert_resource(routing::PointerRoute::default());
        balaur_text::glyph::install(reg);
        balaur_text::fonts::read_system_fonts_ahead(reg);
        balaur_text::font::register_font_asset(reg);
        balaur_text::text_style::register_text_style_asset(reg);
        reg.register_asset_type(
            widget::theme::ASSET_TYPE,
            "themes",
            widget::theme::ASSET_DOC,
            |value| {
                Ok(std::rc::Rc::new(widget::theme::parse(value)) as std::rc::Rc<dyn std::any::Any>)
            },
        );
        api::install_ui_api(&mut *reg.script_module("ui")?);
        widget::input::register(reg);
        // Before `widget`, so a scene gives a node these first and builds
        // its widget once, with their rows.
        widget::beside::register_all(reg);
        widget::schema::register_widget_component(reg);
        widget::presets::register_widget_presets(reg)?;
        balaur_core::preload::on_component(
            reg.engine(),
            <widget::properties::WidgetProperties as balaur_core::components::derived::Component>::NAME,
            balaur_core::preload::Warming {
                preload: images::preload_widget,
                unload: images::unload_widget,
            },
        );
        Ok(())
    }
}

/// Run one UI pass: apply pending theme changes, then let scripts draw.
/// Called by a windowed backend once per frame with the frame's egui
/// context. Does nothing when the `UiPlugin` is not installed.
pub fn run_pass(eng: &Engine, ctx: &egui::Context) {
    // What a pass cost, for `pacing` to decide the next one by. Presentation,
    // outside the simulation, so it reads a clock the digest never sees.
    #[allow(clippy::disallowed_methods)]
    let started = balaur_core::time::Instant::now();
    pass(eng, ctx);
    pacing::note_pass(eng, started.elapsed());
}

/// The UI pass with no window: the one a windowed backend makes, at its
/// pace, on a screen `width` by `height` points that nothing paints. What
/// lets a headless editor run the checks that read its own shell.
pub fn pass_without_window(app: &mut balaur_core::App, width: f32, height: f32) {
    honour_lazy(&app.engine);
    let ctx = egui::Context::default();
    app.add_system(balaur_core::Stage::Render, move |eng, _| {
        if !wants_pass(eng, &ctx, false) {
            return;
        }
        let scale = eng
            .try_resource::<UiConfig>()
            .map_or(1.0, |config| config.borrow().scale);
        ctx.set_zoom_factor(scale);
        ctx.begin_pass(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            ..Default::default()
        });
        run_pass(eng, &ctx);
        // A renderer uploads these; unapplied, the next pass panics.
        ctx.end_pass().textures_delta.clear();
    });
}

fn pass(eng: &Engine, ctx: &egui::Context) {
    let Some(state) = eng.try_resource::<UiState>() else {
        return;
    };
    // Pictures a preload decoded since the last pass, up before anything draws.
    images::settle(eng, ctx);
    {
        let mut state = state.borrow_mut();
        // Once a pass: egui takes the families given last pass, the shaper
        // reads the fonts a widget asked egui for and follows the files under
        // `fonts/`, and egui is given families again whenever it read any.
        theme::take_fonts(eng, ctx);
        widget::schema::reread_text_styles(eng, &mut state.text_styles_read_at);
        let shaper = balaur_text::shaper(eng);
        let generation = shaper.borrow().fonts_generation;
        if state.fonts_installed && state.fonts_taken_at != generation {
            theme::load_fonts(ctx, &shaper.borrow());
            state.fonts_taken_at = generation;
        }
        if !state.fonts_installed {
            // Fonts registered mid-pass only take effect next pass; skip one
            // frame of drawing so widgets never see unbound families.
            theme::load_fonts(ctx, &shaper.borrow());
            state.fonts_installed = true;
            state.fonts_taken_at = generation;
            // A host that reruns the pass draws this frame with the fonts
            // bound; three passes, since a root's first pass only measures.
            ctx.options_mut(|options| {
                options.max_passes = std::num::NonZeroUsize::new(3).unwrap_or(options.max_passes);
            });
            ctx.request_discard("fonts installed");
            drop(state);
            // The one thing this frame can still show: a splash is a picture
            // and a bar, and neither needs a font bound to paint.
            splash::draw(eng, ctx);
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
    // Before the classes are read by anything: the theme answers by them, and
    // so does the floor egui's own controls take.
    widget::theme::set_pass_classes(&widget::arena::begin_classes(eng));
    apply_long_press(eng, ctx);
    bridge::enter_pass(ctx);
    routing::pass_begins(eng);
    // An inspector's rows are nodes it fills, so they are filled before the
    // layer reads the tree.
    widget::inspector::refresh(eng);
    clipboard::read_paste(ctx);
    widget::layer::draw(eng, ctx);
    // Over everything, for as long as it lasts.
    splash::draw(eng, ctx);
    {
        let mut state = state.borrow_mut();
        if let Some(text) = state.clipboard.take() {
            ctx.copy_text(text);
        }
        clipboard::write_copies(ctx);
        state.pasted = ctx.input(|i| {
            i.events
                .iter()
                .rev()
                .find_map(|e| match e {
                    egui::Event::Paste(text) => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_default()
        });
        state.wants_keyboard = ctx.egui_wants_keyboard_input();
    }
    widget::input::hear_pasted_image(eng, ctx);
    routing::pass_drew(eng, ctx);
    bridge::leave_pass();
}

//! Text drawn in the world, from the same shaper the widgets use.
//!
//! `balaur_ui` shapes a run into glyph quads over its atlas; this turns those
//! quads into a mesh and the atlas into a texture kiss3d can sample, so a name
//! over a character and a label in a panel come from one bitmap and one set of
//! font rules.

use balaur_core::Engine;

use crate::vocabulary::{keys as k, words as w};

/// Where a block sits relative to the point it was drawn at.
///
/// Its own enum rather than the shaper's: the buffer and its bindings are in
/// every build, and the shaper comes with the windowed backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
    Left,
    Right,
    Justify,
}

impl Align {
    /// The alignment a scene or a script names; anything else starts.
    pub(crate) fn of(word: &str) -> Self {
        match word {
            w::CENTER => Self::Center,
            w::END => Self::End,
            w::LEFT => Self::Left,
            w::RIGHT => Self::Right,
            w::JUSTIFY => Self::Justify,
            _ => Self::Start,
        }
    }

    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Start => w::START,
            Self::Center => w::CENTER,
            Self::End => w::END,
            Self::Left => w::LEFT,
            Self::Right => w::RIGHT,
            Self::Justify => w::JUSTIFY,
        }
    }
}

/// Every setting of a block past its font and size, in the words a scene
/// spells them; the windowed backend reads them into the shaper's own types.
/// How the font is shaped is the `font` asset's.
#[derive(Clone, Debug, PartialEq)]
pub struct Shaping {
    pub font_style: String,
    pub line_break: String,
    /// End a block cut short with an ellipsis; needs `max_width`.
    pub truncate: bool,
    pub truncate_at: String,
    /// Zero keeps every line.
    pub max_lines: u32,
    /// Font pixels the lines stop at; zero has no limit.
    pub max_height: f32,
    pub tab_width: u16,
}

impl Default for Shaping {
    fn default() -> Self {
        Self {
            font_style: w::NORMAL.into(),
            line_break: w::WORD_OR_GLYPH.into(),
            truncate: false,
            truncate_at: w::END.into(),
            max_lines: 0,
            max_height: 0.0,
            tab_width: 8,
        }
    }
}

impl Shaping {
    /// The settings a component's or a script's table names, over the defaults.
    pub(crate) fn from_params(params: &toml::Value) -> Self {
        let base = Self::default();
        let word = |key: &str, fallback: &str| {
            params
                .get(key)
                .and_then(toml::Value::as_str)
                .unwrap_or(fallback)
                .to_string()
        };
        let flag = |key: &str| {
            params
                .get(key)
                .and_then(toml::Value::as_bool)
                .unwrap_or(false)
        };
        let number = |key: &str| {
            params
                .get(key)
                .and_then(balaur_core::components::as_f64)
                .map_or(0.0, |v| v as f32)
        };
        Self {
            font_style: word(k::FONT_STYLE, &base.font_style),
            line_break: word(k::LINE_BREAK, &base.line_break),
            truncate: flag(k::TRUNCATE),
            truncate_at: word(k::TRUNCATE_AT, &base.truncate_at),
            max_lines: number(k::MAX_LINES).max(0.0) as u32,
            max_height: number(k::MAX_HEIGHT).max(0.0),
            tab_width: params
                .get(k::TAB_WIDTH)
                .and_then(balaur_core::components::as_f64)
                .map_or(base.tab_width, |v| v.clamp(1.0, 64.0) as u16),
        }
    }

    /// The settings back into the table a scene file would have written.
    pub(crate) fn put_into(&self, out: &mut toml::map::Map<String, toml::Value>) {
        let text = |v: &str| toml::Value::String(v.to_string());
        let pairs = [
            (k::FONT_STYLE, text(&self.font_style)),
            (k::LINE_BREAK, text(&self.line_break)),
            (k::TRUNCATE, toml::Value::Boolean(self.truncate)),
            (k::TRUNCATE_AT, text(&self.truncate_at)),
            (
                k::MAX_LINES,
                toml::Value::Integer(i64::from(self.max_lines)),
            ),
            (
                k::MAX_HEIGHT,
                toml::Value::Float(f64::from(self.max_height)),
            ),
            (
                k::TAB_WIDTH,
                toml::Value::Integer(i64::from(self.tab_width)),
            ),
        ];
        for (key, value) in pairs {
            out.insert(key.to_string(), value);
        }
    }

    /// The schema lines `text2d` and `text3d` share for wrapping and cutting.
    pub(crate) fn schema() -> Vec<(&'static str, String)> {
        let options = crate::vocabulary::options;
        vec![
            (k::MAX_HEIGHT, r#"{ type = "float", default = 0.0, min = 0.0, description = "Font pixels the lines stop at; zero has no limit", group = "text" }"#.into()),
            (k::MAX_LINES, r#"{ type = "int", default = 0, min = 0, description = "The most lines a block keeps; zero keeps every line", group = "text" }"#.into()),
            (k::LINE_BREAK, format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Where a wrapped line may break: between words with a too-long word cut anywhere, between words only, or anywhere", group = "text" }}"#, w::WORD_OR_GLYPH, options(w::LINE_BREAKS))),
            (k::TRUNCATE, r#"{ type = "bool", default = false, description = "End a block cut short with an ellipsis; needs `max_width`, and cuts at `max_lines` or `max_height` when either is set, else at one line", group = "text" }"#.into()),
            (k::TRUNCATE_AT, format!(r#"{{ type = "enum", default = "{}", options = [{}], description = "Which part of a cut line the ellipsis stands in for", group = "text" }}"#, w::END, options(w::TRUNCATE_ATS))),
            (k::TAB_WIDTH, r#"{ type = "int", default = 8, min = 1, max = 64, description = "Spaces between tab stops", group = "text" }"#.into()),
        ]
    }
}

/// What a caller asks for, in the words `label` already uses.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub weight: u16,
    pub color: [f32; 4],
    pub align: Align,
    pub markup: bool,
    /// The width lines break at, in the same pixels as `font_size`.
    pub max_width: Option<f32>,
    /// The font it is shaped in: a face file, a `font` asset, or the key an
    /// inline definition is filed under. Empty takes the default font.
    pub font: String,
    /// The `text_style` asset that dresses it: line height and spacing, the
    /// lines drawn with its glyphs, an outline and a shadow. Empty draws it
    /// plain.
    pub text_style: String,
    /// Whether the node is content of the project the engine plays rather
    /// than its own (`project::in_content`), whose faces it draws in.
    pub in_content: bool,
    /// Drop a pixel fainter than this rather than blending it, so 3D text
    /// sorts by depth with the scene; zero blends every pixel.
    pub alpha_cutoff: f32,
    pub shaping: Shaping,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            size: 16.0,
            weight: 400,
            color: [1.0, 1.0, 1.0, 1.0],
            align: Align::Start,
            markup: false,
            max_width: None,
            font: String::new(),
            text_style: String::new(),
            in_content: false,
            alpha_cutoff: 0.0,
            shaping: Shaping::default(),
        }
    }
}

/// How a block sits in three dimensions; the 2D pass reads none of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpaceOptions {
    /// Turn to face the camera each frame.
    pub billboard: bool,
    pub layers: Layers3d,
}

impl Default for SpaceOptions {
    fn default() -> Self {
        Self {
            billboard: true,
            layers: Layers3d::default(),
        }
    }
}

/// Whether a 3D block casts, which lights reach it and which cameras draw
/// it: the keys every 3D renderable takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layers3d {
    pub cast_shadow: bool,
    /// As `light3d`'s mask.
    pub light_layers: u32,
    /// As `camera3d`'s mask.
    pub render_layers: u32,
}

impl Default for Layers3d {
    fn default() -> Self {
        Self {
            cast_shadow: true,
            light_layers: u32::MAX,
            render_layers: u32::MAX,
        }
    }
}

/// What a `text2d` or `text3d` node draws.
///
/// One component for both: the keys are the same, and which pass draws it is
/// `in_3d`, the way a node's dimension is settled everywhere else.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRenderable {
    /// The literal text; `text_key` wins over it when set.
    pub text: String,
    /// A key in the project's strings, re-read every frame so a language
    /// change shows without touching the scene.
    pub text_key: String,
    pub style: TextStyle,
    /// 2D: font pixels to one world unit. 3D: the same, as Godot's
    /// `pixel_size` inverted, so a size reads the same in both.
    pub pixels_per_unit: f32,
    pub in_3d: bool,
    /// What only the 3D pass reads.
    pub in_space: SpaceOptions,
    /// The `material` asset every layer draws with; empty takes an inherited
    /// one, else the shaper's own.
    pub material: String,
    /// The node flags of a `text3d`.
    pub overlay_3d: crate::overlay::Overlay3d,
    /// Bumped by every write, so the backend rebuilds only what moved.
    pub version: u64,
}

impl Default for TextRenderable {
    fn default() -> Self {
        Self {
            text: String::new(),
            text_key: String::new(),
            style: TextStyle::default(),
            pixels_per_unit: 100.0,
            in_3d: false,
            in_space: SpaceOptions::default(),
            material: String::new(),
            overlay_3d: crate::overlay::Overlay3d::default(),
            version: 0,
        }
    }
}

impl TextRenderable {
    /// What this draws right now: the localized string when a key names one,
    /// and the literal otherwise.
    pub fn resolved(&self, eng: &Engine) -> String {
        if self.text_key.is_empty() {
            return self.text.clone();
        }
        balaur_core::strings::tr(eng, &self.text_key, &[])
    }
}

/// One block of text a script asked for this frame and nothing keeps. Like a
/// debug line, none of it is recorded: a replay re-runs the script that drew.
#[derive(Clone, Debug, PartialEq)]
pub struct TextDraw {
    pub text: String,
    /// World position; `z` is ignored by the 2D pass.
    pub at: [f32; 3],
    pub style: TextStyle,
    /// Font pixels to one world unit.
    pub pixels_per_unit: f32,
    /// Whether the 3D pass draws it, facing the camera.
    pub in_3d: bool,
    /// Its place among the 2D nodes; over everything when `None`.
    pub z_index: Option<i32>,
}

/// What scripts drew this frame; the backend drains it as it draws.
#[derive(Default)]
pub struct TextDrawBuffer {
    pub items: Vec<TextDraw>,
}

/// The reference an asset option names: its text, or the key its table is
/// filed under as a component's inline definition is.
fn asset_option(
    eng: &Engine,
    type_name: &str,
    value: &balaur_script::Value,
) -> anyhow::Result<Option<String>> {
    Ok(match value {
        balaur_script::Value::Str(reference) => Some(reference.clone()),
        balaur_script::Value::Map(_) => Some(
            balaur_core::assets::define_inline(
                eng,
                type_name,
                balaur_core::node_api::to_toml(value)?,
            )?
            .to_string(),
        ),
        _ => None,
    })
}

/// A style from a script's options table, over the defaults.
pub(crate) fn style_of(
    eng: &Engine,
    opts: Option<balaur_script::Value>,
) -> anyhow::Result<TextStyle> {
    use balaur_script::Value;
    let mut style = TextStyle::default();
    let Some(Value::Map(entries)) = opts else {
        return Ok(style);
    };
    let number = |v: &Value| match v {
        Value::Num(n) => Some(*n as f32),
        Value::Int(n) => Some(*n as f32),
        _ => None,
    };
    for (key, value) in &entries {
        match key.as_str() {
            k::FONT_SIZE => style.size = number(value).unwrap_or(style.size),
            k::FONT_WEIGHT => {
                style.weight = number(value).map_or(style.weight, |weight| weight as u16);
            }
            k::MARKUP => style.markup = matches!(value, Value::Bool(true)),
            k::MAX_WIDTH => style.max_width = number(value),
            k::FONT => {
                if let Some(reference) =
                    asset_option(eng, balaur_core::assets::FONT_ASSET_TYPE, value)?
                {
                    style.font = reference;
                }
            }
            k::TEXT_STYLE => {
                if let Some(reference) =
                    asset_option(eng, balaur_core::assets::TEXT_STYLE_ASSET_TYPE, value)?
                {
                    style.text_style = reference;
                }
            }
            k::ALPHA_CUTOFF => {
                style.alpha_cutoff = number(value).unwrap_or(0.0).clamp(0.0, 1.0);
            }
            k::COLOR => style.color = crate::draw_2d::color_of(value)?,
            k::TEXT_ALIGN => {
                style.align = match value {
                    Value::Str(word) => Align::of(word),
                    _ => Align::Start,
                }
            }
            _ => {}
        }
    }
    style.shaping = Shaping::from_params(&balaur_core::node_api::to_toml(&Value::Map(entries))?);
    Ok(style)
}

#[cfg(feature = "window")]
pub(crate) use backend::{
    atlas_texture, bucket_ratio, laid_box, layers, mask_2d, mask_3d, mesh_2d, mesh_3d, request_of,
    shape, shape_at,
};

#[cfg(feature = "window")]
mod backend {
    use std::cell::RefCell;
    use std::rc::Rc;

    use balaur_core::Engine;
    use balaur_text::{Align as ShaperAlign, Request, Shaped};
    use glamx::Vec2;
    use kiss3d::context::Context;
    use kiss3d::resource::{GpuMesh2d, GpuMesh3d, Texture, TextureManager};
    use kiss3d::wgpu;

    /// The field of `shaders/text_mask.wesl`'s `Params` the cutoff fills.
    const CUTOFF_PARAM: &str = "cutoff";

    /// The 2D mask material, as kiss3d takes one on a node.
    type Shared2d = Rc<RefCell<Box<dyn kiss3d::resource::Material2d + 'static>>>;

    thread_local! {
        /// One mask material per cutoff: the value is baked into its uniform.
        static MASKS: RefCell<std::collections::HashMap<u32, Shared2d>> =
            RefCell::new(std::collections::HashMap::new());
    }

    /// Draw a 3D text node as a mask at `cutoff`, or blended at zero: kiss3d's
    /// own material reads the mode.
    pub(crate) fn mask_3d(node: &mut kiss3d::scene::SceneNode3d, cutoff: f32) {
        node.set_alpha_mode(if cutoff > 0.0 {
            kiss3d::scene::AlphaMode::Mask(cutoff)
        } else {
            kiss3d::scene::AlphaMode::Blend
        });
    }

    /// The 2D counterpart. kiss3d's 2D material has no mask, so a cutoff
    /// draws through `shaders/text_mask.wesl` instead.
    pub(crate) fn mask_2d(node: &mut kiss3d::scene::SceneNode2d, cutoff: f32) {
        if cutoff <= 0.0 {
            return;
        }
        if let Some(material) = mask_material(cutoff) {
            node.set_material(material);
        }
    }

    fn mask_material(cutoff: f32) -> Option<Shared2d> {
        MASKS.with(|masks| {
            if let Some(found) = masks.borrow().get(&cutoff.to_bits()) {
                return Some(Rc::clone(found));
            }
            let material = crate::material::Material3d {
                shader: "text_mask.wesl".into(),
                features: Vec::new(),
                params: vec![(CUTOFF_PARAM.into(), crate::material::Param::Float(cutoff))],
                surface: crate::material::Surface::default(),
                builtin: None,
            };
            let compiled = crate::material::compile(&material, crate::shaders::TEXT_MASK)
                .inspect_err(|why| tracing::error!("the text mask shader: {why:#}"))
                .ok()?;
            let shared: Shared2d = Rc::new(RefCell::new(Box::new(
                crate::shader_material::ShaderMaterial::new(&compiled, None, false),
            )));
            masks
                .borrow_mut()
                .insert(cutoff.to_bits(), Rc::clone(&shared));
            Some(shared)
        })
    }

    /// The atlas's name carries its side: a texture manager hands back what
    /// it already holds under a name, so a grown atlas needs a new one or the
    /// write overruns the texture made for the smaller side.
    const ATLAS: &str = "balaur text atlas";

    thread_local! {
        /// The atlas revision the texture was last written from.
        static UPLOADED: std::cell::Cell<u64> = const { std::cell::Cell::new(u64::MAX) };
        /// The side it was made at: the atlas doubles as it fills, and a
        /// write of the bigger image into the smaller texture is an error.
        static MADE_AT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    /// The atlas as a texture, uploaded when the shaper has drawn into it
    /// since the last call.
    pub(crate) fn atlas_texture(eng: &Engine) -> Option<std::sync::Arc<Texture>> {
        let state = balaur_text::state(eng)?;
        let state = state.borrow();
        let atlas = state.atlas();
        let side = atlas.side() as u32;
        let grown = MADE_AT.with(std::cell::Cell::get) != side;
        let name = format!("{ATLAS} {side}");
        let texture = TextureManager::get_global_manager(|tm| {
            let held = if grown { None } else { tm.get(&name) };
            held.unwrap_or_else(|| {
                let blank = image::DynamicImage::new_rgba8(side, side);
                UPLOADED.with(|at| at.set(u64::MAX));
                MADE_AT.with(|at| at.set(side));
                tm.add_image(blank, &name)
            })
        });
        if UPLOADED.with(std::cell::Cell::get) != atlas.revision() {
            UPLOADED.with(|at| at.set(atlas.revision()));
            Context::get().write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                atlas.rgba(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(side * 4),
                    rows_per_image: Some(side),
                },
                wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: 1,
                },
            );
        }
        Some(texture)
    }

    /// Shape3d `text`, or report that the fonts are not installed yet.
    ///
    /// The engine holds one shaper, made from the project's faces by whatever
    /// asks first.
    pub(crate) fn shape(eng: &Engine, text: &str, style: &super::TextStyle) -> std::rc::Rc<Shaped> {
        shape_at(eng, text, style, balaur_text::bucket(style.size))
    }

    /// The same, rasterised at `raster` pixels rather than the style's size:
    /// what a block far from the camera or magnified by one asks for.
    pub(crate) fn shape_at(
        eng: &Engine,
        text: &str,
        style: &super::TextStyle,
        raster: f32,
    ) -> std::rc::Rc<Shaped> {
        let state = balaur_text::shaper(eng);
        // Read the first time it is asked for, rather than drawn in the
        // default for a frame: a bitmap face's page goes into the atlas.
        state
            .borrow_mut()
            .load_font(eng, project_of(style), &style.font);
        let mut request = request_of(eng, text, style);
        // The width and the height the lines stop at scale with the raster,
        // so the block wraps where it measures at any distance.
        let ratio = raster.max(1.0) / request.size;
        request.width = request.width.map(|width| width * ratio);
        request.options.max_height = request.options.max_height.map(|height| height * ratio);
        request.size = raster.max(1.0);
        state.borrow_mut().shape(&request)
    }

    /// The shaper's request for a style, dressed by its `text_style`.
    ///
    /// The size is the bucket above what was asked for, so a camera zooming
    /// through it re-shapes a few times rather than every frame; the caller
    /// scales the block back down.
    pub(crate) fn request_of(eng: &Engine, text: &str, style: &super::TextStyle) -> Request {
        let look = balaur_text::text_style::of(eng, &style.text_style);
        Request {
            text: text.to_string(),
            size: balaur_text::bucket(style.size),
            weight: style.weight,
            slant: balaur_text::Slant::of(&style.shaping.font_style),
            width: style.max_width,
            truncate: style.shaping.truncate,
            align: match style.align {
                super::Align::Start => ShaperAlign::Start,
                super::Align::Center => ShaperAlign::Center,
                super::Align::End => ShaperAlign::End,
                super::Align::Left => ShaperAlign::Left,
                super::Align::Right => ShaperAlign::Right,
                super::Align::Justify => ShaperAlign::Justify,
            },
            markup: style.markup,
            font: style.font.clone(),
            project: project_of(style),
            line_height: look.line_height,
            letter_spacing: look.letter_spacing,
            options: balaur_text::Options {
                decoration: look.decoration,
                ..options_of(&style.shaping)
            },
        }
    }

    /// Whose faces a style's font is looked up in.
    fn project_of(style: &super::TextStyle) -> balaur_text::FontProject {
        balaur_text::FontProject::content_if(style.in_content)
    }

    /// A block's shaping settings as the shaper takes them.
    fn options_of(shaping: &super::Shaping) -> balaur_text::Options {
        let positive = |v: f32| (v > 0.0).then_some(v);
        balaur_text::Options {
            decoration: balaur_text::Decoration::default(),
            line_break: balaur_text::LineBreak::of(&shaping.line_break),
            truncate_at: balaur_text::TruncateAt::of(&shaping.truncate_at),
            max_lines: shaping.max_lines,
            max_height: positive(shaping.max_height),
            tab_width: shaping.tab_width,
        }
    }

    /// How far the shaped block has to be scaled to land at the asked size:
    /// it was rasterised at the bucket above it.
    pub(crate) fn bucket_ratio(style: &super::TextStyle) -> f32 {
        style.size.max(1.0) / balaur_text::bucket(style.size)
    }

    /// Where a shaped block's top-left corner sits so `text_align` lands the block
    /// around the anchor, and the y flip the world needs: the shaper places
    /// glyphs down the screen, the world counts up.
    fn origin(shaped: &Shaped, align: super::Align) -> Vec2 {
        let x = match align {
            super::Align::Start | super::Align::Left | super::Align::Justify => 0.0,
            super::Align::Center => -shaped.size.x / 2.0,
            super::Align::End | super::Align::Right => -shaped.size.x,
        };
        Vec2::new(x, shaped.size.y / 2.0)
    }

    /// The box a shaped block is laid out in, in the node's own space at
    /// `scale`, as a centre and half extents: what a click on the text lands
    /// in, its line boxes rather than its ink.
    pub(crate) fn laid_box(shaped: &Shaped, scale: f32, align: super::Align) -> (Vec2, Vec2) {
        let at = origin(shaped, align);
        let centre = Vec2::new(at.x + shaped.size.x / 2.0, at.y - shaped.size.y / 2.0) * scale;
        let half = Vec2::new(shaped.size.x, shaped.size.y) / 2.0 * scale;
        (centre, half)
    }

    /// Positions, triangles and atlas coordinates: what both meshes are built
    /// from, and the 3D one lifts into its plane.
    type Geometry = (Vec<Vec2>, Vec<[u32; 3]>, Vec<Vec2>);

    /// The copies of a block that draw together, back to front: the shadow,
    /// the outline's ring, then the text. Each is one mesh under one colour,
    /// because a mesh carries no per-vertex tint.
    /// One drawn copy of a block: where its quads are offset to, the colour
    /// it draws in, and which of the block's quads it covers.
    pub(crate) type Layer = (Vec<[f32; 2]>, [f32; 4], Vec<usize>);

    pub(crate) fn layers(
        shaped: &Shaped,
        style: &super::TextStyle,
        look: &balaur_text::text_style::TextStyle,
    ) -> Vec<Layer> {
        let all: Vec<usize> = (0..shaped.quads.len() + shaped.lines.len()).collect();
        let mut out: Vec<Layer> = Vec::new();
        // The shadow, then the outline's ring: the copies a widget's label
        // draws too, a layer a colour.
        let mut held = None;
        for (shift, colour) in look.effects.copies() {
            let Some(colour) = colour else {
                continue;
            };
            match out.last_mut() {
                Some((shifts, _, _)) if held == Some(colour) => shifts.push([shift.x, shift.y]),
                _ => {
                    held = Some(colour);
                    let channels = colour.to_srgba_unmultiplied().map(|v| f32::from(v) / 255.0);
                    out.push((vec![[shift.x, shift.y]], channels, all.clone()));
                }
            }
        }
        // The text itself, one layer per colour the markup asked for: a mesh
        // carries one colour, so a coloured word is its own draw.
        for (color, picks) in colour_groups(shaped, style.color) {
            out.push((vec![[0.0, 0.0]], color, picks));
        }
        out
    }

    /// The quads grouped by the colour they draw in: the markup's where it set
    /// one, the block's otherwise.
    fn colour_groups(shaped: &Shaped, base: [f32; 4]) -> Vec<([f32; 4], Vec<usize>)> {
        let mut groups: Vec<([f32; 4], Vec<usize>)> = Vec::new();
        for (index, quad) in shaped.pieces().enumerate() {
            let color = match quad.color {
                // A colour bitmap carries its own; tinting would wash it out.
                Some(_) | None if quad.colored => [1.0, 1.0, 1.0, base[3]],
                Some(marked) => {
                    let c = marked.to_normalized_gamma_f32();
                    [c[0], c[1], c[2], c[3] * base[3]]
                }
                None => base,
            };
            // By bits, not by nearness: these are the same colour value copied
            // from the same place, and a group is one draw either way.
            let same = |known: &[f32; 4]| known.map(f32::to_bits) == color.map(f32::to_bits);
            match groups.iter_mut().find(|(known, _)| same(known)) {
                Some((_, picks)) => picks.push(index),
                None => groups.push((color, vec![index])),
            }
        }
        groups
    }

    /// The quads of a shaped block as one mesh, once per offset in `shifts`,
    /// in units of `scale` per pixel.
    ///
    /// `None` when the block has no inked glyph — a blank line, or text whose
    /// every character is a space.
    fn geometry(
        shaped: &Shaped,
        scale: f32,
        align: super::Align,
        shifts: &[[f32; 2]],
        picks: &[usize],
    ) -> Option<Geometry> {
        if picks.is_empty() || shifts.is_empty() {
            return None;
        }
        let at = origin(shaped, align);
        // Glyphs then decoration lines, the order `layers` indexes them in.
        let pieces: Vec<balaur_text::Piece> = shaped.pieces().collect();
        let room = picks.len() * 4 * shifts.len();
        let mut coords = Vec::with_capacity(room);
        let mut uvs = Vec::with_capacity(room);
        let mut faces = Vec::with_capacity(picks.len() * 2 * shifts.len());
        for shift in shifts {
            for quad in picks.iter().filter_map(|i| pieces.get(*i)) {
                let base = u32::try_from(coords.len()).ok()?;
                let x0 = (quad.rect.min.x + at.x + shift[0]) * scale;
                let x1 = (quad.rect.max.x + at.x + shift[0]) * scale;
                // Down the block is down the screen and *down* in the world
                // too, so the block's top is the largest y.
                let y0 = (at.y - quad.rect.min.y - shift[1]) * scale;
                let y1 = (at.y - quad.rect.max.y - shift[1]) * scale;
                coords.extend_from_slice(&[
                    Vec2::new(x0, y1),
                    Vec2::new(x1, y1),
                    Vec2::new(x1, y0),
                    Vec2::new(x0, y0),
                ]);
                uvs.extend_from_slice(&[
                    Vec2::new(quad.uv.min.x, quad.uv.max.y),
                    Vec2::new(quad.uv.max.x, quad.uv.max.y),
                    Vec2::new(quad.uv.max.x, quad.uv.min.y),
                    Vec2::new(quad.uv.min.x, quad.uv.min.y),
                ]);
                faces.push([base, base + 1, base + 2]);
                faces.push([base, base + 2, base + 3]);
            }
        }
        Some((coords, faces, uvs))
    }

    /// A shaped block as a 2D mesh, one world unit per `1.0 / scale` pixels.
    pub(crate) fn mesh_2d(
        shaped: &Shaped,
        scale: f32,
        align: super::Align,
        shifts: &[[f32; 2]],
        picks: &[usize],
    ) -> Option<Rc<RefCell<GpuMesh2d>>> {
        let (coords, faces, uvs) = geometry(shaped, scale, align, shifts, picks)?;
        Some(Rc::new(RefCell::new(GpuMesh2d::new(
            coords,
            faces,
            Some(uvs),
            false,
        ))))
    }

    /// The same block as a 3D mesh in the node's xy plane, facing +z.
    ///
    /// `depth` lifts the whole block along its own +z, which is towards the
    /// camera once a billboard has turned: the layers are coplanar otherwise
    /// and the depth test picks between them at random.
    pub(crate) fn mesh_3d(
        shaped: &Shaped,
        scale: f32,
        align: super::Align,
        shifts: &[[f32; 2]],
        picks: &[usize],
        depth: f32,
    ) -> Option<Rc<RefCell<GpuMesh3d>>> {
        let (coords, faces, uvs) = geometry(shaped, scale, align, shifts, picks)?;
        let coords = coords
            .iter()
            .map(|p| glamx::Vec3::new(p.x, p.y, depth))
            .collect();
        let normals = vec![glamx::Vec3::new(0.0, 0.0, 1.0); uvs.len()];
        Some(Rc::new(RefCell::new(GpuMesh3d::new(
            coords,
            faces,
            Some(normals),
            Some(uvs),
            false,
        ))))
    }
}

/// The size `text` shapes to, in the same pixels as `style.size`.
///
/// Deterministic: measured against the project's fonts and the bundled ones
/// only, never the machine's, so every platform answers the same. A bitmap
/// face is measured from its own descriptor, which is as fixed.
pub fn measure(eng: &Engine, text: &str, style: &TextStyle) -> anyhow::Result<[f32; 2]> {
    #[cfg(feature = "window")]
    {
        let project = balaur_text::FontProject::content_if(style.in_content);
        balaur_text::load_font(eng, project, &style.font);
        let request = request_of(eng, text, style);
        let size = balaur_text::shaper(eng).borrow_mut().measure(&request);
        Ok([size.x, size.y])
    }
    #[cfg(not(feature = "window"))]
    {
        let _ = (eng, text, style);
        Err(anyhow::anyhow!(
            "no text shaper: this build has no ui plugin, so nothing can measure text"
        ))
    }
}

/// How far in front of the layer behind it each layer sits. Small enough to
/// read as one block, large enough for the depth buffer to tell them apart.
#[cfg(feature = "window")]
pub(crate) fn depth_of(layer: usize) -> f32 {
    layer as f32 * 0.001
}

/// Everything the backend keeps for text between frames: the nodes the
/// immediate calls made, and one slot per node carrying a `text2d` or
/// `text3d`.
#[cfg(feature = "window")]
#[derive(Default)]
pub(crate) struct Frame {
    transients: Transients,
    slots: std::collections::HashMap<balaur_core::hecs::Entity, crate::text_component::TextSlot>,
}

#[cfg(feature = "window")]
impl Frame {
    /// Mirror every `text2d` and `text3d` node, before the 2D order places them.
    pub(crate) fn sync(
        &mut self,
        app: &balaur_core::App,
        scenes: (
            &mut kiss3d::scene::SceneNode2d,
            &mut kiss3d::scene::SceneNode3d,
        ),
        materials: crate::text_component::TextMaterials<'_>,
        viewport_height: f32,
    ) {
        crate::text_component::sync_text(app, scenes, materials, &mut self.slots, viewport_height);
    }

    /// Whether a `text2d` block is drawn for `entity`, and so has a place in
    /// the 2D order.
    pub(crate) fn draws_2d(&self, entity: balaur_core::hecs::Entity) -> bool {
        self.slots
            .get(&entity)
            .is_some_and(|slot| slot.group_2d().is_some())
    }

    /// The node a `text2d` block's layers hang under, which the 2D order places.
    pub(crate) fn group_2d(
        &self,
        entity: balaur_core::hecs::Entity,
    ) -> Option<kiss3d::scene::SceneNode2d> {
        self.slots.get(&entity)?.group_2d().cloned()
    }

    /// Draw what scripts asked for this frame. After the order pass: text
    /// with no `z_index` goes over everything, the light map included.
    pub(crate) fn flush(
        &mut self,
        app: &balaur_core::App,
        scene_2d: &mut kiss3d::scene::SceneNode2d,
        scene_3d: &mut kiss3d::scene::SceneNode3d,
        placed: &crate::draw_2d::Layers2d,
    ) {
        flush(app, scene_2d, scene_3d, placed, &mut self.transients);
    }

    /// Take last frame's script-drawn text out, before the 2D order runs: a
    /// node left at the end of the scene would be swapped into the ordered
    /// nodes the next time one of them is detached.
    pub(crate) fn clear_transients(&mut self) {
        for mut node in self.transients.two_d.drain(..) {
            node.detach();
        }
        for mut node in self.transients.three_d.drain(..) {
            node.detach();
        }
    }
}

/// The nodes one frame's text made, dropped when the next frame draws.
#[cfg(feature = "window")]
#[derive(Default)]
pub(crate) struct Transients {
    two_d: Vec<kiss3d::scene::SceneNode2d>,
    three_d: Vec<kiss3d::scene::SceneNode3d>,
}

/// Draw everything scripts asked for this frame, as nodes that live one frame.
#[cfg(feature = "window")]
fn flush(
    app: &balaur_core::App,
    scene_2d: &mut kiss3d::scene::SceneNode2d,
    scene_3d: &mut kiss3d::scene::SceneNode3d,
    placed: &crate::draw_2d::Layers2d,
    transients: &mut Transients,
) {
    use kiss3d::color::Color;

    let Some(buffer) = app.engine.try_resource::<TextDrawBuffer>() else {
        return;
    };
    let items = std::mem::take(&mut buffer.borrow_mut().items);
    if items.is_empty() {
        return;
    }
    // Shape3d every block first: each may grow the atlas, and the texture is
    // uploaded once for the lot rather than once per block.
    let shaped: Vec<_> = items
        .iter()
        .map(|item| shape(&app.engine, &item.text, &item.style))
        .collect();
    let Some(texture) = atlas_texture(&app.engine) else {
        return;
    };
    for (item, block) in items.iter().zip(shaped) {
        let scale = bucket_ratio(&item.style) / item.pixels_per_unit;
        // Shadow, outline and text: each is a node, because a mesh carries
        // one colour and they are drawn in that order.
        for (layer, (shifts, [r, g, b, a], picks)) in layers(
            &block,
            &item.style,
            &balaur_text::text_style::of(&app.engine, &item.style.text_style),
        )
        .into_iter()
        .enumerate()
        {
            if item.in_3d {
                let Some(mesh) = mesh_3d(
                    &block,
                    scale,
                    item.style.align,
                    &shifts,
                    &picks,
                    depth_of(layer),
                ) else {
                    continue;
                };
                let mut node = scene_3d.add_mesh(mesh, glamx::Vec3::ONE);
                node.set_texture(texture.clone());
                node.set_position(glamx::Vec3::new(item.at[0], item.at[1], item.at[2]));
                node.set_color(Color::new(r, g, b, a));
                mask_3d(&mut node, item.style.alpha_cutoff);
                transients.three_d.push(node);
            } else {
                let Some(mesh) = mesh_2d(&block, scale, item.style.align, &shifts, &picks) else {
                    continue;
                };
                let (mut parent, _) = placed.parent_of(item.z_index, scene_2d);
                let mut node = parent.add_mesh(mesh, glamx::Vec2::ONE);
                node.set_texture(texture.clone());
                node.set_position(glamx::Vec2::new(item.at[0], item.at[1]));
                node.set_color(Color::new(r, g, b, a));
                mask_2d(&mut node, item.style.alpha_cutoff);
                transients.two_d.push(node);
            }
        }
    }
}

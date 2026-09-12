//! Text drawn in the world, from the same shaper the widgets use.
//!
//! `balaur_ui` shapes a run into glyph quads over its atlas; this turns those
//! quads into a mesh and the atlas into a texture kiss3d can sample, so a name
//! over a character and a label in a panel come from one bitmap and one set of
//! font rules.

use balaur_core::Engine;

/// Where a block sits relative to the point it was drawn at.
///
/// Its own enum rather than the shaper's: the buffer and its bindings are in
/// every build, and the shaper comes with the windowed backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

/// An outline around the glyphs and a shadow behind them.
///
/// Both are the same quads drawn again, offset and tinted, under the text:
/// an outline is eight copies around the origin, a shadow one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decoration {
    /// Font pixels the outline reaches; zero draws none.
    pub outline_size: f32,
    pub outline_color: [f32; 4],
    /// Font pixels the shadow is moved by; zero draws none.
    pub shadow_offset: [f32; 2],
    pub shadow_color: [f32; 4],
}

impl Default for Decoration {
    fn default() -> Self {
        Self {
            outline_size: 0.0,
            outline_color: [0.0, 0.0, 0.0, 1.0],
            shadow_offset: [0.0, 0.0],
            shadow_color: [0.0, 0.0, 0.0, 0.5],
        }
    }
}

/// What a caller asks for, in the words `label` already uses.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub weight: u16,
    pub italic: bool,
    pub color: [f32; 4],
    pub align: Align,
    pub markup: bool,
    /// The width lines break at, in the same pixels as `size`.
    pub max_width: Option<f32>,
    pub decoration: Decoration,
    /// A project-relative `.fnt` naming a bitmap face; empty shapes with the
    /// project's vector fonts.
    pub font: String,
    /// Which named chain to shape with — `heading`, `ui`, `mono` or `icons`.
    pub family: String,
    /// Baseline to baseline, as a multiple of the size; zero takes the default.
    pub line_height: f32,
    /// Extra space between glyphs, in the same pixels as `size`.
    pub letter_spacing: f32,
    /// 3D only: discard a pixel this transparent rather than blending it, so
    /// text can be depth-sorted with the scene instead of over it.
    pub alpha_cut: f32,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            size: 16.0,
            weight: 400,
            italic: false,
            color: [1.0, 1.0, 1.0, 1.0],
            align: Align::Start,
            markup: false,
            max_width: None,
            decoration: Decoration::default(),
            font: String::new(),
            family: String::new(),
            line_height: 0.0,
            letter_spacing: 0.0,
            alpha_cut: 0.0,
        }
    }
}

/// How a block sits in three dimensions; the 2D pass reads none of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpaceOptions {
    /// Turn to face the camera each frame.
    pub billboard: bool,
    /// Draw the back of the quad as well as the front.
    pub double_sided: bool,
    /// Let the scene's depth hide it.
    pub depth_test: bool,
}

impl Default for SpaceOptions {
    fn default() -> Self {
        Self {
            billboard: true,
            double_sided: true,
            depth_test: true,
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
}

/// What scripts drew this frame; the backend drains it as it draws.
#[derive(Default)]
pub struct TextDrawBuffer {
    pub items: Vec<TextDraw>,
}

/// A style from a script's options table, over the defaults.
pub(crate) fn style_of(opts: Option<balaur_script::Value>) -> anyhow::Result<TextStyle> {
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
            "size" => style.size = number(value).unwrap_or(style.size),
            "weight" => style.weight = number(value).unwrap_or(400.0) as u16,
            "italic" => style.italic = matches!(value, Value::Bool(true)),
            "markup" => style.markup = matches!(value, Value::Bool(true)),
            "max_width" => style.max_width = number(value),
            "font" => {
                if let Value::Str(path) = value {
                    style.font.clone_from(path);
                }
            }
            "family" => {
                if let Value::Str(chain) = value {
                    style.family.clone_from(chain);
                }
            }
            "line_height" => style.line_height = number(value).unwrap_or(0.0).max(0.0),
            "letter_spacing" => style.letter_spacing = number(value).unwrap_or(0.0),
            "alpha_cut" => style.alpha_cut = number(value).unwrap_or(0.0).clamp(0.0, 1.0),
            "outline_size" => {
                style.decoration.outline_size = number(value).unwrap_or(0.0).max(0.0);
            }
            "outline_color" => style.decoration.outline_color = crate::draw_2d::color_of(value)?,
            "shadow_color" => style.decoration.shadow_color = crate::draw_2d::color_of(value)?,
            "shadow_offset" => {
                if let Value::List(items) = value
                    && items.len() >= 2
                {
                    style.decoration.shadow_offset = [
                        number(&items[0]).unwrap_or(0.0),
                        number(&items[1]).unwrap_or(0.0),
                    ];
                }
            }
            "color" => style.color = crate::draw_2d::color_of(value)?,
            "align" => {
                style.align = match value {
                    Value::Str(word) if word == "center" => Align::Center,
                    Value::Str(word) if word == "end" => Align::End,
                    _ => Align::Start,
                }
            }
            _ => {}
        }
    }
    Ok(style)
}

#[cfg(feature = "kiss3d")]
pub(crate) use backend::{
    atlas_texture, bucket_ratio, layers, mesh_2d, mesh_3d, request_of, shape, shape_at,
};

#[cfg(feature = "kiss3d")]
mod backend {
    use std::cell::RefCell;
    use std::rc::Rc;

    use anyhow::{Result, anyhow};
    use balaur_core::Engine;
    use balaur_text::{Align as ShaperAlign, Request, Shaped};
    use glamx::Vec2;
    use kiss3d::context::Context;
    use kiss3d::resource::{GpuMesh2d, GpuMesh3d, Texture, TextureManager};
    use kiss3d::wgpu;

    /// One name for the atlas: it is written in place when it grows, so a
    /// second texture never has to be made for it.
    const ATLAS: &str = "balaur text atlas";

    thread_local! {
        /// The atlas revision the texture was last written from.
        static UPLOADED: std::cell::Cell<u64> = const { std::cell::Cell::new(u64::MAX) };
    }

    /// The atlas as a texture, uploaded when the shaper has drawn into it
    /// since the last call.
    pub(crate) fn atlas_texture(eng: &Engine) -> Option<std::sync::Arc<Texture>> {
        let state = balaur_text::state(eng)?;
        let state = state.borrow();
        let atlas = state.atlas();
        let side = atlas.side() as u32;
        let texture = TextureManager::get_global_manager(|tm| {
            tm.get(ATLAS).unwrap_or_else(|| {
                let blank = image::DynamicImage::new_rgba8(side, side);
                UPLOADED.with(|at| at.set(u64::MAX));
                tm.add_image(blank, ATLAS)
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

    /// Shape `text`, or report that the fonts are not installed yet.
    ///
    /// The engine holds one shaper, made when the theme's faces load; a run
    /// without the UI plugin has none, and nothing can draw text.
    pub(crate) fn shape(
        eng: &Engine,
        text: &str,
        style: &super::TextStyle,
    ) -> Result<std::rc::Rc<Shaped>> {
        shape_at(eng, text, style, balaur_text::bucket(style.size))
    }

    /// The same, rasterised at `raster` pixels rather than the style's size:
    /// what a block far from the camera or magnified by one asks for.
    pub(crate) fn shape_at(
        eng: &Engine,
        text: &str,
        style: &super::TextStyle,
        raster: f32,
    ) -> Result<std::rc::Rc<Shaped>> {
        let state = balaur_text::state(eng)
            .ok_or_else(|| anyhow!("no text shaper: the ui plugin installs it with the fonts"))?;
        // A bitmap face is loaded the first time it is asked for: the page
        // goes into the atlas beside the rasterised glyphs.
        if !style.font.is_empty() {
            load_bitmap_font(eng, &style.font)?;
        }
        let mut request = request_of(text, style);
        request.size = raster.max(1.0);
        let shaped = state.borrow_mut().shape(&request);
        Ok(shaped)
    }

    /// The shaper's request for a style.
    ///
    /// The size is the bucket above what was asked for, so a camera zooming
    /// through it re-shapes a few times rather than every frame; the caller
    /// scales the block back down.
    pub(crate) fn request_of(text: &str, style: &super::TextStyle) -> Request {
        Request {
            text: text.to_string(),
            size: balaur_text::bucket(style.size),
            weight: style.weight,
            italic: style.italic,
            width: style.max_width,
            align: match style.align {
                super::Align::Start => ShaperAlign::Start,
                super::Align::Center => ShaperAlign::Center,
                super::Align::End => ShaperAlign::End,
            },
            markup: style.markup,
            font: style.font.clone(),
            family: style.family.clone(),
            line_height: style.line_height,
            letter_spacing: style.letter_spacing,
        }
    }

    /// How far the shaped block has to be scaled to land at the asked size:
    /// it was rasterised at the bucket above it.
    pub(crate) fn bucket_ratio(style: &super::TextStyle) -> f32 {
        style.size.max(1.0) / balaur_text::bucket(style.size)
    }

    /// Read a `.fnt` and its page out of the project and hand them to the
    /// shaper, once per face. Later calls find it already there.
    fn load_bitmap_font(eng: &Engine, path: &str) -> Result<()> {
        let state = balaur_text::state(eng)
            .ok_or_else(|| anyhow!("no text shaper: the ui plugin installs it with the fonts"))?;
        if state.borrow().has_bitmap_font(path) {
            return Ok(());
        }
        let files = eng.resource::<balaur_core::project::ProjectFiles>();
        let descriptor = String::from_utf8(files.borrow().read(path)?)
            .map_err(|_| anyhow!("{path} is not a text .fnt descriptor"))?;
        // The page sits beside the descriptor, as the tool that wrote it left it.
        let page_name = balaur_text::bitmap::parse(&descriptor)?.page;
        let directory = path.rsplit_once('/').map_or("", |(head, _)| head);
        let page_path = if directory.is_empty() {
            page_name
        } else {
            format!("{directory}/{page_name}")
        };
        let page = files.borrow().read(&page_path)?;
        state.borrow_mut().add_bitmap_font(path, &descriptor, &page)
    }

    /// Where a shaped block's top-left corner sits so `align` lands the block
    /// around the anchor, and the y flip the world needs: the shaper places
    /// glyphs down the screen, the world counts up.
    fn origin(shaped: &Shaped, align: super::Align) -> Vec2 {
        let x = match align {
            super::Align::Start => 0.0,
            super::Align::Center => -shaped.size.x / 2.0,
            super::Align::End => -shaped.size.x,
        };
        Vec2::new(x, shaped.size.y / 2.0)
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

    pub(crate) fn layers(shaped: &Shaped, style: &super::TextStyle) -> Vec<Layer> {
        let all: Vec<usize> = (0..shaped.quads.len()).collect();
        let mut out = Vec::new();
        let decoration = &style.decoration;
        if decoration.shadow_offset != [0.0, 0.0] {
            out.push((
                vec![decoration.shadow_offset],
                decoration.shadow_color,
                all.clone(),
            ));
        }
        if decoration.outline_size > 0.0 {
            let r = decoration.outline_size;
            // The eight neighbours: a ring reads as an outline where four
            // leaves the diagonals thin.
            let ring = [
                (-1.0, -1.0),
                (0.0, -1.0),
                (1.0, -1.0),
                (-1.0, 0.0),
                (1.0, 0.0),
                (-1.0, 1.0),
                (0.0, 1.0),
                (1.0, 1.0),
            ];
            out.push((
                ring.iter().map(|(x, y)| [x * r, y * r]).collect(),
                decoration.outline_color,
                all.clone(),
            ));
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
        for (index, quad) in shaped.quads.iter().enumerate() {
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
        let room = picks.len() * 4 * shifts.len();
        let mut coords = Vec::with_capacity(room);
        let mut uvs = Vec::with_capacity(room);
        let mut faces = Vec::with_capacity(picks.len() * 2 * shifts.len());
        for shift in shifts {
            for quad in picks.iter().filter_map(|i| shaped.quads.get(*i)) {
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
/// face is measured from its own descriptor, which is as fixed. Without the
/// shaper — a run with no UI plugin — this reports rather than guessing.
pub fn measure(eng: &Engine, text: &str, style: &TextStyle) -> anyhow::Result<[f32; 2]> {
    #[cfg(feature = "kiss3d")]
    {
        let state = balaur_text::state(eng)
            .ok_or_else(|| anyhow::anyhow!("no text shaper: the ui plugin installs it"))?;
        if !style.font.is_empty() {
            let shaped = shape(eng, text, style)?;
            return Ok([shaped.size.x, shaped.size.y]);
        }
        let request = request_of(text, style);
        let size = state.borrow_mut().measure(&request);
        Ok([size.x, size.y])
    }
    #[cfg(not(feature = "kiss3d"))]
    {
        let _ = (eng, text, style);
        Err(anyhow::anyhow!(
            "no text shaper: this build has no ui plugin, so nothing can measure text"
        ))
    }
}

/// How far in front of the layer behind it each layer sits. Small enough to
/// read as one block, large enough for the depth buffer to tell them apart.
#[cfg(feature = "kiss3d")]
pub(crate) fn depth_of(layer: usize) -> f32 {
    layer as f32 * 0.001
}

/// Report a missing shaper once: it is a boot condition on the first frame
/// and a missing plugin forever after, and neither wants a line per call.
#[cfg(feature = "kiss3d")]
pub(crate) fn warn_once(err: &anyhow::Error) {
    if balaur_core::logbuf::first_time("text shaper", "") {
        tracing::warn!("{err:#}");
    }
}

/// Everything the backend keeps for text between frames: the nodes the
/// immediate calls made, and one slot per node carrying a `text2d` or
/// `text3d`.
#[cfg(feature = "kiss3d")]
#[derive(Default)]
pub(crate) struct Frame {
    transients: Transients,
    slots: std::collections::HashMap<balaur_core::hecs::Entity, crate::text_component::TextSlot>,
}

/// Draw this frame's text: the nodes that carry it, then the calls that asked
/// for it. Both come from the same atlas, uploaded once for the pair.
#[cfg(feature = "kiss3d")]
pub(crate) fn draw(
    app: &balaur_core::App,
    scene_2d: &mut kiss3d::scene::SceneNode2d,
    scene_3d: &mut kiss3d::scene::SceneNode3d,
    frame: &mut Frame,
    viewport_height: f32,
) {
    crate::text_component::sync_text(app, scene_2d, scene_3d, &mut frame.slots, viewport_height);
    flush(app, scene_2d, scene_3d, &mut frame.transients);
}

/// The nodes one frame's text made, dropped when the next frame draws.
#[cfg(feature = "kiss3d")]
#[derive(Default)]
pub(crate) struct Transients {
    two_d: Vec<kiss3d::scene::SceneNode2d>,
    three_d: Vec<kiss3d::scene::SceneNode3d>,
}

/// Draw everything scripts asked for this frame, as nodes that live one frame.
#[cfg(feature = "kiss3d")]
pub(crate) fn flush(
    app: &balaur_core::App,
    scene_2d: &mut kiss3d::scene::SceneNode2d,
    scene_3d: &mut kiss3d::scene::SceneNode3d,
    transients: &mut Transients,
) {
    use kiss3d::color::Color;

    for mut node in transients.two_d.drain(..) {
        node.detach();
    }
    for mut node in transients.three_d.drain(..) {
        node.detach();
    }
    let Some(buffer) = app.engine.try_resource::<TextDrawBuffer>() else {
        return;
    };
    let items = std::mem::take(&mut buffer.borrow_mut().items);
    if items.is_empty() {
        return;
    }
    // Shape every block first: each may grow the atlas, and the texture is
    // uploaded once for the lot rather than once per block.
    let mut shaped = Vec::with_capacity(items.len());
    for item in &items {
        match shape(&app.engine, &item.text, &item.style) {
            Ok(block) => shaped.push(Some(block)),
            // The fonts install on the first UI pass, which is later in this
            // frame: said once, since the frame after it draws.
            Err(err) => {
                warn_once(&err);
                shaped.push(None);
            }
        }
    }
    let Some(texture) = atlas_texture(&app.engine) else {
        return;
    };
    for (item, block) in items.iter().zip(shaped) {
        let Some(block) = block else { continue };
        let scale = bucket_ratio(&item.style) / item.pixels_per_unit;
        // Shadow, outline and text: each is a node, because a mesh carries
        // one colour and they are drawn in that order.
        for (layer, (shifts, [r, g, b, a], picks)) in
            layers(&block, &item.style).into_iter().enumerate()
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
                transients.three_d.push(node);
            } else {
                let Some(mesh) = mesh_2d(&block, scale, item.style.align, &shifts, &picks) else {
                    continue;
                };
                let mut node = scene_2d.add_mesh(mesh, glamx::Vec2::ONE);
                node.set_texture(texture.clone());
                node.set_position(glamx::Vec2::new(item.at[0], item.at[1]));
                node.set_color(Color::new(r, g, b, a));
                transients.two_d.push(node);
            }
        }
    }
}

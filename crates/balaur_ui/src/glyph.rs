//! A word as geometry: the outline of every glyph in a shaped run, filled.
//!
//! cosmic-text lays the string out with the same shaper the widget layer
//! uses, swash yields each glyph's outline, the curves flatten, and the
//! contours are triangulated together so a letter's counters stay holes.
//! What comes out is a `mesh` like any other: a collider fits it, a ray
//! picks it, and `docs/PLAN-objects.md` step 3 extrudes it.
//!
//! Nothing here touches a GPU, so a headless build shapes the same word into
//! the same triangles a windowed one draws.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use balaur_core::mesh::{MeshData, TextShape};
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, Weight, fontdb};
use i_triangle::float::triangulator::Triangulator;
use swash::scale::ScaleContext;
use swash::zeno::{Command, PathData};

/// The pixel size a run is shaped at before it is scaled into world units.
/// Big enough that a glyph's grid quantisation is far below the flattening
/// tolerance, small enough that the numbers stay exact in `f32`.
const SHAPING_PIXELS: f32 = 64.0;

/// How far a flattened curve may sit from the true one, as a fraction of the
/// em. Half a percent is where a letter's bowl stops looking like a polygon
/// at the sizes a title is set in.
const FLATNESS: f32 = 0.005;

/// The most segments one curve is cut into. A tolerance alone would let a
/// pathological outline ask for thousands.
const MAX_CURVE_STEPS: u32 = 24;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    text: String,
    font: String,
    /// The size in thousandths of a unit: a cache key cannot be a float.
    size: u32,
    weight: u16,
    italic: bool,
}

impl Key {
    fn of(shape: &TextShape) -> Self {
        Self {
            text: shape.text.clone(),
            font: shape.font.clone(),
            size: (shape.size * 1000.0) as u32,
            weight: shape.weight,
            italic: shape.italic,
        }
    }
}

/// The shaper, the outline scaler and what they have already built.
pub struct GlyphMesher {
    fonts: FontSystem,
    scale: ScaleContext,
    built: HashMap<Key, MeshData>,
}

impl GlyphMesher {
    /// A mesher over the project's faces, in chain order.
    pub(crate) fn new(faces: &[crate::theme::FontFace], locale: &str) -> Self {
        let mut db = fontdb::Database::new();
        for face in faces {
            let shared: Arc<Vec<u8>> = Arc::clone(&face.bytes);
            let data: Arc<dyn AsRef<[u8]> + Send + Sync> = shared;
            db.load_font_source(fontdb::Source::Binary(data));
        }
        Self {
            fonts: FontSystem::new_with_locale_and_db(locale.to_string(), db),
            scale: ScaleContext::new(),
            built: HashMap::new(),
        }
    }

    /// The mesh for one request, shaped once and kept.
    ///
    /// # Errors
    /// If the string is empty, or no face in the project draws any of it.
    pub fn mesh(&mut self, shape: &TextShape) -> Result<MeshData> {
        let key = Key::of(shape);
        if let Some(mesh) = self.built.get(&key) {
            return Ok(mesh.clone());
        }
        let mesh = self.build(shape)?;
        self.built.insert(key, mesh.clone());
        Ok(mesh)
    }

    /// Every contour of every glyph, in world units with y up.
    fn contours(&mut self, shape: &TextShape) -> Vec<Vec<[f32; 2]>> {
        let mut attrs = Attrs::new()
            .weight(Weight(shape.weight))
            .style(if shape.italic {
                Style::Italic
            } else {
                Style::Normal
            });
        if !shape.font.is_empty() {
            attrs = attrs.family(Family::Name(&shape.font));
        }
        let metrics = Metrics::new(SHAPING_PIXELS, SHAPING_PIXELS);
        // Shaping first, outlines after: the layout borrows the font set,
        // and scaling a glyph needs it back.
        let mut placed: Vec<(fontdb::ID, fontdb::Weight, u16, [f32; 2])> = Vec::new();
        {
            let mut buffer = Buffer::new(&mut self.fonts, metrics);
            let mut buffer = buffer.borrow_with(&mut self.fonts);
            buffer.set_size(None, None);
            buffer.set_text(&shape.text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(false);
            // The shaper measures down from the block's top and the outline
            // measures up from the baseline. The mesh is y up and sits on the
            // first line's baseline, the way a font places a word.
            let mut first = None;
            for run in buffer.layout_runs() {
                let base = *first.get_or_insert(run.line_y);
                for glyph in run.glyphs {
                    let origin = [glyph.x, base - run.line_y - glyph.y];
                    placed.push((glyph.font_id, glyph.font_weight, glyph.glyph_id, origin));
                }
            }
        }
        let unit = shape.size / SHAPING_PIXELS;
        let mut out = Vec::new();
        for (font_id, font_weight, glyph_id, origin) in placed {
            let Some(font) = self.fonts.get_font(font_id, font_weight) else {
                continue;
            };
            let mut scaler = self
                .scale
                .builder(font.as_swash())
                .size(SHAPING_PIXELS)
                .hint(false)
                .build();
            let Some(outline) = scaler.scale_outline(glyph_id) else {
                continue;
            };
            walk(outline.path(), origin, unit, &mut out);
        }
        out
    }

    /// Shape, flatten and fill. The contours go in together so a counter --
    /// the hole in an `o` -- is subtracted rather than filled over.
    fn build(&mut self, shape: &TextShape) -> Result<MeshData> {
        if shape.text.trim().is_empty() {
            return Err(anyhow!("a text mesh needs something to say"));
        }
        let contours = self.contours(shape);
        if contours.is_empty() {
            return Err(anyhow!(
                "no font in this project draws '{}', so it has no outline",
                shape.text
            ));
        }
        let mut triangulator: Triangulator<u32, i32> = Triangulator::default();
        let filled = triangulator.triangulate(&contours);
        if filled.indices.len() < 3 {
            return Err(anyhow!("'{}' filled to nothing", shape.text));
        }
        Ok(fill_mesh(&filled.points, &filled.indices))
    }
}

/// The filled outline as a mesh in the z = 0 plane, facing +z.
fn fill_mesh(points: &[[f32; 2]], indices: &[u32]) -> MeshData {
    let (mut min, mut max) = (points[0], points[0]);
    for p in points {
        min = [min[0].min(p[0]), min[1].min(p[1])];
        max = [max[0].max(p[0]), max[1].max(p[1])];
    }
    let span = [
        (max[0] - min[0]).max(f32::MIN_POSITIVE),
        (max[1] - min[1]).max(f32::MIN_POSITIVE),
    ];
    MeshData {
        positions: points.iter().map(|p| [p[0], p[1], 0.0]).collect(),
        indices: indices.as_chunks::<3>().0.to_vec(),
        normals: Some(vec![[0.0, 0.0, 1.0]; points.len()]),
        uvs: Some(
            points
                .iter()
                .map(|p| [(p[0] - min[0]) / span[0], (p[1] - min[1]) / span[1]])
                .collect(),
        ),
        source: None,
        text: None,
        path: None,
        skin: None,
    }
}

/// Walk one glyph's path, flattening its curves, and append its closed
/// contours placed at `origin` and scaled by `unit`.
fn walk(path: impl PathData, origin: [f32; 2], unit: f32, out: &mut Vec<Vec<[f32; 2]>>) {
    let place = |x: f32, y: f32| [(origin[0] + x) * unit, (origin[1] + y) * unit];
    let mut contour: Vec<[f32; 2]> = Vec::new();
    let mut here = [0.0f32, 0.0];
    let close = |contour: &mut Vec<[f32; 2]>, out: &mut Vec<Vec<[f32; 2]>>| {
        if contour.len() >= 3 {
            out.push(std::mem::take(contour));
        } else {
            contour.clear();
        }
    };
    for command in path.commands() {
        match command {
            Command::MoveTo(to) => {
                close(&mut contour, out);
                here = [to.x, to.y];
                contour.push(place(to.x, to.y));
            }
            Command::LineTo(to) => {
                here = [to.x, to.y];
                contour.push(place(to.x, to.y));
            }
            Command::QuadTo(control, to) => {
                let control = [control.x, control.y];
                let to = [to.x, to.y];
                for i in 1..=steps(&[here, control, to]) {
                    let t = i as f32 / steps(&[here, control, to]) as f32;
                    let p = quadratic(here, control, to, t);
                    contour.push(place(p[0], p[1]));
                }
                here = to;
            }
            Command::CurveTo(first, second, to) => {
                let first = [first.x, first.y];
                let second = [second.x, second.y];
                let to = [to.x, to.y];
                let n = steps(&[here, first, second, to]);
                for i in 1..=n {
                    let p = cubic(here, first, second, to, i as f32 / n as f32);
                    contour.push(place(p[0], p[1]));
                }
                here = to;
            }
            Command::Close => close(&mut contour, out),
        }
    }
    close(&mut contour, out);
}

/// How many straight pieces a curve is cut into: enough that the widest gap
/// between the chord and the curve stays under the tolerance.
fn steps(control: &[[f32; 2]]) -> u32 {
    let length: f32 = control
        .windows(2)
        .map(|pair| {
            let (dx, dy) = (pair[1][0] - pair[0][0], pair[1][1] - pair[0][1]);
            libm::hypotf(dx, dy)
        })
        .sum();
    let tolerance = FLATNESS * SHAPING_PIXELS;
    let wanted = (length / tolerance).sqrt().ceil() as u32;
    wanted.clamp(1, MAX_CURVE_STEPS)
}

fn quadratic(a: [f32; 2], b: [f32; 2], c: [f32; 2], t: f32) -> [f32; 2] {
    let u = 1.0 - t;
    let at = |i: usize| u * u * a[i] + 2.0 * u * t * b[i] + t * t * c[i];
    [at(0), at(1)]
}

fn cubic(a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2], t: f32) -> [f32; 2] {
    let u = 1.0 - t;
    let at = |i: usize| {
        u * u * u * a[i] + 3.0 * u * u * t * b[i] + 3.0 * u * t * t * c[i] + t * t * t * d[i]
    };
    [at(0), at(1)]
}

/// Fill in core's text seam with this crate's shaper.
///
/// The mesher is built on the first word asked for, not here: the faces come
/// from the project's `fonts/` directory, and no project is open when a
/// plugin declares itself.
pub(crate) fn install(reg: &mut balaur_plugin::Registry<'_>) {
    let mesher: std::cell::RefCell<Option<GlyphMesher>> = std::cell::RefCell::new(None);
    reg.insert_resource(balaur_core::mesh::TextGeometry(Box::new(
        move |eng, shape| {
            let mut slot = mesher.borrow_mut();
            let mesher = slot.get_or_insert_with(|| {
                let faces = crate::theme::font_faces(eng);
                GlyphMesher::new(&faces, &balaur_core::strings::locale(eng))
            });
            mesher.mesh(shape)
        },
    )));
}

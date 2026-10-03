//! A word as geometry: the outline of every glyph in a shaped run, filled.
//!
//! cosmic-text lays the string out with the same shaper the widget layer
//! uses, swash yields each glyph's outline, the curves flatten, and the
//! contours are triangulated together so a letter's counters stay holes.
//! What comes out is a `mesh` like any other: a collider fits it, a ray
//! picks it, and `balaur_core::path::extrude` gives it thickness.
//!
//! Nothing here touches a GPU, so a headless build shapes the same word into
//! the same triangles a windowed one draws.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use balaur_core::mesh::{MeshData, TextShape};
use cosmic_text::{FontSystem, fontdb};
use swash::scale::ScaleContext;
use swash::zeno::{Command, PathData};

use crate::shape::{Chain, Chains, Tweaks, shape_into};

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
    underline: String,
    strikethrough: bool,
    overline: bool,
}

impl Key {
    fn of(shape: &TextShape) -> Self {
        Self {
            text: shape.text.clone(),
            font: shape.font.clone(),
            size: (shape.size * 1000.0) as u32,
            weight: shape.weight,
            italic: shape.italic,
            underline: shape.underline.clone(),
            strikethrough: shape.strikethrough,
            overline: shape.overline,
        }
    }
}

/// The shaper, the outline scaler and what they have already built.
pub struct GlyphMesher {
    fonts: FontSystem,
    /// Each face's import `scale` and `y_offset`, by the id `fonts` gave it.
    tweaks: Tweaks,
    chains: Chains,
    scale: ScaleContext,
    built: HashMap<Key, MeshData>,
}

impl GlyphMesher {
    /// A mesher over the project's faces, in chain order.
    pub fn new(faces: &[crate::fonts::FontFace], locale: &str) -> Self {
        let mut db = fontdb::Database::new();
        // The family each role loaded under. A request that names no font takes
        // the default family, and an empty database answers with whatever it
        // has: the icon face, which draws no letters, sorts before `ui-`.
        let mut role: std::collections::HashMap<&str, String> = HashMap::new();
        let mut tweaks = Tweaks::default();
        let mut chains = HashMap::new();
        for face in faces {
            let shared: Arc<Vec<u8>> = Arc::clone(&face.bytes);
            let data: Arc<dyn AsRef<[u8]> + Send + Sync> = shared;
            let loaded = db.load_font_source(fontdb::Source::Binary(data));
            tweaks.add(&loaded, face.tweak);
            if face.chain == "icon" {
                continue;
            }
            let Some(info) = loaded.first().and_then(|id| db.face(*id)) else {
                continue;
            };
            let Some((family, _)) = info.families.first() else {
                continue;
            };
            chains
                .entry(face.chain.to_string())
                .or_insert_with(|| Chain {
                    family: family.clone(),
                    scale: face.tweak.scale,
                    weight: info.weight.0,
                });
            role.entry(face.chain).or_insert_with(|| family.clone());
        }
        let text = ["ui", "heading", "system", "mono"]
            .into_iter()
            .find_map(|chain| role.get(chain))
            .cloned();
        if let Some(text) = text {
            db.set_sans_serif_family(text.clone());
            db.set_serif_family(text.clone());
            db.set_cursive_family(text.clone());
            db.set_fantasy_family(text);
        }
        if let Some(mono) = role.get("mono") {
            db.set_monospace_family(mono.clone());
        }
        Self {
            fonts: FontSystem::new_with_locale_and_db(locale.to_string(), db),
            tweaks,
            chains: Chains(chains),
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

    /// Every contour of every glyph and every decoration bar, in world units
    /// with y up. Shaped the way a label is, so a face's import `scale` and
    /// `y_offset` and a chain's weight reach a word as they reach a label.
    fn contours(&mut self, shape: &TextShape) -> Vec<Vec<[f32; 2]>> {
        let mut options = crate::Options {
            font_name: shape.font.clone(),
            ..crate::Options::default()
        };
        options.decoration.underline = crate::Underline::of(&shape.underline);
        options.decoration.strikethrough = shape.strikethrough;
        options.decoration.overline = shape.overline;
        let request = crate::RequestRef {
            text: &shape.text,
            size: SHAPING_PIXELS,
            weight: shape.weight,
            slant: if shape.italic {
                crate::Slant::Italic
            } else {
                crate::Slant::Normal
            },
            width: None,
            truncate: false,
            align: crate::Align::Start,
            markup: false,
            font: "",
            family: "",
            // Baseline to baseline at one em, as a mesh's lines have always sat.
            line_height: 1.0,
            letter_spacing: 0.0,
            options: &options,
        };
        // Shaping first, outlines after: the layout borrows the font set,
        // and scaling a glyph needs it back.
        let mut placed: Vec<(fontdb::ID, fontdb::Weight, u16, f32, [f32; 2])> = Vec::new();
        let mut bars: Vec<[[f32; 2]; 4]> = Vec::new();
        {
            let buffer = shape_into(&mut self.fonts, &self.tweaks, &self.chains, &request);
            // The shaper measures down from the block's top and the outline
            // measures up from the baseline. The mesh is y up and sits on the
            // first line's baseline, the way a font places a word.
            let mut first = None;
            for run in buffer.layout_runs() {
                let base = *first.get_or_insert(run.line_y);
                for glyph in run.glyphs {
                    // `y_offset` lowers the glyph, never the line.
                    let lower = glyph.font_size * self.tweaks.of(glyph.font_id).y_offset;
                    let origin = [glyph.x, base - run.line_y - glyph.y - lower];
                    placed.push((
                        glyph.font_id,
                        glyph.font_weight,
                        glyph.glyph_id,
                        glyph.font_size,
                        origin,
                    ));
                }
                for (bar, _) in crate::decorations(&run) {
                    let (top, bottom) = (base - bar.min.y, base - bar.max.y);
                    bars.push([
                        [bar.min.x, bottom],
                        [bar.max.x, bottom],
                        [bar.max.x, top],
                        [bar.min.x, top],
                    ]);
                }
            }
        }
        let unit = shape.size / SHAPING_PIXELS;
        let mut out = Vec::new();
        for (font_id, font_weight, glyph_id, size, origin) in placed {
            let Some(font) = self.fonts.get_font(font_id, font_weight) else {
                continue;
            };
            let mut scaler = self
                .scale
                .builder(font.as_swash())
                .size(size)
                .hint(false)
                .build();
            let Some(outline) = scaler.scale_outline(glyph_id) else {
                continue;
            };
            walk(outline.path(), origin, unit, &mut out);
        }
        // A bar has to wind the way the glyphs do, or where it crosses one
        // the non-zero fill cancels the two out and leaves a hole.
        let glyphs_wind = out.iter().map(|c| signed_area(c)).sum::<f32>() >= 0.0;
        for bar in bars {
            let mut ring: Vec<[f32; 2]> = bar.iter().map(|p| [p[0] * unit, p[1] * unit]).collect();
            if (signed_area(&ring) >= 0.0) != glyphs_wind {
                ring.reverse();
            }
            out.push(ring);
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
        let (points, triangles) = balaur_core::triangulate::triangulate_shape(&contours);
        if triangles.is_empty() {
            return Err(anyhow!("'{}' filled to nothing", shape.text));
        }
        Ok(fill_mesh(&points, &triangles))
    }
}

/// The filled outline as a mesh in the z = 0 plane, facing +z.
fn fill_mesh(points: &[[f32; 2]], indices: &[[u32; 3]]) -> MeshData {
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
        indices: indices.to_vec(),
        normals: Some(vec![[0.0, 0.0, 1.0]; points.len()]),
        uvs: Some(
            points
                .iter()
                .map(|p| [(p[0] - min[0]) / span[0], (p[1] - min[1]) / span[1]])
                .collect(),
        ),
        colors: None,
        morphs: Vec::new(),
        source: None,
        part: None,
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

/// Twice a closed contour's area, positive counter-clockwise with y up.
fn signed_area(contour: &[[f32; 2]]) -> f32 {
    let n = contour.len();
    (0..n)
        .map(|i| {
            let (a, b) = (contour[i], contour[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
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
pub fn install(reg: &mut balaur_plugin::Registry<'_>) {
    let mesher: std::cell::RefCell<Option<GlyphMesher>> = std::cell::RefCell::new(None);
    reg.insert_resource(balaur_core::mesh::TextGeometry(Box::new(
        move |eng, shape| {
            let mut slot = mesher.borrow_mut();
            let mesher = slot.get_or_insert_with(|| {
                // The project's and the bundled faces only: a text mesh is
                // real geometry — colliders fit it and rays pick it — so the
                // machine's own fonts must not reach it.
                let mut faces = crate::fonts::font_faces(eng);
                faces.retain(|face| face.chain != "system");
                GlyphMesher::new(&faces, &balaur_core::strings::locale(eng))
            });
            mesher.mesh(shape)
        },
    )));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesher(tweak: crate::fonts::FaceTweak) -> GlyphMesher {
        let face = crate::fonts::FontFace {
            name: "ui-SourceSans3-Regular".into(),
            chain: "ui",
            bytes: Arc::new(
                include_bytes!("../../../editor/fonts/ui-SourceSans3-Regular.ttf").to_vec(),
            ),
            tweak,
        };
        GlyphMesher::new(&[face], "en-US")
    }

    fn word(text: &str) -> TextShape {
        TextShape {
            text: text.into(),
            size: 1.0,
            weight: 400,
            ..TextShape::default()
        }
    }

    /// The lowest and highest y, and the width, the mesh covers.
    fn extent(mesh: &MeshData) -> (f32, f32, f32) {
        let ys = mesh.positions.iter().map(|p| p[1]);
        let xs = mesh.positions.iter().map(|p| p[0]);
        let (low, high) = ys.fold((f32::MAX, f32::MIN), |(l, h), y| (l.min(y), h.max(y)));
        let (left, right) = xs.fold((f32::MAX, f32::MIN), |(l, r), x| (l.min(x), r.max(x)));
        (low, high, right - left)
    }

    fn area(mesh: &MeshData) -> f32 {
        mesh.indices
            .iter()
            .map(|[a, b, c]| {
                let (a, b, c) = (
                    mesh.positions[*a as usize],
                    mesh.positions[*b as usize],
                    mesh.positions[*c as usize],
                );
                ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs()
            })
            .sum()
    }

    #[test]
    fn a_face_scale_sizes_a_mesh_word_as_it_sizes_a_label() {
        let plain = mesher(crate::fonts::FaceTweak::default())
            .mesh(&word("HI"))
            .unwrap();
        let scaled = mesher(crate::fonts::FaceTweak {
            scale: 2.0,
            ..Default::default()
        })
        .mesh(&word("HI"))
        .unwrap();
        let ((_, top, wide), (_, top2, wide2)) = (extent(&plain), extent(&scaled));
        assert!((wide2 - wide * 2.0).abs() < 0.05, "{wide} against {wide2}");
        assert!((top2 - top * 2.0).abs() < 0.05, "{top} against {top2}");
    }

    #[test]
    fn a_face_y_offset_lowers_a_mesh_word() {
        let plain = mesher(crate::fonts::FaceTweak::default())
            .mesh(&word("H"))
            .unwrap();
        let lowered = mesher(crate::fonts::FaceTweak {
            y_offset: 0.25,
            ..Default::default()
        })
        .mesh(&word("H"))
        .unwrap();
        let drop = extent(&plain).0 - extent(&lowered).0;
        assert!(
            (drop - 0.25).abs() < 0.02,
            "dropped {drop} for a quarter em"
        );
    }

    #[test]
    fn an_underline_fills_a_bar_below_the_baseline() {
        let mut mesher = mesher(crate::fonts::FaceTweak::default());
        let plain = mesher.mesh(&word("HH")).unwrap();
        let mut marked = word("HH");
        marked.underline = crate::vocabulary::words::SINGLE.into();
        let underlined = mesher.mesh(&marked).unwrap();
        assert!(
            extent(&underlined).0 < extent(&plain).0 - 0.02,
            "nothing below the baseline"
        );
        assert!(area(&underlined) > area(&plain));
    }

    /// A strikethrough crosses every glyph; wound against them it would cut
    /// them in two instead of joining them.
    #[test]
    fn a_strikethrough_joins_the_glyphs_it_crosses_rather_than_cutting_them() {
        let mut mesher = mesher(crate::fonts::FaceTweak::default());
        let plain = mesher.mesh(&word("oo")).unwrap();
        let mut marked = word("oo");
        marked.strikethrough = true;
        let struck = mesher.mesh(&marked).unwrap();
        assert!(area(&struck) > area(&plain), "the bar took ink away");
    }
}

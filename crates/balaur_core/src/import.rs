//! Import settings: how a file is read, stated beside the file.
//!
//! Godot writes a `.import` next to every asset; here it is a TOML sidecar
//! named after the whole file, so `art/hero.png.import.toml` sorts beside its image
//! and cannot collide with a scene or a clip of the same stem.
//! `[import.<kind>]` in `project.toml` sets the default for every file of a
//! kind, and the sidecar overrides it key by key.
//!
//! Settings change pixels and samples, never sizes. A headless run resolves
//! them for nothing and computes the same world, which is why nothing here
//! is allowed to move an extent. The one exception is an SVG's `scale`, which
//! is how many pixels it has, and which every build measures alike.

use std::collections::HashMap;
use std::rc::Rc;

use crate::engine::Engine;
use crate::project::ProjectFiles;

/// The kinds a file sorts into. A kind names its defaults table in
/// `project.toml` and the settings its reader looks for.
pub mod kinds {
    pub const TEXTURE: &str = "texture";
    pub const AUDIO: &str = "audio";
    pub const FONT: &str = "font";
    pub const MODEL: &str = "model";
}

/// Setting keys, so a reader, the editor and the exporter spell them alike.
///
/// A key is here once something reads it: a setting a backend cannot honour
/// would be a promise the picture does not keep.
pub mod keys {
    pub const FILTER: &str = "filter";
    /// `filter` for one direction alone, when the two differ.
    pub const MAG_FILTER: &str = "mag_filter";
    pub const MIN_FILTER: &str = "min_filter";
    pub const SRGB: &str = "srgb";
    pub const REPEAT: &str = "repeat";
    /// `repeat` for one axis alone, for a texture that tiles across and
    /// clamps down.
    pub const REPEAT_U: &str = "repeat_u";
    pub const REPEAT_V: &str = "repeat_v";
    pub const MIPMAPS: &str = "mipmaps";
    pub const MIPMAP_FILTER: &str = "mipmap_filter";
    pub const ANISOTROPY: &str = "anisotropy";
    pub const PREMULTIPLY: &str = "premultiply";
    pub const RECODE: &str = "recode";
    /// A re-encode's quality for this file alone: imagequant's 0 to 100 for a
    /// picture, libvorbis's -0.1 to 1.0 for a sound.
    pub const QUALITY: &str = "quality";
    /// The pixel size an image was drawn at, when a smaller copy shipped in
    /// its place. Written by an export that folds a variant or caps a size;
    /// read by whatever measures the picture rather than samples it.
    pub const SIZE: &str = "size";
    /// A texture's pixels to one world unit, which a node that states none
    /// takes: a sprite whose own `pixels_per_unit` is 0.
    pub const PIXELS_PER_UNIT: &str = "pixels_per_unit";
    /// A normal map: data rather than colour, so `srgb` defaults off.
    pub const NORMAL_MAP: &str = "normal_map";
    /// Invert green, for a normal map baked the way DirectX reads one.
    pub const FLIP_GREEN: &str = "flip_green";
    /// Spread the edge colour into fully transparent texels, so a linear
    /// filter never samples the colour hiding under alpha zero.
    pub const BLEED: &str = "bleed";
    /// Pixels per unit when an SVG is rasterized; a face's size multiplier.
    pub const SCALE: &str = "scale";
    /// A sound loops wherever it is played.
    pub const LOOP: &str = "loop";
    /// Seconds into a looping sound its repeats start from, past an intro.
    pub const LOOP_OFFSET: &str = "loop_offset";
    /// A sound's own gain, multiplied into every play of it.
    pub const VOLUME: &str = "volume";
    /// The family a face joins: `ui`, `heading`, `mono` or `icon`.
    pub const FONT_FAMILY: &str = "font_family";
    /// A face's vertical nudge, as a fraction of its size.
    pub const Y_OFFSET: &str = "y_offset";
    /// Snap a face's outlines to the pixel grid; off for a smooth face.
    pub const HINTING: &str = "hinting";
    /// Where a model's origin moves to, in its own units after `scale`.
    pub const OFFSET: &str = "offset";
    /// How many simpler copies of a model to build, each with half the
    /// triangles of the one before.
    pub const LODS: &str = "lods";
    /// The distance from the camera, in world units, at which the first
    /// simpler copy takes over; each further one takes over at twice it.
    pub const LOD_DISTANCE: &str = "lod_distance";
    /// Mix a sound to one channel at export.
    pub const MONO: &str = "mono";
    /// The highest sample rate a sound ships at, in Hz; 0 keeps its own.
    pub const MAX_RATE: &str = "max_rate";
    /// Smooth a face's glyph edges; off draws every pixel on or off.
    pub const ANTIALIAS: &str = "antialias";
}

/// The values a key takes.
pub mod words {
    pub const NEAREST: &str = "nearest";
    pub const LINEAR: &str = "linear";
    /// `repeat = "repeat"`: the texture tiles past its edge.
    pub const REPEAT: &str = "repeat";
    /// Every other tile flipped, so a tiling texture has no seam.
    pub const MIRROR: &str = "mirror";
    /// The edge texel held, which is what a sprite wants.
    pub const CLAMP: &str = "clamp";
    /// `recode = "keep"`: ship this file's own bytes whatever the export's
    /// mode is.
    pub const KEEP: &str = "keep";
    /// `recode` for a picture: lossless WebP, or a 256-colour palette.
    pub const WEBP: &str = "webp";
    pub const QUANTISED: &str = "quantised";
    /// `recode` for a sound: lossless FLAC, or lossy Ogg Vorbis.
    pub const FLAC: &str = "flac";
    pub const VORBIS: &str = "vorbis";
    /// The font families `font_family` names.
    pub const UI: &str = "ui";
    pub const HEADING: &str = "heading";
    pub const MONO: &str = "mono";
    pub const ICON: &str = "icon";
}

/// Which kind a file belongs to, by extension, or `None` for a file no
/// importer claims.
#[must_use]
pub fn kind_of(path: &str) -> Option<&'static str> {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    match extension.as_str() {
        // The formats `crate::pixels` reads. An SVG is rasterized, which a
        // build without the `svg` feature leaves to `balaur export`.
        "png" | "webp" | "jpg" | "jpeg" | "svg" => Some(kinds::TEXTURE),
        "ogg" | "wav" | "mp3" | "flac" => Some(kinds::AUDIO),
        "ttf" | "otf" | "ttc" | "fnt" => Some(kinds::FONT),
        "glb" | "gltf" | "obj" => Some(kinds::MODEL),
        _ => None,
    }
}

/// The sidecar that states one file's settings.
#[must_use]
pub fn sidecar_of(path: &str) -> String {
    format!("{path}{SIDECAR}")
}

/// What a sidecar's name adds to the file it describes: `hero.png.import.toml`.
pub const SIDECAR: &str = ".import.toml";

/// Whether a project file is a sidecar rather than content of its own.
///
/// The exporter and the asset walk both need this: a sidecar is settings,
/// not an asset a scene may name.
#[must_use]
pub fn is_sidecar(path: &str) -> bool {
    let Some(rest) = path.strip_suffix(SIDECAR) else {
        return false;
    };
    kind_of(rest).is_some()
}

/// One file's settings and the stamp naming them, resolved once per asset
/// generation.
pub struct Resolved {
    pub settings: toml::Table,
    /// [`stamp`] of those settings, so a caller naming an upload does not
    /// hash the table again on every frame that draws it.
    pub stamp: String,
}

/// The settings already resolved, dropped whole when an asset is saved.
#[derive(Default)]
struct ImportState {
    generation: u64,
    files: HashMap<String, Rc<Resolved>>,
}

/// One file's settings, resolved once.
///
/// A sprite is attached to its texture on every frame that draws it, and each
/// attach used to open the sidecar again; the answer only changes when a file
/// is saved, which is what moves the asset generation.
#[must_use]
pub fn resolved(eng: &Engine, path: &str) -> Rc<Resolved> {
    let generation = crate::assets::generation(eng);
    let cache = if let Some(found) = eng.try_resource::<ImportState>() {
        found
    } else {
        eng.insert_resource(ImportState::default());
        eng.resource::<ImportState>()
    };
    {
        let held = cache.borrow();
        if held.generation == generation
            && let Some(found) = held.files.get(path)
        {
            return Rc::clone(found);
        }
    }
    let settings = settings(eng, path);
    let found = Rc::new(Resolved {
        stamp: stamp(&settings),
        settings,
    });
    let mut cache = cache.borrow_mut();
    if cache.generation != generation {
        cache.files.clear();
        cache.generation = generation;
    }
    cache.files.insert(path.to_string(), Rc::clone(&found));
    found
}

/// The size an image was drawn at, when its sidecar says a smaller copy
/// shipped in its place; `None` for a file that is its own size.
///
/// 2D measures with pixels — a sprite's quad, a sheet's frames, a tile — so
/// a shrunk texture must still answer with the pixels the artist counted.
#[must_use]
pub fn drawn_size(eng: &Engine, path: &str) -> Option<(u32, u32)> {
    size_in(&resolved(eng, path).settings)
}

/// [`keys::SIZE`] out of a settings table: two counts, both above zero.
fn size_in(settings: &toml::Table) -> Option<(u32, u32)> {
    let pair = settings.get(keys::SIZE)?.as_array()?;
    let side = |at: usize| {
        u32::try_from(pair.get(at)?.as_integer()?)
            .ok()
            .filter(|n| *n > 0)
    };
    Some((side(0)?, side(1)?))
}

/// One file's settings: the project's defaults for its kind, with the
/// sidecar's keys written over them.
///
/// Answers an empty table for a file of no kind, and never fails: a sidecar
/// that does not parse is reported and ignored, because a game that refused
/// to start over one is worse than a game that starts with plain defaults.
#[must_use]
pub fn settings(eng: &Engine, path: &str) -> toml::Table {
    let mut table = defaults(eng, path);
    let Some(files) = eng.try_resource::<ProjectFiles>() else {
        return table;
    };
    let sidecar = sidecar_of(path);
    // A sidecar is a document, kept with a pack's scenes where `ProjectFiles`
    // never looks; the file read is for a run with no scene source at all.
    let bytes = match crate::project::scene_text(eng, &sidecar) {
        Ok(text) => text.into_bytes(),
        Err(_) => match files.borrow().read(&sidecar) {
            Ok(bytes) => bytes,
            Err(_) => return table,
        },
    };
    if let Ok(Ok(overrides)) = std::str::from_utf8(&bytes).map(toml::from_str::<toml::Table>) {
        for (key, value) in overrides {
            table.insert(key, value);
        }
    } else {
        tracing::warn!("{sidecar} is not a settings file; using the defaults");
    }
    table
}

/// One file's settings for a tool with no engine, such as an export or
/// `balaur shrink`: `[import.<kind>]` from a manifest already resolved for its
/// target, with the sidecar's text written over it.
#[must_use]
pub fn merged(manifest: &toml::Table, path: &str, sidecar: Option<&str>) -> toml::Table {
    let mut table = kind_of(path)
        .and_then(|kind| manifest.get("import")?.get(kind)?.as_table().cloned())
        .unwrap_or_default();
    if let Some(text) = sidecar {
        if let Ok(own) = toml::from_str::<toml::Table>(text) {
            table.extend(own);
        } else {
            tracing::warn!("{} is not a settings file", sidecar_of(path));
        }
    }
    table
}

/// The project's defaults for whatever kind `path` is, as this run resolves
/// them: `[override.mobile.import.texture] mipmaps = false` answers on a phone.
fn defaults(eng: &Engine, path: &str) -> toml::Table {
    let Some(kind) = kind_of(path) else {
        return toml::Table::new();
    };
    crate::settings::table(eng, &format!("import/{kind}"))
}

/// A short stable name for a resolved set of settings.
///
/// Folded into the name a texture is uploaded under, so changing one
/// image's filter re-uploads that image and leaves the rest alone.
///
/// The engine's own digest rather than `DefaultHasher`, whose numbers are not
/// promised to be the same from one Rust release to the next, and key by key
/// in sorted order so two tables holding the same settings stamp alike
/// however they were written.
#[must_use]
pub fn stamp(settings: &toml::Table) -> String {
    if settings.is_empty() {
        return String::new();
    }
    let mut keys: Vec<&String> = settings.keys().collect();
    keys.sort_unstable();
    let mut hash = crate::assets::FNV_OFFSET;
    for key in keys {
        hash = crate::assets::digest_bytes(hash, key.as_bytes());
        hash = crate::assets::digest_bytes(hash, settings[key].to_string().as_bytes());
    }
    format!("{hash:016x}")
}

/// A string setting, or `fallback` when it is missing or of another type.
#[must_use]
pub fn word<'a>(settings: &'a toml::Table, key: &str, fallback: &'a str) -> &'a str {
    settings
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or(fallback)
}

/// A boolean setting, or `fallback` when it is missing or of another type.
#[must_use]
pub fn flag(settings: &toml::Table, key: &str, fallback: bool) -> bool {
    settings
        .get(key)
        .and_then(toml::Value::as_bool)
        .unwrap_or(fallback)
}

/// A number setting, integer or float, or `fallback` when it is missing or
/// of another type.
#[must_use]
pub fn number(settings: &toml::Table, key: &str, fallback: f64) -> f64 {
    match settings.get(key) {
        Some(toml::Value::Float(found)) if found.is_finite() => *found,
        Some(toml::Value::Integer(found)) => *found as f64,
        _ => fallback,
    }
}

/// A whole-number setting, or `fallback` when it is missing or of another
/// type.
#[must_use]
pub fn count(settings: &toml::Table, key: &str, fallback: u16) -> u16 {
    settings
        .get(key)
        .and_then(toml::Value::as_integer)
        .and_then(|found| u16::try_from(found).ok())
        .unwrap_or(fallback)
}

/// What a texture's settings say about sampling it, with the words already
/// read and the constraints already applied.
///
/// Here rather than in the renderer so a headless test can prove the merge
/// and the refusals without a GPU, and so the editor and the exporter read
/// the same answer the picture does.
pub mod texture {
    use super::{count, flag, keys, number, word, words};

    /// Between texels.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Filter {
        /// The nearest texel, which is what keeps pixel art crisp.
        Nearest,
        /// Interpolated between texels.
        Linear,
    }

    /// What a coordinate past the edge reads.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Wrap {
        Repeat,
        Mirror,
        Clamp,
    }

    /// One texture's whole sampler, resolved.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Sampling {
        pub mag: Filter,
        pub min: Filter,
        pub mipmap: Filter,
        pub wrap_u: Wrap,
        pub wrap_v: Wrap,
        pub mipmaps: bool,
        /// Samples per fetch, 1 to 16, where 1 is off.
        pub anisotropy: u16,
        pub srgb: bool,
        pub premultiply: bool,
    }

    /// What the engine samples with when nothing says otherwise: a sprite's
    /// clamped bilinear colour, with no mip chain to build at load.
    impl Default for Sampling {
        fn default() -> Self {
            Sampling {
                mag: Filter::Linear,
                min: Filter::Linear,
                mipmap: Filter::Linear,
                wrap_u: Wrap::Clamp,
                wrap_v: Wrap::Clamp,
                mipmaps: false,
                anisotropy: 1,
                srgb: true,
                premultiply: false,
            }
        }
    }

    /// The sampler one file's resolved settings ask for.
    ///
    /// Every key falls back to the one above it — `mag_filter` to `filter`,
    /// `repeat_u` to `repeat` — so a project that samples the same way in
    /// both directions writes one line.
    #[must_use]
    pub fn sampling(settings: &toml::Table) -> Sampling {
        let base = Sampling::default();
        let both = filter(settings, keys::FILTER, base.mag);
        let axes = wrap(settings, keys::REPEAT, base.wrap_u);
        Sampling {
            mag: filter(settings, keys::MAG_FILTER, both),
            min: filter(settings, keys::MIN_FILTER, both),
            mipmap: filter(settings, keys::MIPMAP_FILTER, base.mipmap),
            wrap_u: wrap(settings, keys::REPEAT_U, axes),
            wrap_v: wrap(settings, keys::REPEAT_V, axes),
            mipmaps: flag(settings, keys::MIPMAPS, base.mipmaps),
            anisotropy: count(settings, keys::ANISOTROPY, base.anisotropy).clamp(1, 16),
            srgb: flag(
                settings,
                keys::SRGB,
                !flag(settings, keys::NORMAL_MAP, !base.srgb),
            ),
            premultiply: flag(settings, keys::PREMULTIPLY, base.premultiply),
        }
    }

    /// The smallest and largest `scale` an SVG is rasterized at.
    pub const SCALE_RANGE: (f32, f32) = (0.01, 64.0);

    /// What a texture's settings do to its texels before anything samples
    /// them.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Texels {
        pub bleed: bool,
        pub flip_green: bool,
        /// Pixels per unit for an SVG; a raster ignores it.
        pub scale: f32,
    }

    /// The texel settings one file's resolved settings ask for. `bleed` is on
    /// by default for colour sampled linearly, the one case it changes.
    #[must_use]
    pub fn texels(settings: &toml::Table) -> Texels {
        let sampled = sampling(settings);
        let linear = sampled.mag == Filter::Linear || sampled.min == Filter::Linear;
        let (least, most) = SCALE_RANGE;
        Texels {
            bleed: flag(settings, keys::BLEED, sampled.srgb && linear),
            flip_green: flag(settings, keys::FLIP_GREEN, false),
            scale: (number(settings, keys::SCALE, 1.0) as f32).clamp(least, most),
        }
    }

    /// Pixel art: magnified nearest, so every texel is a square a smaller
    /// copy would drop.
    #[must_use]
    pub fn is_pixel_art(settings: &toml::Table) -> bool {
        sampling(settings).mag == Filter::Nearest
    }

    /// Whether this sampler asks for anisotropy the filters cannot give it.
    ///
    /// Anisotropy averages between texels and between mip levels; a nearest
    /// filter does neither, and a GPU refuses the pair. The caller reports it
    /// and samples without, rather than dropping the texture.
    #[must_use]
    pub fn anisotropy_refused(sampling: &Sampling) -> bool {
        sampling.anisotropy > 1
            && (sampling.mag != Filter::Linear
                || sampling.min != Filter::Linear
                || sampling.mipmap != Filter::Linear)
    }

    fn filter(settings: &toml::Table, key: &str, fallback: Filter) -> Filter {
        match word(settings, key, "") {
            words::NEAREST => Filter::Nearest,
            words::LINEAR => Filter::Linear,
            _ => fallback,
        }
    }

    fn wrap(settings: &toml::Table, key: &str, fallback: Wrap) -> Wrap {
        match word(settings, key, "") {
            words::REPEAT => Wrap::Repeat,
            words::MIRROR => Wrap::Mirror,
            words::CLAMP => Wrap::Clamp,
            _ => fallback,
        }
    }
}

/// What a model file's settings do to the geometry a mesh reads from it.
pub mod model {
    use super::{keys, number};
    use crate::mesh::MeshData;

    /// The most simpler copies a model may ask for.
    pub const MAX_LODS: usize = 6;
    /// Where the first simpler copy takes over when the model does not say.
    pub const DEFAULT_LOD_DISTANCE: f32 = 20.0;

    /// The triangles of each simpler copy a model's `lods` asks for, level
    /// one first; empty for none. A skinned or morphing mesh gets none: its
    /// vertices move, and a copy simplified at rest would tear.
    #[must_use]
    pub fn lod_levels(mesh: &MeshData, settings: &toml::Table) -> Vec<Vec<[u32; 3]>> {
        let wanted = number(settings, keys::LODS, 0.0).clamp(0.0, MAX_LODS as f64) as usize;
        if wanted == 0 || mesh.skin.is_some() || !mesh.morphs.is_empty() {
            return Vec::new();
        }
        let source: Vec<u32> = mesh.indices.iter().flatten().copied().collect();
        let mut levels = Vec::new();
        let mut held = source.len();
        // A farther copy may stray further from the shape: 2% of the model's
        // size at the first, doubling from there.
        let mut error = 0.02f32;
        for _ in 0..wanted {
            let target = (held / 2) / 3 * 3;
            if target < 3 {
                break;
            }
            let mut out = vec![0u32; source.len()];
            let kept =
                meshopt_rs::simplify::simplify(&mut out, &source, &mesh.positions, target, error);
            if kept == 0 || kept >= held {
                break;
            }
            out.truncate(kept);
            held = kept;
            levels.push(out.as_chunks::<3>().0.to_vec());
            error = (error * 2.0).min(0.5);
        }
        levels
    }

    /// The camera distance each simpler copy takes over at, level one first.
    #[must_use]
    pub fn lod_distances(settings: &toml::Table, levels: usize) -> Vec<f32> {
        let first = number(
            settings,
            keys::LOD_DISTANCE,
            f64::from(DEFAULT_LOD_DISTANCE),
        );
        let first = if first > 0.0 {
            first as f32
        } else {
            DEFAULT_LOD_DISTANCE
        };
        std::iter::successors(Some(first), |d| Some(d * 2.0))
            .take(levels)
            .collect()
    }

    /// A model file's `scale` and `offset`, applied to what a mesh reads from it:
    /// a file modelled in centimetres, or around a corner rather than its centre.
    /// A rig's bones are nodes a scene places, so a skinned mesh is left as is.
    pub fn place(mesh: &mut MeshData, settings: &toml::Table, source: &str) {
        let triple = |key: &str, fallback: f32| -> [f32; 3] {
            match settings.get(key) {
                Some(toml::Value::Array(items)) if items.len() == 3 => {
                    let at = |i: usize| match &items[i] {
                        toml::Value::Float(f) => *f as f32,
                        toml::Value::Integer(n) => *n as f32,
                        _ => fallback,
                    };
                    [at(0), at(1), at(2)]
                }
                _ => [number(settings, key, f64::from(fallback)) as f32; 3],
            }
        };
        let scale = triple(keys::SCALE, 1.0).map(|s| if s.abs() < 1e-6 { 1.0 } else { s });
        let offset = triple(keys::OFFSET, 0.0);
        let unit = scale.iter().all(|s| (s - 1.0).abs() < f32::EPSILON);
        if unit && offset.iter().all(|o| o.abs() < f32::EPSILON) {
            return;
        }
        if mesh.skin.is_some() {
            tracing::warn!(
                "{source}: `scale` and `offset` move no rig, so a skinned mesh is left as authored"
            );
            return;
        }
        for p in &mut mesh.positions {
            *p = [0, 1, 2].map(|i| p[i] * scale[i] + offset[i]);
        }
        for morph in &mut mesh.morphs {
            for d in &mut morph.positions {
                *d = [0, 1, 2].map(|i| d[i] * scale[i]);
            }
        }
        if let Some(normals) = &mut mesh.normals {
            for n in normals.iter_mut() {
                let bent = [0, 1, 2].map(|i| n[i] / scale[i]);
                let length = bent.iter().map(|c| c * c).sum::<f32>().sqrt().max(1e-12);
                *n = bent.map(|c| c / length);
            }
        }
        // A mirror turns every triangle inside out; swapping two corners puts it back.
        if scale.iter().filter(|s| **s < 0.0).count() % 2 == 1 {
            for triangle in &mut mesh.indices {
                triangle.swap(1, 2);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::place;

        const TRIANGLE: &str = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";

        fn close(got: [f32; 3], want: [f32; 3]) -> bool {
            got.iter().zip(&want).all(|(a, b)| (a - b).abs() < 1e-6)
        }

        /// A grid of `side` by `side` quads in the z = 0 plane.
        fn grid(side: u32) -> crate::mesh::MeshData {
            let mut mesh = crate::mesh::MeshData::default();
            for y in 0..=side {
                for x in 0..=side {
                    let bump = ((x * 7 + y * 3) % 5) as f32 * 0.01;
                    mesh.positions.push([x as f32, y as f32, bump]);
                }
            }
            let at = |x: u32, y: u32| y * (side + 1) + x;
            for y in 0..side {
                for x in 0..side {
                    mesh.indices
                        .push([at(x, y), at(x + 1, y), at(x + 1, y + 1)]);
                    mesh.indices
                        .push([at(x, y), at(x + 1, y + 1), at(x, y + 1)]);
                }
            }
            mesh
        }

        /// Each simpler copy has at most half the triangles of the one before.
        #[test]
        fn a_model_asking_for_lods_gets_simpler_copies() {
            let mesh = grid(16);
            let settings: toml::Table = toml::from_str("lods = 2\nlod_distance = 8").unwrap();
            let levels = super::lod_levels(&mesh, &settings);
            assert_eq!(levels.len(), 2);
            assert!(
                levels[0].len() * 2 <= mesh.indices.len() + 2,
                "{}",
                levels[0].len()
            );
            assert!(levels[1].len() < levels[0].len());
            assert_eq!(super::lod_distances(&settings, 2), vec![8.0, 16.0]);
            assert!(super::lod_levels(&mesh, &toml::Table::new()).is_empty());
        }

        /// A model file's own `scale` and `offset` place its geometry.
        #[test]
        fn a_model_is_scaled_and_moved_by_its_settings() {
            let mut mesh = crate::mesh::parse_obj(TRIANGLE.as_bytes(), "t.obj").unwrap();
            let settings: toml::Table = toml::from_str("scale = 0.01\noffset = [0, 1, 0]").unwrap();
            place(&mut mesh, &settings, "t.obj");
            assert!(close(mesh.positions[1], [0.01, 1.0, 0.0]));
            let mirrored: toml::Table = toml::from_str("scale = [-1, 1, 1]").unwrap();
            let before = mesh.indices[0];
            place(&mut mesh, &mirrored, "t.obj");
            assert_eq!(
                mesh.indices[0],
                [before[0], before[2], before[1]],
                "a mirror keeps its facing"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    /// Two counts above zero, or nothing: a size that is not a size would
    /// measure a sprite as a point.
    #[test]
    fn the_drawn_size_is_two_counts_or_nothing() {
        let read = |text: &str| super::size_in(&toml::from_str::<toml::Table>(text).unwrap());
        assert_eq!(read("size = [400, 200]"), Some((400, 200)));
        assert_eq!(read("size = [400]"), None);
        assert_eq!(read("size = [0, 200]"), None);
        assert_eq!(read("size = \"400x200\""), None);
        assert_eq!(read("filter = \"linear\""), None);
    }

    use super::{flag, is_sidecar, kind_of, kinds, merged, number, sidecar_of, stamp, word};

    /// A tool reads the project's default, then the file's own over it.
    #[test]
    fn a_tool_merges_the_project_default_under_the_sidecar() {
        let manifest: toml::Table =
            toml::from_str("[import.texture]\nfilter = \"nearest\"\nmipmaps = true").unwrap();
        let own = merged(&manifest, "art/hero.png", Some("mipmaps = false"));
        assert_eq!(word(&own, "filter", ""), "nearest");
        assert!(!flag(&own, "mipmaps", true));
        assert!(merged(&manifest, "sfx/hit.wav", None).is_empty());
        assert_eq!(
            merged(&manifest, "art/hero.png", Some("not = [toml")).len(),
            2
        );
    }

    #[test]
    fn a_number_reads_an_integer_or_a_float() {
        let table: toml::Table = toml::from_str("a = 2\nb = 0.5\nc = \"x\"").unwrap();
        assert!((number(&table, "a", 0.0) - 2.0).abs() < f64::EPSILON);
        assert!((number(&table, "b", 0.0) - 0.5).abs() < f64::EPSILON);
        assert!((number(&table, "c", 7.0) - 7.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_file_sorts_into_the_kind_its_extension_names() {
        assert_eq!(kind_of("art/hero.PNG"), Some(kinds::TEXTURE));
        assert_eq!(kind_of("art/photo.jpg"), Some(kinds::TEXTURE));
        assert_eq!(kind_of("art/logo.svg"), Some(kinds::TEXTURE));
        assert_eq!(kind_of("art/photo.tga"), None, "no reader decodes a TGA");
        assert_eq!(kind_of("sfx/hit.wav"), Some(kinds::AUDIO));
        assert_eq!(kind_of("fonts/pixel.fnt"), Some(kinds::FONT));
        assert_eq!(kind_of("scenes/main.toml"), None);
    }

    #[test]
    fn a_sidecar_sorts_beside_the_file_it_settles() {
        assert_eq!(sidecar_of("art/hero.png"), "art/hero.png.import.toml");
        assert!(is_sidecar("art/hero.png.import.toml"));
    }

    /// A scene and a clip are content, however they are named.
    #[test]
    fn a_scene_is_not_mistaken_for_a_sidecar() {
        assert!(!is_sidecar("scenes/main.toml"));
        assert!(!is_sidecar("animations/walk.toml"));
    }

    /// Two tables holding the same settings are the same settings, so the
    /// texture they name is uploaded once.
    #[test]
    fn the_order_a_table_was_written_in_does_not_change_its_stamp() {
        let one: toml::Table = toml::from_str("filter = \"nearest\"\nsrgb = false").unwrap();
        let other: toml::Table = toml::from_str("srgb = false\nfilter = \"nearest\"").unwrap();
        assert_eq!(stamp(&one), stamp(&other));
    }

    #[test]
    fn a_changed_setting_changes_the_stamp() {
        let nearest: toml::Table = toml::from_str(r#"filter = "nearest""#).unwrap();
        let linear: toml::Table = toml::from_str(r#"filter = "linear""#).unwrap();
        assert_ne!(stamp(&nearest), stamp(&linear));
        assert_eq!(stamp(&toml::Table::new()), "");
    }

    #[test]
    fn a_missing_setting_reads_as_its_fallback() {
        let table: toml::Table = toml::from_str(r#"filter = "nearest""#).unwrap();
        assert_eq!(word(&table, "filter", "linear"), "nearest");
        assert_eq!(word(&table, "repeat", "clamp"), "clamp");
        assert!(flag(&table, "srgb", true));
    }

    mod sampling {
        use crate::import::texture::{
            Filter, Sampling, Wrap, anisotropy_refused, is_pixel_art, sampling, texels,
        };

        fn of(source: &str) -> Sampling {
            sampling(&toml::from_str::<toml::Table>(source).unwrap())
        }

        fn table(source: &str) -> toml::Table {
            toml::from_str(source).unwrap()
        }

        /// A normal map is data, so it reads raw unless `srgb` says otherwise.
        #[test]
        fn a_normal_map_reads_raw_by_default() {
            assert!(!of("normal_map = true").srgb);
            assert!(of("normal_map = true\nsrgb = true").srgb);
            assert!(of("normal_map = false").srgb);
        }

        /// Bleeding only changes what a linear filter reads from colour.
        #[test]
        fn bleed_defaults_on_for_colour_sampled_linearly() {
            assert!(texels(&table("")).bleed);
            assert!(!texels(&table("filter = \"nearest\"")).bleed);
            assert!(!texels(&table("srgb = false")).bleed);
            assert!(!texels(&table("normal_map = true")).bleed);
            assert!(texels(&table("filter = \"nearest\"\nbleed = true")).bleed);
        }

        #[test]
        fn an_svg_scale_is_clamped_to_something_a_gpu_holds() {
            assert!((texels(&table("scale = 2")).scale - 2.0).abs() < f32::EPSILON);
            assert!((texels(&table("scale = 1000.0")).scale - 64.0).abs() < f32::EPSILON);
            assert!((texels(&table("")).scale - 1.0).abs() < f32::EPSILON);
        }

        #[test]
        fn pixel_art_is_what_magnifies_nearest() {
            assert!(is_pixel_art(&table("filter = \"nearest\"")));
            assert!(is_pixel_art(&table("mag_filter = \"nearest\"")));
            assert!(!is_pixel_art(&table("min_filter = \"nearest\"")));
        }

        /// A file that says nothing samples the way a sprite wants: clamped,
        /// smooth, and with no mip chain to build at load.
        #[test]
        fn a_file_with_no_settings_samples_the_way_it_always_did() {
            assert_eq!(of(""), Sampling::default());
        }

        /// One line for both directions, and the per-axis key only where they
        /// differ.
        #[test]
        fn a_per_axis_key_falls_back_to_the_one_that_covers_both() {
            let tiled = of("repeat = \"repeat\"\nrepeat_v = \"clamp\"");
            assert_eq!(tiled.wrap_u, Wrap::Repeat);
            assert_eq!(tiled.wrap_v, Wrap::Clamp);

            let pixel = of("filter = \"nearest\"\nmin_filter = \"linear\"");
            assert_eq!(pixel.mag, Filter::Nearest);
            assert_eq!(pixel.min, Filter::Linear);
        }

        #[test]
        fn every_key_is_read() {
            let all = of(
                "filter = \"nearest\"\nrepeat = \"mirror\"\nmipmaps = true\n\
                 mipmap_filter = \"nearest\"\nanisotropy = 8\nsrgb = false\n\
                 premultiply = true",
            );
            assert_eq!(all.mag, Filter::Nearest);
            assert_eq!(all.wrap_u, Wrap::Mirror);
            assert_eq!(all.mipmap, Filter::Nearest);
            assert!(all.mipmaps && all.premultiply && !all.srgb);
            assert_eq!(all.anisotropy, 8);
        }

        /// A misspelled value is the engine's own, not a texture that refuses
        /// to draw: a settings file is written by hand.
        #[test]
        fn a_word_nothing_knows_reads_as_the_default() {
            assert_eq!(of("filter = \"bilinear\"").mag, Filter::Linear);
            assert_eq!(of("repeat = \"tile\"").wrap_u, Wrap::Clamp);
            assert_eq!(of("anisotropy = \"lots\"").anisotropy, 1);
        }

        /// A GPU refuses anisotropy without a linear filter on every axis, so
        /// the pair is caught here and reported rather than sampled.
        #[test]
        fn anisotropy_asks_for_a_linear_filter() {
            assert!(anisotropy_refused(&of(
                "filter = \"nearest\"\nanisotropy = 4"
            )));
            assert!(!anisotropy_refused(&of("anisotropy = 4")));
            assert!(
                !anisotropy_refused(&of("filter = \"nearest\"")),
                "no anisotropy asked for is nothing to refuse"
            );
        }

        #[test]
        fn anisotropy_past_what_a_gpu_offers_is_clamped() {
            assert_eq!(of("anisotropy = 64").anisotropy, 16);
            assert_eq!(of("anisotropy = 0").anisotropy, 1);
            assert_eq!(of("anisotropy = -4").anisotropy, 1);
        }
    }
}

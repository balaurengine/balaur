//! What `balaur import` keeps of a glTF material: its factors, its maps, and
//! the `material` asset each becomes.
//!
//! glTF states a surface as a factor times a map. Balaur states it as a
//! `material` asset over one stock shader, so the whole of a file's material
//! survives the import rather than its base colour alone — and a model with a
//! surface per material becomes one mesh node per material, each drawing its
//! own.
//!
//! [`crate::glb`] owns the geometry and the scene tree; this owns the paint.

use anyhow::{Result, anyhow};

use crate::collections::DetHashMap;
use crate::glb::{Beside, Model, SideReader, floats, percent_decoded, slug, uri_bytes};

/// What a primitive with no material of its own draws with. A `part` may name
/// it, so it needs a name nothing else can take.
pub(crate) const DEFAULT_MATERIAL: &str = "default";

/// Every material's name, by index, unique within the file.
///
/// A mesh's `part` names one of these and an imported scene writes one
/// `material` asset per entry, so the two must agree exactly: both call this.
pub(crate) fn material_names(document: &gltf::Document) -> Vec<String> {
    let mut taken: DetHashMap<String, u32> = DetHashMap::default();
    taken.insert(DEFAULT_MATERIAL.to_string(), 1);
    let mut names = Vec::new();
    for material in document.materials() {
        let base = material.name().filter(|n| !n.is_empty()).map_or_else(
            || format!("material{}", names.len()),
            |n| n.replace('/', "_"),
        );
        let seen = taken.entry(base.clone()).or_insert(0);
        *seen += 1;
        names.push(if *seen == 1 {
            base
        } else {
            format!("{base}_{seen}")
        });
    }
    names
}

/// The name of the material a primitive draws with, from the table above.
pub(crate) fn drawn_with(names: &[String], primitive: &gltf::Primitive<'_>) -> String {
    primitive
        .material()
        .index()
        .and_then(|index| names.get(index).cloned())
        .unwrap_or_else(|| DEFAULT_MATERIAL.to_string())
}

/// The stock surface an imported glTF material draws with, written into the
/// project beside the model. One file however many materials a scene holds.
pub const MATERIAL_SHADER: &str = include_str!("glb_material.wesl");

/// Where [`MATERIAL_SHADER`] is written, project-relative.
pub const MATERIAL_SHADER_PATH: &str = "shaders/imported.wesl";

/// The texture slots a glTF material's maps land in, named as the render
/// contract's `TEXTURE_SLOTS` names them.
const SLOTS: [&str; 5] = [
    "albedo",
    "normal",
    "metallic_roughness",
    "occlusion",
    "emissive",
];

/// The images a file's materials name, pulled out once each.
///
/// Keyed by glTF image index, so two materials sharing a map share the file
/// written for it rather than writing it twice under two names.
pub(crate) struct Images<'a> {
    pub(crate) stem: &'a str,
    pub(crate) by_index: DetHashMap<usize, String>,
    pub(crate) files: Vec<(String, Beside)>,
}

/// How a glTF sampler says a texture is read, as the keys an import sidecar
/// spells them.
///
/// The map is only half of what a file says about a texture; the other half is
/// how to sample it, and Balaur's own defaults are not glTF's. A floor whose
/// UVs run to twenty and whose sampler is dropped clamps to one texel and
/// draws flat, which is what this exists to stop.
fn sampling_of(texture: &gltf::texture::Texture<'_>, colour: bool) -> String {
    let sampler = texture.sampler();
    let wrap = |mode| match mode {
        gltf::texture::WrappingMode::ClampToEdge => "clamp",
        gltf::texture::WrappingMode::MirroredRepeat => "mirror",
        gltf::texture::WrappingMode::Repeat => "repeat",
    };
    let filter = matches!(
        sampler.mag_filter(),
        Some(gltf::texture::MagFilter::Nearest)
    );
    // Every minification mode glTF spells but the two plain ones asks for a
    // mip chain.
    let mipmaps = !matches!(
        sampler.min_filter(),
        Some(gltf::texture::MinFilter::Nearest | gltf::texture::MinFilter::Linear)
    );
    // glTF has no word for anisotropy, and Balaur's default of one is a 2D
    // default: a mip chain alone blurs a floor into bands at a glancing angle.
    // Only with the filtering that can use it, which `nearest` cannot.
    let anisotropy = if filter || !mipmaps { 1 } else { 16 };
    format!(
        "# Written by `balaur import` from the model's own sampler.\n\
         repeat_u = \"{}\"\nrepeat_v = \"{}\"\nfilter = \"{}\"\n\
         mipmaps = {mipmaps}\nanisotropy = {anisotropy}\nsrgb = {colour}\n",
        wrap(sampler.wrap_s()),
        wrap(sampler.wrap_t()),
        if filter { "nearest" } else { "linear" },
    )
}

/// Whether a slot holds colour, which is the only kind of map sRGB describes.
///
/// A roughness or a normal read back through the sRGB curve is not the number
/// the file stored: 0.19 of roughness decodes to 0.03, and a scene of mirrors
/// is what a glancing sky then makes of it.
fn is_colour(slot: &str) -> bool {
    slot == SLOTS[0] || slot == SLOTS[4]
}

impl Images<'_> {
    /// The file name this image is written under, extracting it on first ask.
    /// A side image keeps the name it already has; an embedded one is named
    /// after the model and its index, which is the only name it has.
    pub(crate) fn file_for(
        &mut self,
        model: &Model,
        texture: &gltf::texture::Texture<'_>,
        side: SideReader<'_>,
        slot: &str,
    ) -> Result<String> {
        let image = texture.source();
        if let Some(name) = self.by_index.get(&image.index()) {
            return Ok(name.clone());
        }
        let (name, bytes) = match image.source() {
            gltf::image::Source::Uri { uri, .. } if !uri.starts_with("data:") => {
                // Named, not read: nothing here decodes a texture, so the
                // copy at the other end is the only thing that needs it.
                let name = percent_decoded(uri);
                (name.clone(), Beside::Named(name))
            }
            gltf::image::Source::Uri { uri, .. } => {
                let extension = if uri.starts_with("data:image/jpeg") {
                    "jpg"
                } else {
                    "png"
                };
                (
                    format!("{}_{}.{extension}", self.stem, image.index()),
                    Beside::Bytes(uri_bytes(uri, side)?),
                )
            }
            gltf::image::Source::View { view, mime_type } => {
                let bytes = model
                    .view_bytes(&view)
                    .ok_or_else(|| anyhow!("the embedded texture's buffer view is out of range"))?;
                let extension = match mime_type {
                    "image/jpeg" => "jpg",
                    _ => "png",
                };
                (
                    format!("{}_{}.{extension}", self.stem, image.index()),
                    Beside::Bytes(bytes.to_vec()),
                )
            }
        };
        self.by_index.insert(image.index(), name.clone());
        self.files.push((name.clone(), bytes));
        self.files.push((
            crate::import::sidecar_of(&name),
            Beside::Bytes(sampling_of(texture, is_colour(slot)).into_bytes()),
        ));
        Ok(name)
    }
}

/// One glTF material as the project reads it: its factors, its maps, and the
/// `[[assets]]` id the scene declares it under.
pub(crate) struct Surface {
    pub(crate) id: String,
    pub(crate) part: String,
    base_color: [f32; 4],
    emissive: [f32; 4],
    metallic: f32,
    roughness: f32,
    reflectance: f32,
    alpha_cutoff: f32,
    transmission: f32,
    ior: f32,
    thickness: f32,
    attenuation_color: [f32; 3],
    attenuation_distance: f32,
    pub(crate) double_sided: bool,
    /// `slot -> file name under `models/``, in [`SLOTS`] order.
    maps: Vec<(&'static str, String)>,
}

/// Balaur's `reflectance`, where 0.5 is the 4% of a common dielectric, from
/// the index of refraction and the specular scale glTF states instead.
///
/// `f0 = ((ior - 1) / (ior + 1))^2`, and the surface spells `f0` as
/// `0.16 * reflectance^2`, so the one is the other rearranged.
pub(crate) fn reflectance_of(ior: f32, specular: f32) -> f32 {
    let edge = (ior - 1.0) / (ior + 1.0);
    let f0 = edge * edge * specular.clamp(0.0, 1.0);
    (f0 / 0.16).sqrt().clamp(0.0, 1.0)
}

/// Every material the file declares, with its maps extracted.
pub(crate) fn surfaces(
    model: &Model,
    stem: &str,
    side: SideReader<'_>,
    images: &mut Images<'_>,
) -> Result<Vec<Surface>> {
    let names = material_names(&model.document);
    let root = slug(stem).trim_start_matches("n_").to_string();
    let mut out = Vec::new();
    for material in model.document.materials() {
        let Some(index) = material.index() else {
            continue;
        };
        let pbr = material.pbr_metallic_roughness();
        let mut maps: Vec<(&'static str, String)> = Vec::new();
        let mut keep =
            |slot: &'static str, texture: Option<gltf::texture::Texture<'_>>| -> Result<()> {
                if let Some(texture) = texture {
                    maps.push((slot, images.file_for(model, &texture, side, slot)?));
                }
                Ok(())
            };
        keep(SLOTS[0], pbr.base_color_texture().map(|i| i.texture()))?;
        keep(SLOTS[1], material.normal_texture().map(|n| n.texture()))?;
        keep(
            SLOTS[2],
            pbr.metallic_roughness_texture().map(|i| i.texture()),
        )?;
        keep(SLOTS[3], material.occlusion_texture().map(|o| o.texture()))?;
        keep(SLOTS[4], material.emissive_texture().map(|i| i.texture()))?;
        let emissive = material.emissive_factor();
        let strength = material.emissive_strength().unwrap_or(1.0);
        let volume = material.volume();
        let attenuation_color = volume
            .as_ref()
            .map_or([1.0, 1.0, 1.0], gltf::material::Volume::attenuation_color);
        out.push(Surface {
            id: format!("{root}_{}", slug(&names[index]).trim_start_matches("n_")),
            part: names[index].clone(),
            base_color: pbr.base_color_factor(),
            emissive: [
                emissive[0] * strength,
                emissive[1] * strength,
                emissive[2] * strength,
                1.0,
            ],
            metallic: pbr.metallic_factor(),
            roughness: pbr.roughness_factor(),
            reflectance: reflectance_of(
                material.ior().unwrap_or(1.5),
                material.specular().map_or(1.0, |s| s.specular_factor()),
            ),
            // Only `MASK` cuts; `OPAQUE` and `BLEND` keep every fragment, and
            // the surface's own alpha is what decides how `BLEND` reads.
            alpha_cutoff: match material.alpha_mode() {
                gltf::material::AlphaMode::Mask => material.alpha_cutoff().unwrap_or(0.5),
                _ => 0.0,
            },
            transmission: material
                .transmission()
                .map_or(0.0, |t| t.transmission_factor()),
            ior: material.ior().unwrap_or(1.5),
            thickness: volume
                .as_ref()
                .map_or(0.0, gltf::material::Volume::thickness_factor),
            attenuation_color,
            attenuation_distance: volume
                .as_ref()
                .map_or(0.0, gltf::material::Volume::attenuation_distance),
            double_sided: material.double_sided(),
            maps,
        });
    }
    Ok(out)
}

impl Surface {
    /// This material as an `[[assets]]` entry the scene declares.
    pub(crate) fn asset(&self) -> toml::Value {
        let mut entry = toml::map::Map::new();
        entry.insert("id".into(), toml::Value::String(self.id.clone()));
        entry.insert("type".into(), toml::Value::String("material".into()));
        entry.insert(
            "shader".into(),
            toml::Value::String(MATERIAL_SHADER_PATH.into()),
        );
        // A map the file did not name is a feature the shader turns off, so
        // the factor stands where glTF says a missing map is one.
        let mut features = toml::map::Map::new();
        for slot in [SLOTS[2], SLOTS[4]] {
            let bound = self.maps.iter().any(|(name, _)| *name == slot);
            features.insert(format!("{slot}_map"), toml::Value::Boolean(bound));
        }
        entry.insert("features".into(), toml::Value::Table(features));
        let mut params = toml::map::Map::new();
        params.insert("base_color".into(), floats(self.base_color));
        params.insert("emissive".into(), floats(self.emissive));
        params.insert(
            "attenuation".into(),
            floats([
                self.attenuation_color[0],
                self.attenuation_color[1],
                self.attenuation_color[2],
                self.attenuation_distance,
            ]),
        );
        params.insert("metallic".into(), toml::Value::Float(self.metallic.into()));
        params.insert(
            "roughness".into(),
            toml::Value::Float(self.roughness.into()),
        );
        params.insert(
            "reflectance".into(),
            toml::Value::Float(self.reflectance.into()),
        );
        params.insert(
            "alpha_cutoff".into(),
            toml::Value::Float(self.alpha_cutoff.into()),
        );
        params.insert(
            "transmission".into(),
            toml::Value::Float(self.transmission.into()),
        );
        params.insert("ior".into(), toml::Value::Float(self.ior.into()));
        params.insert(
            "thickness".into(),
            toml::Value::Float(self.thickness.into()),
        );
        for (slot, file) in &self.maps {
            params.insert(
                (*slot).into(),
                toml::Value::String(format!("models/{file}")),
            );
        }
        entry.insert("params".into(), toml::Value::Table(params));
        // What decides which pass the node joins and how it rasterizes. The
        // shader reads the same numbers through its own `Params`, so a scene
        // editing one half has to edit the other.
        let mut surface = toml::map::Map::new();
        surface.insert(
            "alpha".into(),
            toml::Value::String(
                if self.alpha_cutoff > 0.0 {
                    "mask"
                } else {
                    "opaque"
                }
                .into(),
            ),
        );
        surface.insert(
            "alpha_cutoff".into(),
            toml::Value::Float(self.alpha_cutoff.into()),
        );
        surface.insert(
            "double_sided".into(),
            toml::Value::Boolean(self.double_sided),
        );
        surface.insert(
            "transmission".into(),
            toml::Value::Float(self.transmission.into()),
        );
        surface.insert("ior".into(), toml::Value::Float(self.ior.into()));
        surface.insert(
            "thickness".into(),
            toml::Value::Float(self.thickness.into()),
        );
        surface.insert(
            "attenuation_distance".into(),
            toml::Value::Float(self.attenuation_distance.into()),
        );
        entry.insert("surface".into(), toml::Value::Table(surface));
        toml::Value::Table(entry)
    }
}

//! The `material` asset: a shader, the `@if` features that pick its variant,
//! and the values its uniforms take.
//!
//! A material is data; the shader it names is source. Which values a shader
//! takes is the shader's own business, so the fields are read off its
//! `Params` struct once it is linked rather than declared a second time here
//! — a material that sets a value the shader does not read says so, and a
//! shader that grows a field needs no edit anywhere else.

use anyhow::{Result, anyhow, bail};
use balaur_core::Engine;
use balaur_core::hecs::Entity;
use balaur_plugin::Registry;

pub use crate::material_compile::{
    Compiled, Field, FieldType, compile, compile_with, fields, pack,
};

pub(crate) use crate::material_check::{install_material_check, install_material_params};

/// The asset type name, and what an `asset`-typed property asks for.
pub const MATERIAL_ASSET_TYPE: &str = "material";

/// A file a material names, as its own project would find it. A material
/// handed in by absolute path — the editor mirrors a game's files that way —
/// names its shader and textures relative to that game, not this engine's
/// root, so the path is joined under the root that owns the material.
#[must_use]
pub fn project_path(eng: &Engine, reference: &str, path: &str) -> Option<String> {
    let material = std::path::Path::new(reference);
    if !balaur_core::files::rooted(material) {
        return None;
    }
    let owner = balaur_core::document_paths::owner_of(eng, material)?;
    Some(owner.join(path).to_string_lossy().into_owned())
}

/// The shader a material names, read against the material's own project.
pub(crate) fn shader_text(eng: &Engine, reference: &str, shader: &str) -> Result<String> {
    if let Some(full) = project_path(eng, reference, shader)
        && let Ok(bytes) = balaur_core::files::backend(eng).read(std::path::Path::new(&full))
    {
        return Ok(String::from_utf8(bytes)?);
    }
    balaur_core::project::scene_text(eng, shader)
}

/// Which bind group a material's own uniform takes. Groups 0, 1 and 2 are
/// the frame, the object and its texture, as every Balaur material lays them
/// out.
pub const PARAMS_GROUP: u32 = 3;

/// One value a material sets, in the shape its `[params]` table wrote it.
#[derive(Clone, Debug, PartialEq)]
pub enum Param {
    Float(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    /// An image bound to one of [`TEXTURE_SLOTS`], rather than a number in
    /// the uniform block. A param named for a slot and given a file path.
    Texture(String),
}

impl Param {
    /// How the value would be spelled in WGSL, for an error that has to name
    /// both sides of a mismatch.
    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            Param::Float(_) => "f32",
            Param::Vec2(_) => "vec2<f32>",
            Param::Vec3(_) => "vec3<f32>",
            Param::Vec4(_) => "vec4<f32>",
            Param::Texture(_) => "texture_2d<f32>",
        }
    }

    pub(crate) fn floats(&self) -> &[f32] {
        match self {
            Param::Float(v) => std::slice::from_ref(v),
            Param::Vec2(v) => v,
            Param::Vec3(v) => v,
            Param::Vec4(v) => v,
            // Not a number in the block: a texture is bound, not uploaded.
            Param::Texture(_) => &[],
        }
    }
}

/// The texture slots the 3D contract declares, in binding order. A `[params]`
/// key named for one and given a file path binds that slot; every slot a
/// material leaves out gets a one-pixel fallback, so a shader never branches
/// on absence.
pub const TEXTURE_SLOTS: &[&str] = &[
    "albedo",
    "normal",
    "metallic_roughness",
    "occlusion",
    "emissive",
    "height",
];

/// The images a 2D material binds beside the node's own, in binding order:
/// `texture_1` to `texture_4` in `package::sprite`.
pub const SPRITE_TEXTURE_SLOTS: &[&str] = &["texture_1", "texture_2", "texture_3", "texture_4"];

/// Whether `name` is one of [`TEXTURE_SLOTS`] or [`SPRITE_TEXTURE_SLOTS`].
#[must_use]
pub fn is_texture_slot(name: &str) -> bool {
    TEXTURE_SLOTS.contains(&name) || SPRITE_TEXTURE_SLOTS.contains(&name)
}

/// Image extensions a `[params]` string is read as a texture path for. A
/// colour is `#rrggbb`, and a slot name with anything else is an error.
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "tga", "hdr", "exr"];

fn names_an_image(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    IMAGE_EXTENSIONS
        .iter()
        .any(|extension| lower.ends_with(&format!(".{extension}")))
}

/// A parsed `material` asset.
#[derive(Clone, Debug, Default)]
pub struct Material3d {
    /// Project-relative path to the WESL shader this material draws with.
    pub shader: String,
    /// `@if` flags, in the order written; chosen when the shader is linked.
    pub features: Vec<(String, bool)>,
    /// Values for the shader's `Params` fields, by name.
    pub params: Vec<(String, Param)>,
    /// How a node drawing this rasterizes, from the `[surface]` table.
    pub surface: Surface,
}

/// The feature a material names to be handed a colour per vertex.
pub const VERTEX_COLOR: &str = "vertex_color";

impl Material3d {
    /// The image each texture slot is bound to, in [`TEXTURE_SLOTS`] order;
    /// `None` for a slot this material left out.
    #[must_use]
    pub fn textures(&self) -> Vec<Option<&str>> {
        self.bound(TEXTURE_SLOTS)
    }

    /// The image each of [`SPRITE_TEXTURE_SLOTS`] is bound to, in order.
    #[must_use]
    pub fn sprite_textures(&self) -> Vec<Option<&str>> {
        self.bound(SPRITE_TEXTURE_SLOTS)
    }

    fn bound(&self, slots: &[&str]) -> Vec<Option<&str>> {
        slots
            .iter()
            .map(|slot| {
                self.params.iter().find_map(|(name, param)| match param {
                    Param::Texture(path) if name == slot => Some(path.as_str()),
                    _ => None,
                })
            })
            .collect()
    }

    /// Whether `features` asks for the last frame as `screen_texture`.
    #[must_use]
    pub fn reads_screen(&self) -> bool {
        self.features
            .iter()
            .any(|(name, on)| name == "screen" && *on)
    }

    /// Whether `features` asks for a colour per vertex. Only a material that
    /// does gets the attribute, so nothing else pays for the buffer.
    #[must_use]
    pub fn reads_vertex_color(&self) -> bool {
        self.features
            .iter()
            .any(|(name, on)| name == VERTEX_COLOR && *on)
    }
}

/// What a definition table holds, for the generated reference.
pub(crate) const MATERIAL_ASSET_DOC: &str = r##"A shader and its values. `shader` names a `.wesl` file, `[features]` sets its `@if` flags, `[params]` fills its `Params` struct by field name.

```toml
[[assets]]
id = "water"
type = "material"
shader = "shaders/water.wesl"
features = { lit = true }
# a number is an f32, [x, y] a vec2, [x, y, z] a vec3, [x, y, z, w] or "#rrggbb"/"#rrggbbaa" a vec4
params = { speed = 0.4, tint = "#3aa0ff" }

# How a node drawing it rasterizes, rather than what colour it comes out.
[surface]
alpha = "blend"              # opaque, mask (a cutout), or blend
alpha_cutoff = 0.5           # what a mask drops a fragment below
double_sided = true
transmission = 0.9           # above zero is glass: it refracts the scene behind it
ior = 1.5                    # how sharply it bends light
thickness = 0.2              # how far light travels inside it, in world units
attenuation_color = "#dff0ea"
attenuation_distance = 2.0
mirror = true                # show the scene reflected in this surface's own plane
mirror_intensity = 1.0
mirror_falloff = 0.0         # above zero fades the reflection as the surface turns away
mirror_normal = [0.0, 1.0, 0.0]   # which way the plane faces in the node's own space
```"##;

/// `#rrggbb` or `#rrggbbaa` as four channels in 0..=1.
fn hex_rgba(text: &str) -> Option<[f32; 4]> {
    let hex = text.strip_prefix('#')?;
    let channel = |i: usize| {
        u8::from_str_radix(hex.get(i..i + 2)?, 16)
            .ok()
            .map(|b| f32::from(b) / 255.0)
    };
    match hex.len() {
        6 => Some([channel(0)?, channel(2)?, channel(4)?, 1.0]),
        8 => Some([channel(0)?, channel(2)?, channel(4)?, channel(6)?]),
        _ => None,
    }
}

fn parse_param(name: &str, value: &toml::Value) -> Result<Param> {
    if let Some(text) = value.as_str() {
        if names_an_image(text) {
            if !is_texture_slot(name) {
                bail!(
                    "param `{name}`: an image binds a texture slot, and the slots are {} in \
                     3D and {} in 2D",
                    TEXTURE_SLOTS.join(", "),
                    SPRITE_TEXTURE_SLOTS.join(", ")
                );
            }
            return Ok(Param::Texture(text.to_string()));
        }
        return hex_rgba(text)
            .map(Param::Vec4)
            .ok_or_else(|| anyhow!("param `{name}`: `{text}` is not #rrggbb or #rrggbbaa"));
    }
    if let Some(number) = balaur_core::components::as_f64(value) {
        return Ok(Param::Float(number as f32));
    }
    let array = value
        .as_array()
        .ok_or_else(|| anyhow!("param `{name}`: expected a number, an array or a colour string"))?;
    let numbers: Option<Vec<f32>> = array
        .iter()
        .map(|v| balaur_core::components::as_f64(v).map(|n| n as f32))
        .collect();
    let numbers =
        numbers.ok_or_else(|| anyhow!("param `{name}`: every element must be a number"))?;
    match numbers[..] {
        [x, y] => Ok(Param::Vec2([x, y])),
        [x, y, z] => Ok(Param::Vec3([x, y, z])),
        [x, y, z, w] => Ok(Param::Vec4([x, y, z, w])),
        _ => bail!(
            "param `{name}`: an array is two, three or four numbers, not {}",
            numbers.len()
        ),
    }
}

/// How a surface's alpha is read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlphaMode {
    /// Alpha is ignored and the surface is solid.
    #[default]
    Opaque,
    /// A fragment fainter than `alpha_cutoff` is dropped rather than drawn:
    /// a leaf's outline, cut from the rectangle it was painted on.
    Mask,
    /// The surface is drawn over what is behind it, in the pass that resolves
    /// overlapping translucent surfaces without sorting them.
    Blend,
}

/// What a material says about how its node draws, rather than what colour it
/// comes out.
///
/// These decide which pass a node joins and how it is rasterized, so the
/// backend reads them off the material and sets them on the node, while the
/// shader reads the same numbers through its own `Params`. Everything here
/// defaults to the surface a material that says nothing already had.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub alpha: AlphaMode,
    /// The alpha a `mask` surface drops a fragment below.
    pub alpha_cutoff: f32,
    /// Whether the back of a triangle draws as well as the front.
    pub double_sided: bool,
    /// How much of the scene behind this surface comes through it. Above zero
    /// makes it glass: it draws after the opaque scene is resolved, refracting
    /// what that pass left.
    pub transmission: f32,
    /// How sharply light bends entering it. 1.0 does not bend at all; window
    /// glass is 1.5, water 1.33, diamond 2.42.
    pub ior: f32,
    /// How far light travels inside it, in world units. Zero refracts without
    /// tinting, which is what a single pane wants.
    pub thickness: f32,
    /// What is left of white light after `attenuation_distance` inside it.
    pub attenuation_color: [f32; 4],
    /// Zero takes nothing out however thick the glass.
    pub attenuation_distance: f32,
    /// Whether this surface shows the scene reflected in its own plane. The
    /// reflection is rendered from a mirrored camera, so it is sharp where a
    /// probe or the sky would only be approximate.
    pub mirror: bool,
    /// How much of the reflection shows, from nothing to all of it.
    pub mirror_intensity: f32,
    /// How fast the reflection fades as the surface turns away from its own
    /// plane. Zero keeps it even, which is what a flat mirror wants; above
    /// zero keeps a curved one reflecting only on the face that looks along
    /// the plane's normal.
    pub mirror_falloff: f32,
    /// Which way the mirror's plane faces in the node's own space. A Balaur
    /// `plane` lies in xz, so its face looks up.
    pub mirror_normal: [f32; 3],
}

impl Default for Surface {
    fn default() -> Self {
        Self {
            alpha: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
            transmission: 0.0,
            ior: 1.5,
            thickness: 0.0,
            attenuation_color: [1.0, 1.0, 1.0, 1.0],
            attenuation_distance: 0.0,
            mirror: false,
            mirror_intensity: 1.0,
            mirror_falloff: 0.0,
            mirror_normal: [0.0, 1.0, 0.0],
        }
    }
}

impl Surface {
    /// Whether a node drawing this joins the refraction pass rather than the
    /// opaque one.
    #[must_use]
    pub const fn refracts(&self) -> bool {
        self.transmission > 0.0
    }
}

/// Every key a `[surface]` table may set, for the error a typo gets. A
/// `[params]` key the shader does not read is refused, and this is the one
/// table that would otherwise swallow one.
const SURFACE_KEYS: &[&str] = &[
    "alpha",
    "alpha_cutoff",
    "double_sided",
    "transmission",
    "ior",
    "thickness",
    "attenuation_color",
    "attenuation_distance",
    "mirror",
    "mirror_intensity",
    "mirror_falloff",
    "mirror_normal",
];

/// Read a material's `[surface]` table.
fn parse_surface(value: &toml::Value) -> Result<Surface> {
    let base = Surface::default();
    let Some(table) = value.get("surface") else {
        return Ok(base);
    };
    if let Some(table) = table.as_table() {
        for name in table.keys() {
            if !SURFACE_KEYS.contains(&name.as_str()) {
                bail!(
                    "a material's `[surface]` has no `{name}`; it takes {}",
                    SURFACE_KEYS.join(", ")
                );
            }
        }
    }
    let num = |key: &str, default: f32| {
        table
            .get(key)
            .and_then(balaur_core::components::as_f64)
            .unwrap_or(f64::from(default)) as f32
    };
    Ok(Surface {
        alpha: match table
            .get("alpha")
            .and_then(toml::Value::as_str)
            .unwrap_or(crate::vocabulary::words::OPAQUE)
        {
            crate::vocabulary::words::OPAQUE => AlphaMode::Opaque,
            crate::vocabulary::words::MASK => AlphaMode::Mask,
            crate::vocabulary::words::BLEND => AlphaMode::Blend,
            other => bail!(
                "a material's `surface.alpha` is {}, not '{other}'",
                crate::vocabulary::words::ALPHA_MODES.join(", ")
            ),
        },
        alpha_cutoff: num("alpha_cutoff", base.alpha_cutoff).clamp(0.0, 1.0),
        double_sided: table
            .get("double_sided")
            .and_then(toml::Value::as_bool)
            .unwrap_or(base.double_sided),
        transmission: num("transmission", 0.0).clamp(0.0, 1.0),
        ior: num("ior", base.ior).max(1.0),
        thickness: num("thickness", 0.0).max(0.0),
        attenuation_color: table
            .get("attenuation_color")
            .and_then(toml::Value::as_str)
            .and_then(hex_rgba)
            .unwrap_or(base.attenuation_color),
        attenuation_distance: num("attenuation_distance", 0.0).max(0.0),
        mirror: table
            .get(crate::vocabulary::keys::MIRROR)
            .and_then(toml::Value::as_bool)
            .unwrap_or(base.mirror),
        mirror_intensity: num("mirror_intensity", base.mirror_intensity).clamp(0.0, 1.0),
        mirror_falloff: num("mirror_falloff", 0.0).max(0.0),
        mirror_normal: {
            let axis = |i: usize, default: f32| {
                table
                    .get("mirror_normal")
                    .and_then(toml::Value::as_array)
                    .and_then(|a| a.get(i))
                    .and_then(balaur_core::components::as_f64)
                    .unwrap_or(f64::from(default)) as f32
            };
            let normal = base.mirror_normal;
            [axis(0, normal[0]), axis(1, normal[1]), axis(2, normal[2])]
        },
    })
}

/// Parse a `material` definition table.
pub fn parse(value: &toml::Value) -> Result<Material3d> {
    let shader = value
        .get("shader")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| anyhow!("a material names its shader: `shader = \"shaders/x.wesl\"`"))?
        .to_string();
    let mut features = Vec::new();
    if let Some(table) = value.get("features").and_then(toml::Value::as_table) {
        for (name, on) in table {
            let on = on
                .as_bool()
                .ok_or_else(|| anyhow!("feature `{name}` is on or off, not `{on}`"))?;
            features.push((name.clone(), on));
        }
    }
    let mut params = Vec::new();
    if let Some(table) = value.get("params").and_then(toml::Value::as_table) {
        for (name, value) in table {
            params.push((name.clone(), parse_param(name, value)?));
        }
    }
    Ok(Material3d {
        shader,
        features,
        params,
        surface: parse_surface(value)?,
    })
}

/// The `[surface]` a material reference declares, or the default for a node
/// that names none or whose material does not load.
///
/// Read every frame rather than cached on the node: the asset itself is
/// cached, and a material a reload changed must reach the node it is on.
#[must_use]
pub fn surface_of(eng: &Engine, reference: &str) -> Surface {
    if reference.is_empty() {
        return Surface::default();
    }
    balaur_core::assets::load_typed::<Material3d>(eng, reference)
        .map_or_else(|_| Surface::default(), |material| material.surface)
}

/// Point `entity` at the `material` asset it draws with; empty is the
/// built-in material.
///
/// A change bumps `version`, which rebuilds the backend's node: a material
/// owns its pipeline, so it cannot be swapped onto a node already built
/// against a different one.
pub(crate) fn set_material_2d(eng: &Engine, entity: Entity, reference: &str) -> Result<()> {
    let world = eng.world_mut();
    let mut renderable = world
        .get::<&mut crate::Renderable2d>(entity)
        .map_err(|_| anyhow!("node has no 2D shape yet"))?;
    if renderable.material != reference {
        renderable.material = reference.to_string();
        renderable.version += 1;
    }
    Ok(())
}

/// Point `entity` at the `material` asset its 3D shape draws with.
///
/// The 3D counterpart of [`set_material_2d`]; a change rebuilds the node for
/// the same reason.
pub(crate) fn set_material_3d(eng: &Engine, entity: Entity, reference: &str) -> Result<()> {
    let world = eng.world_mut();
    let mut renderable = world
        .get::<&mut crate::Renderable3d>(entity)
        .map_err(|_| anyhow!("node has no 3D shape yet"))?;
    if renderable.material != reference {
        renderable.material = reference.to_string();
        renderable.version += 1;
    }
    Ok(())
}

/// The component that names a material for a node and its subtree.
pub const MATERIAL_COMPONENT: &str = "material";

/// Marks a node the `material` component was put on. The reference itself is
/// `Appearance::material`, where the tree composes it.
pub(crate) struct NodeMaterial;

/// The `material` component: any node, shape or none, naming the material it
/// and every descendant draw with. A renderable's own `material` wins over it
/// for that renderable alone.
pub(crate) fn register_material_component(reg: &mut Registry<'_>) {
    use balaur_core::components::ComponentDef;
    use balaur_core::scene::{Appearance, MaterialId};
    reg.register_component(
        MATERIAL_COMPONENT,
        ComponentDef {
            doc: "`source` is the `material` asset this node and everything under it draw with. A renderable's own `material` property overrides it for that node alone.",
            schema: ComponentDef::parse_schema(
                MATERIAL_COMPONENT,
                &ComponentDef::schema(&[(
                    crate::vocabulary::keys::SOURCE,
                    &format!(
                        r#"{{ type = "asset", asset = "{MATERIAL_ASSET_TYPE}", default = "", description = "The material asset; empty takes the parent's" }}"#
                    ),
                )]),
            ),
            tags: &[balaur_core::components::tag::RENDER],
            expects: &[],
            apply: Box::new(|eng, entity, params| {
                let reference = params
                    .get(crate::vocabulary::keys::SOURCE)
                    .and_then(toml::Value::as_str)
                    .unwrap_or_default();
                let mut world = eng.world_mut();
                world
                    .get::<&mut Appearance>(entity)
                    .map_err(|_| anyhow!("node is dead"))?
                    .material = MaterialId::intern(reference);
                world
                    .insert_one(entity, NodeMaterial)
                    .map_err(|_| anyhow!("node is dead"))
            }),
            remove: Box::new(|eng, entity| {
                let mut world = eng.world_mut();
                let _ = world.remove_one::<NodeMaterial>(entity);
                if let Ok(mut appearance) = world.get::<&mut Appearance>(entity) {
                    appearance.material = MaterialId::NONE;
                }
                Ok(())
            }),
            // Present when put on, or when a script named a material for the
            // node: either way the node carries one.
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let material = world.get::<&Appearance>(entity).ok()?.material;
                if material.is_none() && world.get::<&NodeMaterial>(entity).is_err() {
                    return None;
                }
                let mut map = toml::map::Map::new();
                map.insert(
                    crate::vocabulary::keys::SOURCE.into(),
                    toml::Value::String(material.reference().to_string()),
                );
                Some(toml::Value::Table(map))
            }),
        },
    );
}

/// The `material` asset type: files live in `materials/`.
pub(crate) fn register_material_asset(reg: &mut Registry<'_>) {
    reg.register_asset_type(
        MATERIAL_ASSET_TYPE,
        "materials",
        MATERIAL_ASSET_DOC,
        |value| Ok(std::rc::Rc::new(parse(value)?) as std::rc::Rc<dyn std::any::Any>),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shaders;

    fn table(text: &str) -> toml::Value {
        toml::from_str(text).unwrap()
    }

    fn params_of(shader: &str) -> Vec<Field> {
        let linked = shaders::link(&[("package::m", shader)], "package::m", &[]).unwrap();
        fields(&linked.syntax).unwrap()
    }

    #[test]
    fn a_material_names_its_shader() {
        let m = parse(&table("shader = \"shaders/water.wesl\"")).unwrap();
        assert_eq!(m.shader, "shaders/water.wesl");
        assert!(m.params.is_empty());
    }

    #[test]
    fn a_material_without_a_shader_is_an_error() {
        let err = parse(&table("params = { speed = 1.0 }")).unwrap_err();
        assert!(format!("{err}").contains("names its shader"), "{err}");
    }

    #[test]
    fn params_take_numbers_arrays_and_colours() {
        let m = parse(&table(
            r##"
            shader = "s.wesl"
            [params]
            speed = 0.5
            offset = [1.0, 2.0]
            tint = "#ff8000"
            "##,
        ))
        .unwrap();
        let by_name = |n: &str| m.params.iter().find(|(k, _)| k == n).unwrap().1.clone();
        assert_eq!(by_name("speed"), Param::Float(0.5));
        assert_eq!(by_name("offset"), Param::Vec2([1.0, 2.0]));
        assert_eq!(by_name("tint"), Param::Vec4([1.0, 128.0 / 255.0, 0.0, 1.0]));
    }

    #[test]
    fn a_colour_that_is_not_hex_names_the_param() {
        let err = parse(&table("shader = \"s.wesl\"\nparams = { tint = \"blue\" }")).unwrap_err();
        assert!(format!("{err}").contains("tint"), "{err}");
    }

    #[test]
    fn features_are_read_in_the_order_written() {
        let m = parse(&table(
            "shader = \"s.wesl\"\nfeatures = { lit = true, fog = false }",
        ))
        .unwrap();
        assert_eq!(
            m.features,
            vec![("fog".to_string(), false), ("lit".to_string(), true)]
        );
    }

    const WITH_PARAMS: &str = r"
struct Params { speed: f32, tint: vec4<f32> }
@group(3) @binding(0) var<uniform> params: Params;
@fragment fn fs_main() -> @location(0) vec4<f32> {
    return params.tint * params.speed;
}
";

    #[test]
    fn fields_come_off_the_shaders_own_struct() {
        assert_eq!(
            params_of(WITH_PARAMS),
            vec![
                Field {
                    name: "speed".into(),
                    ty: FieldType::F32,
                    offset: 0
                },
                Field {
                    name: "tint".into(),
                    ty: FieldType::Vec4,
                    offset: 16
                },
            ]
        );
    }

    #[test]
    fn a_shader_with_no_params_has_no_fields() {
        let shader = "@fragment fn fs_main() -> @location(0) vec4<f32> {
            return vec4<f32>(1.0);
        }";
        assert!(params_of(shader).is_empty());
    }

    #[test]
    fn a_vec3_pads_the_field_after_it_to_sixteen() {
        let shader = r"
struct Params { a: vec3<f32>, b: f32, c: vec2<f32> }
@group(3) @binding(0) var<uniform> params: Params;
@fragment fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(params.a, params.b) + vec4<f32>(params.c, 0.0, 0.0);
}
";
        let offsets: Vec<usize> = params_of(shader).iter().map(|f| f.offset).collect();
        assert_eq!(offsets, vec![0, 12, 16]);
    }

    #[test]
    fn packing_writes_each_value_at_its_own_offset() {
        let fields = params_of(WITH_PARAMS);
        let params = vec![
            ("speed".to_string(), Param::Float(2.0)),
            ("tint".to_string(), Param::Vec4([0.25, 0.5, 0.75, 1.0])),
        ];
        let bytes = pack(&fields, &params).unwrap();
        assert_eq!(bytes.len(), 32);
        // Bits, not values: what is asserted is a byte-exact round-trip.
        let at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
        assert_eq!(at(0), 2.0f32.to_bits());
        assert_eq!(
            (at(16), at(20), at(24), at(28)),
            (
                0.25f32.to_bits(),
                0.5f32.to_bits(),
                0.75f32.to_bits(),
                1.0f32.to_bits()
            )
        );
    }

    #[test]
    fn a_field_no_param_names_keeps_its_zero() {
        let fields = params_of(WITH_PARAMS);
        let bytes = pack(&fields, &[("speed".to_string(), Param::Float(3.0))]).unwrap();
        assert_eq!(
            u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
            0.0f32.to_bits()
        );
    }

    #[test]
    fn a_param_of_the_wrong_type_names_both_sides() {
        let fields = params_of(WITH_PARAMS);
        let err = pack(&fields, &[("tint".to_string(), Param::Float(1.0))]).unwrap_err();
        let text = format!("{err}");
        assert!(
            text.contains("tint") && text.contains("vec4<f32>"),
            "{text}"
        );
    }

    const WITH_VARIANT: &str = r"
struct Params { speed: f32, tint: vec4<f32> }
@group(3) @binding(0) var<uniform> params: Params;
@if(lit) fn boost() -> f32 { return 2.0; }
@if(!lit) fn boost() -> f32 { return 1.0; }
@fragment fn fs_main() -> @location(0) vec4<f32> {
    return params.tint * params.speed * boost();
}
";

    #[test]
    fn compiling_links_the_shader_and_packs_the_values() {
        let material = parse(&table(
            "shader = \"s.wesl\"\nparams = { speed = 2.0, tint = [0.25, 0.5, 0.75, 1.0] }",
        ))
        .unwrap();
        let compiled = compile(&material, WITH_PARAMS).unwrap();
        assert!(compiled.wgsl.contains("fn fs_main"), "{}", compiled.wgsl);
        assert_eq!(compiled.params.len(), 32);
        let at = |o: usize| f32::from_le_bytes(compiled.params[o..o + 4].try_into().unwrap());
        assert_eq!((at(0), at(16), at(28)), (2.0, 0.25, 1.0));
    }

    #[test]
    fn the_screen_feature_binds_the_last_frame_and_nothing_else_does() {
        const READS: &str = r"
import package::sprite::{VertexInput, VertexOutput, vertex, screen_uv, sample_screen};
@vertex fn vs_main(in: VertexInput) -> VertexOutput { return vertex(in); }
@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return sample_screen(screen_uv(in.clip_position));
}
";
        let on = parse(&table("shader = \"s.wesl\"\nfeatures = { screen = true }")).unwrap();
        assert!(on.reads_screen());
        let wgsl = compile(&on, READS).unwrap().wgsl;
        assert!(wgsl.contains("screen_texture"), "{wgsl}");
        let off = parse(&table("shader = \"s.wesl\"")).unwrap();
        assert!(!off.reads_screen());
        assert!(
            compile(&off, READS).is_err(),
            "without the feature there is nothing to sample"
        );
    }

    #[test]
    fn a_feature_picks_which_variant_is_linked() {
        let on = parse(&table("shader = \"s.wesl\"\nfeatures = { lit = true }")).unwrap();
        let off = parse(&table("shader = \"s.wesl\"\nfeatures = { lit = false }")).unwrap();
        assert!(
            compile(&on, WITH_VARIANT)
                .unwrap()
                .wgsl
                .contains("return 2f")
        );
        assert!(
            compile(&off, WITH_VARIANT)
                .unwrap()
                .wgsl
                .contains("return 1f")
        );
    }

    #[test]
    fn a_shader_that_does_not_link_names_the_file_the_material_pointed_at() {
        let material = parse(&table("shader = \"shaders/broken.wesl\"")).unwrap();
        let err = compile(&material, "fn broken(").err().unwrap();
        assert!(
            format!("{err:#}").contains("shaders/broken.wesl"),
            "{err:#}"
        );
    }

    /// What a project writes: the contract module carries everything but the
    /// two entry points and the material's own values.
    const PROJECT_SHADER: &str = r"
import package::sprite::{VertexInput, VertexOutput, vertex, sample_albedo, tint, time};

struct Params { speed: f32, glow: vec4<f32> }
@group(3) @binding(0) var<uniform> params: Params;

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in);
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let pulse = 0.5 + 0.5 * sin(time() * params.speed);
    return sample_albedo(in.uv) * tint(in) + params.glow * pulse;
}
";

    #[test]
    fn a_link_error_points_at_the_file_and_line_the_author_wrote() {
        let material = parse(&table("shader = \"shaders/water.wesl\"")).unwrap();
        // The `;` is missing on line 5, so that is where WESL should point.
        let broken = r"
import package::sprite::{VertexInput, VertexOutput, vertex};

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in)
}
";
        let err = format!("{:#}", compile(&material, broken).err().unwrap());
        assert!(err.contains("shaders/water.wesl:6"), "{err}");
        assert!(!err.contains("package::material"), "{err}");
    }

    #[test]
    fn a_project_shader_links_against_the_sprite_contract() {
        let material = parse(&table(
            "shader = \"shaders/water.wesl\"\nparams = { speed = 3.0, glow = \"#204080\" }",
        ))
        .unwrap();
        let compiled = compile(&material, PROJECT_SHADER).unwrap();
        assert!(compiled.wgsl.contains("fn vs_main"), "{}", compiled.wgsl);
        assert!(compiled.wgsl.contains("fn fs_main"), "{}", compiled.wgsl);
        // The contract's helpers were pulled in, not left as imports.
        assert!(!compiled.wgsl.contains("import "), "{}", compiled.wgsl);
        assert!(compiled.wgsl.contains("textureSample"), "{}", compiled.wgsl);
        assert_eq!(
            compiled.fields,
            vec![
                Field {
                    name: "speed".into(),
                    ty: FieldType::F32,
                    offset: 0
                },
                Field {
                    name: "glow".into(),
                    ty: FieldType::Vec4,
                    offset: 16
                },
            ]
        );
        assert_eq!(compiled.params.len(), 32);
    }

    /// The 3D counterpart: lights and fog come from the contract too.
    const PROJECT_SHADER_3D: &str = r"
import package::mesh::{VertexInput, VertexOutput, vertex, shade, diffuse, sample_albedo, tint, apply_fog, time};

struct Params { pulse: f32 }
@group(3) @binding(0) var<uniform> params: Params;

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in);
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let base = shade(in);
    return base * (1.0 + params.pulse * sin(time()));
}
";

    #[test]
    fn a_project_shader_links_against_the_mesh_contract() {
        let material = parse(&table(
            "shader = \"shaders/rock.wesl\"\nparams = { pulse = 0.2 }",
        ))
        .unwrap();
        let compiled = compile(&material, PROJECT_SHADER_3D).unwrap();
        assert!(compiled.wgsl.contains("fn vs_main"), "{}", compiled.wgsl);
        assert!(!compiled.wgsl.contains("import "), "{}", compiled.wgsl);
        // The lighting loop came in with `shade`.
        assert!(compiled.wgsl.contains("ambient_count"), "{}", compiled.wgsl);
        assert_eq!(compiled.params.len(), 16);
    }

    /// The physically based module links against the mesh contract, with the
    /// texture slots and the BRDF it adds. A shader that imports it and calls
    /// `shade` is a whole material.
    #[test]
    fn the_pbr_module_links_over_the_mesh_contract() {
        let shader = r"
import package::mesh::{VertexInput, VertexOutput, vertex};
import package::pbr::{shade, default_surface, shade_pbr};

@vertex fn vs_main(in: VertexInput) -> VertexOutput { return vertex(in); }

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var s = default_surface(in);
    s.metallic = 1.0;
    s.roughness = 0.25;
    return shade_pbr(in, s);
}
";
        let material = Material3d {
            shader: "shaders/metal.wesl".into(),
            ..Default::default()
        };
        compile(&material, shader).expect("package::pbr links");
    }

    /// A `[params]` string that names an image binds a texture slot; one that
    /// names a slot with anything else is refused rather than read as a colour.
    #[test]
    fn an_image_param_binds_the_slot_it_is_named_for() {
        let value: toml::Value = toml::from_str(
            r##"
shader = "shaders/x.wesl"
[params]
albedo = "art/hero.png"
normal = "art/hero_n.png"
tint = "#ff8800"
"##,
        )
        .unwrap();
        let material = parse(&value).unwrap();
        assert_eq!(
            material.textures()[0],
            Some("art/hero.png"),
            "albedo is slot zero"
        );
        assert_eq!(material.textures()[1], Some("art/hero_n.png"));
        assert_eq!(material.textures()[2], None, "an unset slot stays unset");

        let bad: toml::Value =
            toml::from_str("shader = \"x.wesl\"\n[params]\nspeed = \"art/hero.png\"").unwrap();
        let err = parse(&bad).unwrap_err().to_string();
        assert!(err.contains("texture slot"), "{err}");
    }

    #[test]
    fn a_plugin_module_is_importable_by_a_project_shader() {
        let plugin = (
            "package::water".to_string(),
            "fn ripple(x: f32) -> f32 { return sin(x * 6.28318); }".to_string(),
        );
        let shader = r"
import package::water::ripple;
import package::sprite::{VertexInput, VertexOutput, vertex, sample_albedo};

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in);
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return sample_albedo(in.uv) * ripple(in.uv.x);
}
";
        let material = parse(&table("shader = \"shaders/w.wesl\"")).unwrap();
        let compiled = compile_with(&material, shader, std::slice::from_ref(&plugin)).unwrap();
        assert!(compiled.wgsl.contains("6.28318"), "{}", compiled.wgsl);
    }

    #[test]
    fn importing_a_module_nobody_registered_says_which() {
        let material = parse(&table("shader = \"shaders/w.wesl\"")).unwrap();
        let shader = "import package::water::ripple;
@fragment fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(ripple(1.0));
}";
        let err = compile(&material, shader).err().unwrap();
        assert!(format!("{err:#}").contains("water"), "{err:#}");
    }

    #[test]
    fn a_param_the_shader_does_not_read_is_dropped_not_fatal() {
        let fields = params_of(WITH_PARAMS);
        let bytes = pack(&fields, &[("nonesuch".to_string(), Param::Float(1.0))]).unwrap();
        assert_eq!(bytes.len(), 32);
    }
}

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
use balaur_script::{Bindings, BindingsExt};
use wesl::syntax::{GlobalDeclaration, TranslationUnit};

/// The asset type name, and what an `asset`-typed property asks for.
pub const MATERIAL_ASSET_TYPE: &str = "material";

/// The shader a material names, read against the material's own project. A
/// material handed in by absolute path — the editor mirrors a game's files
/// that way — names its shader relative to that game, not this engine's root.
pub(crate) fn shader_text(eng: &Engine, reference: &str, shader: &str) -> Result<String> {
    let material = std::path::Path::new(reference);
    if material.is_absolute() {
        let mut dir = material.parent();
        while let Some(d) = dir {
            if d.join("project.toml").exists() {
                let full = d.join(shader);
                if full.exists() {
                    return Ok(std::fs::read_to_string(full)?);
                }
                break;
            }
            dir = d.parent();
        }
    }
    balaur_core::project::scene_text(eng, shader)
}

/// The struct a shader declares to take a material's values.
const PARAMS_STRUCT: &str = "Params";

/// Which bind group a material's own uniform takes. Groups 0, 1 and 2 are
/// the frame, the object and its texture, as every Balaur material lays them
/// out.
pub const PARAMS_GROUP: u32 = 3;

/// A uniform buffer's size is a multiple of this, whatever the struct holds.
const UNIFORM_ALIGN: usize = 16;

/// One value a material sets, in the shape its `[params]` table wrote it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Param {
    Float(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
}

impl Param {
    /// How the value would be spelled in WGSL, for an error that has to name
    /// both sides of a mismatch.
    fn type_name(self) -> &'static str {
        match self {
            Param::Float(_) => "f32",
            Param::Vec2(_) => "vec2<f32>",
            Param::Vec3(_) => "vec3<f32>",
            Param::Vec4(_) => "vec4<f32>",
        }
    }

    fn floats(&self) -> &[f32] {
        match self {
            Param::Float(v) => std::slice::from_ref(v),
            Param::Vec2(v) => v,
            Param::Vec3(v) => v,
            Param::Vec4(v) => v,
        }
    }
}

/// A parsed `material` asset.
#[derive(Clone, Debug, Default)]
pub struct Material {
    /// Project-relative path to the WESL shader this material draws with.
    pub shader: String,
    /// `@if` flags, in the order written; chosen when the shader is linked.
    pub features: Vec<(String, bool)>,
    /// Values for the shader's `Params` fields, by name.
    pub params: Vec<(String, Param)>,
}

impl Material {
    /// Whether `features` asks for the last frame as `screen_texture`.
    #[must_use]
    pub fn reads_screen(&self) -> bool {
        self.features
            .iter()
            .any(|(name, on)| name == "screen" && *on)
    }
}

/// What a definition table holds, for the generated reference.
pub(crate) const MATERIAL_ASSET_DOC: &str = r##"A shader and the values it draws with. `shader` names a `.wesl` file
(project-relative); `[features]` are the `@if` flags that pick a variant when
it is linked; `[params]` are the values of the shader's `Params` struct, by
field name. A number is an `f32`, an array of two, three or four numbers a
`vec2`/`vec3`/`vec4`, and a `#rrggbb` or `#rrggbbaa` string a `vec4`.

```toml
[[assets]]
id = "water"
type = "material"
shader = "shaders/water.wesl"
features = { lit = true }
params = { speed = 0.4, tint = "#3aa0ff" }
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

/// Parse a `material` definition table.
pub fn parse(value: &toml::Value) -> Result<Material> {
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
    Ok(Material {
        shader,
        features,
        params,
    })
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
        .get::<&mut crate::Renderable>(entity)
        .map_err(|_| anyhow!("node has no 3D shape yet"))?;
    if renderable.material != reference {
        renderable.material = reference.to_string();
        renderable.version += 1;
    }
    Ok(())
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

/// `render::check_material(path)` — what is wrong with a material asset, as
/// `[#{ file, line, column, severity, message }]`, empty when it links.
///
/// Linking is CPU work with no GPU in it, so a check runs in a headless
/// editor and in CI. The asset layer deliberately parses a material without
/// linking it — a scene must load on a machine that cannot draw — which is
/// why a broken shader needs asking about rather than waiting for.
pub(crate) fn install_material_check(m: &mut dyn Bindings<balaur_core::Engine>) {
    m.describe(&[(
        "check_material",
        &[],
        "", "Every diagnostic about the material at that path, as `[#{ file, line, column, severity, message }]`; empty when it links.",
    )]);
    m.function(
        "check_material",
        |eng: &balaur_core::Engine, path: String| {
            Ok(balaur_script::Value::List(
                match check_material(eng, &path) {
                    Ok(()) => Vec::new(),
                    Err(why) => vec![finding(&path, &format!("{why:#}"))],
                },
            ))
        },
    );
}

/// `render::material_params(path)` — the values a material's shader takes, as
/// `[#{ name, type, value }]` in the order the uniform lays them out.
///
/// `type` is the vocabulary a component schema uses (`float`, `vec2`, `vec3`,
/// `color`), so an inspector draws these with the editors it already has.
/// Reading the fields off the linked shader is what keeps the rows and the
/// shader in step; the material only says what the values are.
///
/// A material that will not link has no rows. What is wrong with it is
/// `check_material`'s answer, not this one's.
pub(crate) fn install_material_params(m: &mut dyn Bindings<balaur_core::Engine>) {
    m.describe(&[(
        "material_params",
        &[],
        "", "The material's editable rows, one `#{ name, type, value }` per field its linked shader declares; empty when it will not link.",
    )]);
    m.function(
        "material_params",
        |eng: &balaur_core::Engine, path: String| {
            Ok(balaur_script::Value::List(
                material_params(eng, &path).unwrap_or_default(),
            ))
        },
    );
}

/// The editor type a field is drawn as. A `vec4` is a colour: it is what one
/// almost always is, `Value::Color` is the engine's own four-channel type,
/// and the `[params]` table takes `#rrggbb` and `[r, g, b, a]` alike.
fn row_type(ty: FieldType) -> &'static str {
    match ty {
        FieldType::F32 => "float",
        FieldType::Vec2 => "vec2",
        FieldType::Vec3 => "vec3",
        FieldType::Vec4 => "color",
    }
}

/// A field's current value, or its zero when the material sets nothing.
fn row_value(ty: FieldType, param: Option<Param>) -> balaur_script::Value {
    use balaur_script::Value;
    match (ty, param) {
        (FieldType::F32, Some(Param::Float(v))) => Value::Num(f64::from(v)),
        (FieldType::Vec2, Some(Param::Vec2(v))) => Value::Vec2(v),
        (FieldType::Vec3, Some(Param::Vec3(v))) => Value::Vec3(v),
        (FieldType::Vec4, Some(Param::Vec4(v))) => Value::Color(v),
        (FieldType::F32, _) => Value::Num(0.0),
        (FieldType::Vec2, _) => Value::Vec2([0.0; 2]),
        (FieldType::Vec3, _) => Value::Vec3([0.0; 3]),
        (FieldType::Vec4, _) => Value::Color([0.0; 4]),
    }
}

fn material_params(eng: &balaur_core::Engine, path: &str) -> Result<Vec<balaur_script::Value>> {
    use balaur_script::Value;
    let files = eng.resource::<balaur_core::project::ProjectFiles>();
    let text = String::from_utf8(files.borrow().read(path)?)?;
    let material = parse(&toml::from_str::<toml::Value>(&text)?)?;
    let source = shader_text(eng, path, &material.shader)?;
    let compiled = compile_with(&material, &source, &crate::shaders::plugin_modules(eng))?;
    Ok(compiled
        .fields
        .iter()
        .map(|field| {
            let set = material
                .params
                .iter()
                .find(|(name, _)| name == &field.name)
                .map(|(_, param)| *param);
            Value::Map(vec![
                ("name".to_string(), Value::Str(field.name.clone())),
                (
                    "type".to_string(),
                    Value::Str(row_type(field.ty).to_string()),
                ),
                ("value".to_string(), row_value(field.ty, set)),
            ])
        })
        .collect())
}

/// Parse the material at `path`, read the shader it names, and link them.
fn check_material(eng: &balaur_core::Engine, path: &str) -> Result<()> {
    let files = eng.resource::<balaur_core::project::ProjectFiles>();
    let text = String::from_utf8(files.borrow().read(path)?)?;
    let material = parse(&toml::from_str::<toml::Value>(&text)?)?;
    let source = shader_text(eng, path, &material.shader)?;
    let modules = crate::shaders::plugin_modules(eng);
    compile_with(&material, &source, &modules).map(|_| ())
}

/// The `--> file:line:column` a WESL diagnostic carries, if it has one.
///
/// `compile` rewrites the module path in a span to the shader file, so what
/// comes out here is a place an editor can put a marker.
fn span_of(message: &str) -> Option<(String, i64, i64)> {
    let head = message.split("--> ").nth(1)?.split_whitespace().next()?;
    let mut parts = head.rsplitn(3, ':');
    let column = parts.next()?.parse().ok()?;
    let line = parts.next()?.parse().ok()?;
    Some((parts.next()?.to_string(), line, column))
}

/// One finding, in the shape `script::check` answers in, so the editor's
/// Problems list takes both without knowing which produced which.
///
/// A link error names the shader and the line in it; anything else — a
/// material that will not parse, a file that is not there — is about the
/// material, which is what `path` is.
fn finding(path: &str, message: &str) -> balaur_script::Value {
    use balaur_script::Value;
    let (file, line, column) = span_of(message).unwrap_or_else(|| (path.to_string(), 0, 0));
    Value::Map(vec![
        ("file".to_string(), Value::Str(file)),
        ("line".to_string(), Value::Int(line)),
        ("column".to_string(), Value::Int(column)),
        ("severity".to_string(), Value::Str("error".to_string())),
        ("message".to_string(), Value::Str(message.to_string())),
    ])
}

/// A scalar or vector a `Params` field may have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    F32,
    Vec2,
    Vec3,
    Vec4,
}

impl FieldType {
    /// WGSL's alignment and size for the type, which is what decides where
    /// the next field starts.
    fn align_size(self) -> (usize, usize) {
        match self {
            FieldType::F32 => (4, 4),
            FieldType::Vec2 => (8, 8),
            FieldType::Vec3 => (16, 12),
            FieldType::Vec4 => (16, 16),
        }
    }

    fn name(self) -> &'static str {
        match self {
            FieldType::F32 => "f32",
            FieldType::Vec2 => "vec2<f32>",
            FieldType::Vec3 => "vec3<f32>",
            FieldType::Vec4 => "vec4<f32>",
        }
    }

    /// The type a WGSL type expression names, or `None` for one a material
    /// cannot write.
    fn parse(ty: &wesl::syntax::TypeExpression) -> Option<Self> {
        let arg_is_f32 = || match &ty.template_args {
            Some(args) if args.len() == 1 => args[0].expression.to_string() == "f32",
            _ => false,
        };
        match ty.ident.name().as_str() {
            "f32" => Some(FieldType::F32),
            "vec2f" => Some(FieldType::Vec2),
            "vec3f" => Some(FieldType::Vec3),
            "vec4f" => Some(FieldType::Vec4),
            "vec2" if arg_is_f32() => Some(FieldType::Vec2),
            "vec3" if arg_is_f32() => Some(FieldType::Vec3),
            "vec4" if arg_is_f32() => Some(FieldType::Vec4),
            _ => None,
        }
    }
}

/// One field of a shader's `Params` struct, and where it sits in the buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: FieldType,
    pub offset: usize,
}

/// The `Params` fields a linked shader declares, laid out the way WGSL lays
/// out a uniform.
///
/// Empty for a shader with no `Params` — most shaders — which is not an
/// error: a material may exist only to pick a variant.
pub fn fields(linked: &TranslationUnit) -> Result<Vec<Field>> {
    let Some(declaration) = linked
        .global_declarations
        .iter()
        .find_map(|d| match d.node() {
            GlobalDeclaration::Struct(s) if s.ident.name().as_str() == PARAMS_STRUCT => Some(s),
            _ => None,
        })
    else {
        return Ok(Vec::new());
    };
    let mut fields = Vec::new();
    let mut offset: usize = 0;
    for member in &declaration.members {
        let name = member.ident.name().to_string();
        let ty = FieldType::parse(&member.ty).ok_or_else(|| {
            anyhow!(
                "`{PARAMS_STRUCT}.{name}` is `{}`; a material writes f32, vec2, vec3 and vec4 only",
                member.ty.ident.name()
            )
        })?;
        let (align, size) = ty.align_size();
        offset = offset.next_multiple_of(align);
        fields.push(Field { name, ty, offset });
        offset += size;
    }
    Ok(fields)
}

/// The bytes `params` make for `fields`, sized as the uniform buffer wants.
///
/// A field no param names keeps its zero. A param no field names is dropped
/// with a warning rather than an error: stripping removes a field the shader
/// stopped reading, and commenting out a line should not fail a scene.
pub fn pack(fields: &[Field], params: &[(String, Param)]) -> Result<Vec<u8>> {
    let end = fields
        .last()
        .map_or(0, |f| f.offset + f.ty.align_size().1)
        .next_multiple_of(UNIFORM_ALIGN);
    let mut bytes = vec![0u8; end];
    for (name, param) in params {
        let Some(field) = fields.iter().find(|f| &f.name == name) else {
            tracing::warn!(
                param = name.as_str(),
                "no such field in the shader's Params"
            );
            continue;
        };
        let expected = match field.ty {
            FieldType::F32 => matches!(param, Param::Float(_)),
            FieldType::Vec2 => matches!(param, Param::Vec2(_)),
            FieldType::Vec3 => matches!(param, Param::Vec3(_)),
            FieldType::Vec4 => matches!(param, Param::Vec4(_)),
        };
        if !expected {
            bail!(
                "param `{name}` is a {}, but the shader declares it {}",
                param.type_name(),
                field.ty.name()
            );
        }
        for (i, value) in param.floats().iter().enumerate() {
            let at = field.offset + i * 4;
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(bytes)
}

/// A material linked and packed: what a backend needs to draw with it.
pub struct Compiled {
    /// The linked WGSL, ready for `create_shader_module`.
    pub wgsl: String,
    /// The `Params` fields the shader declares, in buffer order.
    pub fields: Vec<Field>,
    /// The values, laid out for the uniform buffer; empty for a shader that
    /// declares no `Params`.
    pub params: Vec<u8>,
    /// Whether the shader writes a previewed value out for one pixel — true
    /// only for a source `preview` rewrote.
    pub probes: bool,
}

/// Link `material`'s shader and pack its values against what it declares.
///
/// `source` is the shader file's text. Reading it stays the caller's job:
/// where a project's bytes come from — the pack, the directory, an unsaved
/// editor buffer — is not this module's business.
pub fn compile(material: &Material, source: &str) -> Result<Compiled> {
    compile_with(material, source, &[])
}

/// [`compile`], with modules a plugin registered mounted alongside the
/// engine's own, so a project's shader can import them.
pub fn compile_with(
    material: &Material,
    source: &str,
    plugin_modules: &[(String, String)],
) -> Result<Compiled> {
    let features: Vec<(&str, bool)> = material
        .features
        .iter()
        .map(|(name, on)| (name.as_str(), *on))
        .collect();
    let mut modules: Vec<(&str, &str)> = plugin_modules
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    let root = "package::material";
    // WESL spans name the module they came from, and the module is a name
    // this function invented; the author only ever saw the file, so that is
    // what the error has to point at.
    modules.push((root, source));
    let linked = crate::shaders::link(&modules, root, &features)
        .map_err(|why| anyhow!("{}", format!("{why:#}").replace(root, &material.shader)))?;
    let fields = fields(&linked.syntax)?;
    let params = pack(&fields, &material.params)?;
    // Read off the linked output rather than threaded down from whoever
    // rewrote it: the binding either survived stripping or it did not.
    let probes = linked.syntax.global_declarations.iter().any(|d| {
        matches!(d.node(), GlobalDeclaration::Declaration(v)
            if v.ident.name().as_str() == "balaur_probe")
    });
    Ok(Compiled {
        wgsl: crate::shaders::wgsl(&linked),
        fields,
        params,
        probes,
    })
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
        let by_name = |n: &str| m.params.iter().find(|(k, _)| k == n).unwrap().1;
        assert_eq!(by_name(k::SPEED), Param::Float(0.5));
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
                    name: k::SPEED.into(),
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
        assert!(compile(&on, WITH_VARIANT).unwrap().wgsl.contains("2.0"));
        assert!(compile(&off, WITH_VARIANT).unwrap().wgsl.contains("1.0"));
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
    fn a_finding_takes_its_place_from_the_diagnostic() {
        let material = parse(&table("shader = \"shaders/water.wesl\"")).unwrap();
        let broken = r"
import package::sprite::{VertexInput, VertexOutput, vertex};

@vertex fn vs_main(in: VertexInput) -> VertexOutput {
    return vertex(in)
}
";
        let message = format!("{:#}", compile(&material, broken).err().unwrap());
        assert_eq!(
            span_of(&message),
            Some(("shaders/water.wesl".to_string(), 6, 1))
        );
    }

    #[test]
    fn a_message_with_no_span_places_nothing() {
        assert_eq!(span_of("materials/x.toml: no such file"), None);
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
                    name: k::SPEED.into(),
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

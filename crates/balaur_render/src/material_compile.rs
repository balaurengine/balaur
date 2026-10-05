//! Linking a material's shader and laying its values out: the `Params`
//! fields a linked shader declares, and the uniform bytes a material packs.

use anyhow::{Result, bail};
#[cfg(feature = "compile")]
use anyhow::anyhow;
#[cfg(feature = "compile")]
use wesl::syntax::{Attribute, GlobalDeclaration, Ident, TranslationUnit};

use crate::material::{Material3d, Param};
use crate::prelinked::Linked;

/// The struct a shader declares to take a material's values.
#[cfg(feature = "compile")]
const PARAMS_STRUCT: &str = "Params";

/// A uniform buffer's size is a multiple of this, whatever the struct holds.
const UNIFORM_ALIGN: usize = 16;

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

    pub(crate) fn name(self) -> &'static str {
        match self {
            FieldType::F32 => "f32",
            FieldType::Vec2 => "vec2<f32>",
            FieldType::Vec3 => "vec3<f32>",
            FieldType::Vec4 => "vec4<f32>",
        }
    }

    /// The type [`Self::name`] wrote.
    pub(crate) fn named(name: &str) -> Option<Self> {
        [Self::F32, Self::Vec2, Self::Vec3, Self::Vec4]
            .into_iter()
            .find(|ty| ty.name() == name)
    }

    /// The type a WGSL type expression names, or `None` for one a material
    /// cannot write.
    #[cfg(feature = "compile")]
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
#[cfg(feature = "compile")]
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
        // An image binds a slot, which is not a field of `Params`.
        if matches!(param, Param::Texture(_)) {
            continue;
        }
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
// Each flag is a link feature of its own, so no two of them make a mode.
#[allow(clippy::struct_excessive_bools)]
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
    /// Whether the material asked for a colour per vertex, which decides
    /// whether its pipeline carries the attribute at all.
    pub vertex_color: bool,
    /// Whether it asked for each instance's custom data, the same way.
    pub instance_custom: bool,
    /// The same shader with a second fragment entry point, [`TRANSPARENT_ENTRY`],
    /// for kiss3d's order-independent transparency pass. `None` where the
    /// shader's `fs_main` cannot be wrapped (see [`transparent_variant`]).
    pub transparent_wgsl: Option<String>,
    /// Whether the shader was linked with the `morph` bindings, which only a
    /// device whose vertex stage reads storage buffers takes.
    pub morph: bool,
}

/// The entry point every material's colour pass draws through.
#[cfg(feature = "compile")]
const FRAGMENT_ENTRY: &str = "fs_main";

/// The entry point the transparency pass draws a 3D material through.
pub const TRANSPARENT_ENTRY: &str = "fs_oit";

/// What a material's own `fs_main` is renamed to once both entry points call it.
#[cfg(feature = "compile")]
const FRAGMENT_BODY: &str = "balaur_fragment";

/// The contract's fragment input, which carries the position the
/// transparency pass weighs a fragment's depth by.
#[cfg(feature = "compile")]
const SURFACE_INPUT: &str = "VertexOutput";

/// `source` with [`TRANSPARENT_ENTRY`] beside `fs_main`, linked to WGSL: it
/// writes the material's colour into kiss3d's transparency targets.
///
/// WGSL cannot call an entry point, so `fs_main` becomes a plain function both
/// entry points call. `None` for an `fs_main` that returns anything but a
/// `vec4<f32>` or takes no `VertexOutput`: that material blends in the colour
/// pass instead.
#[must_use]
pub fn transparent_variant(
    material: &Material3d,
    source: &str,
    plugin_modules: &[(String, String)],
    morph: bool,
) -> Option<String> {
    let key = material_key("transparent", material, source, plugin_modules, morph);
    crate::prelinked::cached_optional(key, || {
        link_transparent(material, source, plugin_modules, morph).map(Linked::wgsl)
    })
    .map(|linked| linked.wgsl)
}

#[cfg(not(feature = "compile"))]
fn link_transparent(_: &Material3d, _: &str, _: &[(String, String)], _: bool) -> Option<String> {
    None
}

#[cfg(feature = "compile")]
fn link_transparent(
    material: &Material3d,
    source: &str,
    plugin_modules: &[(String, String)],
    morph: bool,
) -> Option<String> {
    let mut unit: TranslationUnit = source.parse().ok()?;
    let fragment = unit
        .global_declarations
        .iter_mut()
        .find_map(|d| match d.node_mut() {
            GlobalDeclaration::Function(f)
                if f.ident.name().as_str() == FRAGMENT_ENTRY
                    && f.attributes
                        .iter()
                        .any(|a| matches!(a.node(), Attribute::Fragment)) =>
            {
                Some(f)
            }
            _ => None,
        })?;
    let returns = fragment.return_type.as_ref()?.to_string().replace(' ', "");
    if returns != "vec4<f32>" && returns != "vec4f" {
        return None;
    }
    let input = fragment
        .parameters
        .iter()
        .find(|p| p.ty.ident.name().as_str() == SURFACE_INPUT)?
        .ident
        .name()
        .to_string();
    let entry_parameters = fragment
        .parameters
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let arguments = fragment
        .parameters
        .iter()
        .map(|p| p.ident.name().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    fragment.ident = Ident::new(FRAGMENT_BODY.to_string());
    fragment
        .attributes
        .retain(|a| !matches!(a.node(), Attribute::Fragment));
    fragment.return_attributes.clear();
    for parameter in &mut fragment.parameters {
        parameter.attributes.clear();
    }
    let wrapped = format!(
        "{unit}\n@fragment fn {FRAGMENT_ENTRY}({entry_parameters}) -> @location(0) vec4<f32> {{\n    \
         return {FRAGMENT_BODY}({arguments});\n}}\n\n\
         @fragment fn {TRANSPARENT_ENTRY}({entry_parameters}) -> package::mesh::OitOutput {{\n    \
         return package::mesh::oit_output({FRAGMENT_BODY}({arguments}), {input});\n}}\n"
    );
    link_material(material, &wrapped, plugin_modules, morph)
        .inspect_err(|why| {
            tracing::debug!(
                shader = material.shader,
                "no transparency entry point: {why:#}"
            );
        })
        .ok()
        .map(|linked| linked.wgsl)
}

/// Link `material`'s shader and pack its values against what it declares.
///
/// `source` is the shader file's text. Reading it stays the caller's job:
/// where a project's bytes come from — the pack, the directory, an unsaved
/// editor buffer — is not this module's business.
pub fn compile(material: &Material3d, source: &str) -> Result<Compiled> {
    compile_with(material, source, &[])
}

/// [`compile`], with modules a plugin registered mounted alongside the
/// engine's own, so a project's shader can import them.
pub fn compile_with(
    material: &Material3d,
    source: &str,
    plugin_modules: &[(String, String)],
) -> Result<Compiled> {
    compile_on(material, source, plugin_modules, false)
}

/// [`compile_with`] for a device: `morph` links the morph-target bindings in.
/// The engine decides it, so a project's own `morph` feature is not read.
pub fn compile_on(
    material: &Material3d,
    source: &str,
    plugin_modules: &[(String, String)],
    morph: bool,
) -> Result<Compiled> {
    let key = material_key("material", material, source, plugin_modules, morph);
    let linked = crate::prelinked::cached(key, || {
        link_material(material, source, plugin_modules, morph)
    })?;
    let params = pack(&linked.fields, &material.params)?;
    Ok(Compiled {
        wgsl: linked.wgsl,
        fields: linked.fields,
        params,
        probes: linked.probes,
        vertex_color: material.reads_vertex_color(),
        instance_custom: material.reads_instance_custom(),
        transparent_wgsl: None,
        morph,
    })
}

/// The root module a material's own shader is linked as.
const MATERIAL_ROOT: &str = "package::material";

/// The `@if` flags `material` links with on a device that does or does not
/// morph.
fn features(material: &Material3d, morph: bool) -> Vec<(&str, bool)> {
    material
        .features
        .iter()
        .filter(|(name, _)| name != crate::shaders::MORPH)
        .map(|(name, on)| (name.as_str(), *on))
        .chain(std::iter::once((crate::shaders::MORPH, morph)))
        .collect()
}

/// What [`crate::prelinked`] keeps a material's link under.
fn material_key(
    kind: &str,
    material: &Material3d,
    source: &str,
    plugin_modules: &[(String, String)],
    morph: bool,
) -> u64 {
    let mut modules: Vec<(&str, &str)> = plugin_modules
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    modules.push((MATERIAL_ROOT, source));
    crate::prelinked::key(kind, &modules, MATERIAL_ROOT, &features(material, morph))
}

#[cfg(not(feature = "compile"))]
fn link_material(_: &Material3d, _: &str, _: &[(String, String)], _: bool) -> Result<Linked> {
    bail!("this build has no shader linker")
}

#[cfg(feature = "compile")]
fn link_material(
    material: &Material3d,
    source: &str,
    plugin_modules: &[(String, String)],
    morph: bool,
) -> Result<Linked> {
    let mut modules: Vec<(&str, &str)> = plugin_modules
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    // WESL spans name the module they came from, and the module is a name
    // this function invented; the author only ever saw the file, so that is
    // what the error has to point at.
    modules.push((MATERIAL_ROOT, source));
    let linked = crate::shaders::link(&modules, MATERIAL_ROOT, &features(material, morph)).map_err(
        |why| anyhow!("{}", format!("{why:#}").replace(MATERIAL_ROOT, &material.shader)),
    )?;
    let fields = fields(&linked.syntax)?;
    // Read off the linked output rather than threaded down from whoever
    // rewrote it: the binding either survived stripping or it did not.
    let probes = linked.syntax.global_declarations.iter().any(|d| {
        matches!(d.node(), GlobalDeclaration::Declaration(v)
            if v.ident.name().as_str() == "balaur_probe")
    });
    Ok(Linked {
        wgsl: crate::shaders::wgsl(&linked)?,
        fields,
        probes,
    })
}

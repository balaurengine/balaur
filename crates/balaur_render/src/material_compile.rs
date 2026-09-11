//! Linking a material's shader and laying its values out: the `Params`
//! fields a linked shader declares, and the uniform bytes a material packs.

use anyhow::{Result, anyhow, bail};
use wesl::syntax::{GlobalDeclaration, TranslationUnit};

use crate::material::{Material, Param};

/// The struct a shader declares to take a material's values.
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
        vertex_color: material.reads_vertex_color(),
    })
}

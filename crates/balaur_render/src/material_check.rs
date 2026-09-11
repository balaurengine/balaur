//! `render::check_material` and `render::material_params`: a material linked
//! on the CPU, for the editor's Problems list and its inspector rows.

use anyhow::Result;
use balaur_script::{Bindings, BindingsExt};

use crate::material::{
    FieldType, Material, Param, TEXTURE_SLOTS, compile_with, parse, shader_text,
};

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

/// The material behind any reference the engine can resolve: a file, an
/// `id://`, or the `#id` of a scene's own `[[assets]]` block. Read through
/// `assets::definition` rather than as a file, so an inline material reaches
/// the same panel a file one does.
fn material_at(eng: &balaur_core::Engine, reference: &str) -> Result<Material> {
    parse(&balaur_core::assets::definition(eng, reference)?)
}

fn material_params(eng: &balaur_core::Engine, path: &str) -> Result<Vec<balaur_script::Value>> {
    use balaur_script::Value;
    let material = material_at(eng, path)?;
    let source = shader_text(eng, path, &material.shader)?;
    let compiled = compile_with(&material, &source, &crate::shaders::plugin_modules(eng))?;
    let mut rows: Vec<Value> = compiled
        .fields
        .iter()
        .map(|field| {
            let set = material
                .params
                .iter()
                .find(|(name, _)| name == &field.name)
                .map(|(_, param)| param.clone());
            Value::Map(vec![
                ("name".to_string(), Value::Str(field.name.clone())),
                (
                    "type".to_string(),
                    Value::Str(row_type(field.ty).to_string()),
                ),
                ("value".to_string(), row_value(field.ty, set)),
            ])
        })
        .collect();
    // The texture slots, which are bindings rather than fields of `Params`
    // and so are not in what the shader compiled to.
    for (slot, bound) in TEXTURE_SLOTS.iter().zip(material.textures()) {
        rows.push(Value::Map(vec![
            ("name".to_string(), Value::Str((*slot).to_string())),
            ("type".to_string(), Value::Str("texture".to_string())),
            (
                "value".to_string(),
                Value::Str(bound.unwrap_or_default().to_string()),
            ),
        ]));
    }
    Ok(rows)
}

/// Parse the material at `path`, read the shader it names, and link them.
fn check_material(eng: &balaur_core::Engine, path: &str) -> Result<()> {
    let material = material_at(eng, path)?;
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

#[cfg(test)]
mod tests {
    use super::span_of;
    use crate::material::{compile, parse};

    fn table(text: &str) -> toml::Value {
        toml::from_str(text).unwrap()
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
}

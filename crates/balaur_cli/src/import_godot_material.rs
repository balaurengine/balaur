//! A Godot `ShaderMaterial` as an inline `material` asset.
//!
//! The shader is the translation `import_godot_shader` wrote for the
//! `.gdshader` the material names, or one written here for a shader saved
//! inside the scene. Its uniforms' defaults come first and the material's
//! `shader_parameter/*` over them, each in the shape its `Params` field takes:
//! a bool or an int as a number, a `source_color` in linear light, an image as
//! the slot its sampler was given.

use std::rc::Rc;

use balaur_plugin::toml;
use toml::Value as Toml;

use crate::import_godot::{Section, Value};
use crate::import_godot_nodes::{Asset, Mapped, Resources, image_path};
use crate::import_godot_shader::{Translated, Uniform, field_name, linear};

/// A shader translated once for the whole import, and where it was written.
pub(crate) struct Shader {
    pub path: String,
    pub translated: Translated,
}

/// Where a `.gdshader` is translated to: beside it, as `.wesl`.
pub(crate) fn shader_path(godot: &str) -> String {
    format!("{}.wesl", godot.strip_suffix(".gdshader").unwrap_or(godot))
}

/// Put the material a node's `material` names on it: on the renderable's own
/// `material` when it has one, else on the `material` component, which its
/// descendants draw with too, as Godot's `use_parent_material` children do.
pub(crate) fn attach(value: &Value, res: &Resources<'_>, out: &mut Mapped) {
    let loaded;
    let (section, lookup, kind) = if let Some(section) = res.sub(value) {
        (section, res, section.attr_str("type"))
    } else if let Some((document, nested)) = crate::import_godot_nodes::load(res, value) {
        loaded = (document, nested);
        let Some(section) = loaded.0.first("resource") else {
            return;
        };
        // A resource saved as its own file states its type on the header.
        let kind = loaded.0.first("gd_resource").and_then(|h| h.attr_str("type"));
        (section, &loaded.1, kind)
    } else {
        return;
    };
    match kind {
        Some("ShaderMaterial") => {}
        Some("CanvasItemMaterial") => {
            out.note("a CanvasItemMaterial: its blend mode and light mode have no equivalent");
            return;
        }
        other => {
            out.note(format!("a {} material has no equivalent", other.unwrap_or("typeless")));
            return;
        }
    }
    let Some(shader) = shader_of(section, lookup, out) else {
        return;
    };
    let mut table = toml::Table::new();
    table.insert("type".into(), Toml::String("material".into()));
    table.insert("shader".into(), Toml::String(shader.path.clone()));
    if shader.translated.screen {
        let mut features = toml::Table::new();
        features.insert("screen".into(), Toml::Boolean(true));
        table.insert("features".into(), Toml::Table(features));
    }
    let mut params = toml::Table::new();
    for uniform in &shader.translated.uniforms {
        let set = section.field(&format!("shader_parameter/{}", uniform.name));
        let value = match set {
            Some(value) => param(uniform, value, lookup, out),
            None => uniform.default.as_deref().and_then(|d| numbers(uniform, d)),
        };
        if let Some(value) = value {
            params.insert(key(uniform), value);
        }
    }
    if !params.is_empty() {
        table.insert("params".into(), Toml::Table(params));
    }
    let (component, key) = if out.components.contains_key("sprite") {
        ("sprite", "material")
    } else if out.components.contains_key("shape2d") {
        ("shape2d", "material")
    } else {
        out.touch("material");
        ("material", "source")
    };
    out.assets.push(Asset {
        component,
        key,
        table,
    });
}

/// The translated shader a material names: one the import already wrote,
/// or the source of one saved inside the scene, translated here.
fn shader_of(section: &Section, res: &Resources<'_>, out: &mut Mapped) -> Option<Rc<Shader>> {
    let value = section.field("shader")?;
    if let Some(path) = res.path(value) {
        let found = res.project.shaders.get(path).cloned();
        if found.is_none() {
            out.note(format!(
                "{path} did not translate, so the material was dropped; the import report says why"
            ));
        }
        return found;
    }
    let inline = res.sub(value)?;
    let code = inline.field("code").and_then(Value::as_str)?;
    let translated = match crate::import_godot_shader::translate(code) {
        Ok(translated) => translated,
        Err(why) => {
            out.note(format!("its shader, saved in the scene, did not translate: {why:#}"));
            return None;
        }
    };
    let path = format!("shaders/inline_{:016x}.wesl", fnv(code));
    for note in &translated.notes {
        out.note(format!("its shader: {note}"));
    }
    out.files.push((path.clone(), translated.wesl.clone()));
    if let Err(why) = crate::import_godot_shader::check(&translated) {
        out.note(format!(
            "its shader, saved in the scene as {path}, does not compile, so the material was dropped: {why:#}"
        ));
        return None;
    }
    Some(Rc::new(Shader { path, translated }))
}

/// The `[params]` key a uniform is set under: its field, or the image slot
/// its sampler binds.
fn key(uniform: &Uniform) -> String {
    match uniform.slot {
        Some(slot) => format!("texture_{slot}"),
        None => field_name(&uniform.name),
    }
}

/// A material's value for one uniform, in the shape its field takes.
fn param(uniform: &Uniform, value: &Value, res: &Resources<'_>, out: &mut Mapped) -> Option<Toml> {
    if uniform.slot.is_some() {
        let Some(path) = res.path(value) else {
            out.note(format!(
                "`{}`: only an image file binds a slot; a generated texture has no equivalent",
                uniform.name
            ));
            return None;
        };
        return Some(Toml::String(image_path(path, res, out)));
    }
    if uniform.screen {
        return None;
    }
    let values = match value {
        Value::Bool(on) => vec![if *on { 1.0 } else { 0.0 }],
        other => other.as_f64().map(|n| vec![n]).or_else(|| other.numbers())?,
    };
    numbers(uniform, &values)
}

/// Numbers as the field's type: one for a scalar, a list for a vector, the
/// colour channels in linear light for a `source_color`.
fn numbers(uniform: &Uniform, values: &[f64]) -> Option<Toml> {
    if uniform.slot.is_some() || uniform.screen {
        return None;
    }
    let width = match uniform.ty.as_str() {
        "vec2" => 2,
        "vec3" => 3,
        "vec4" => 4,
        _ => 1,
    };
    let mut values: Vec<f64> = values.iter().copied().take(width).collect();
    if values.len() < width {
        return None;
    }
    if uniform.source_color {
        for channel in values.iter_mut().take(3) {
            *channel = linear(*channel);
        }
    }
    Some(if width == 1 {
        Toml::Float(values[0])
    } else {
        Toml::Array(values.into_iter().map(Toml::Float).collect())
    })
}

/// FNV-1a, for naming a shader saved inside a scene by its source.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

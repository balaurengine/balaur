// What a script declares tunable: every `pub` field of its class, as a spec in
// the component schema's words, so an inspector draws it with the same rows.

use super::File;
use crate::convert::from_neutral;
use crate::vocabulary::keys;
use crate::vocabulary::words::{doc_types as kinds, records};
use balaur_bs::runtime::Value as L;
use balaur_bs::{Literal, Meta, Ty};
use balaur_script::Value;

/// How deep a field's type may nest before it is left out: a struct that holds
/// a list of itself has no end to draw.
const DEEPEST: usize = 8;

/// Every `pub` field of `file`'s class with the spec it is drawn and set by,
/// in declaration order. A field whose type has no shape to draw, an `any` or
/// a `fn`, is left out.
pub(super) fn exports(file: &File, path: &str) -> Vec<(String, Value)> {
    let meta = &file.compiled.meta;
    let defaults = meta.defaults(&file.ty);
    meta.public_fields(&file.ty)
        .iter()
        .filter_map(|(name, doc)| {
            let ty = file.field_ty(name)?;
            let spec = spec_of(meta, &ty, 0)?;
            let literal = defaults
                .iter()
                .find(|(field, _, _)| field == name)
                .map(|(_, _, literal)| literal);
            let default = match literal {
                Some(literal) => written(literal, &ty),
                None => zero(&spec)?,
            };
            let Value::Map(mut pairs) = spec else {
                return None;
            };
            pairs.push((keys::DEFAULT.into(), default));
            if !doc.is_empty() {
                pairs.push((keys::DESCRIPTION.into(), Value::text(doc)));
            }
            match balaur_core::node_api::checked_property_spec(&Value::Map(pairs)) {
                Ok(done) => Some((name.clone(), done)),
                Err(why) => {
                    tracing::warn!("[{path}] property '{name}' is not shown: {why}");
                    None
                }
            }
        })
        .collect()
}

/// Whether `name` is the class a script file declares, which a scene names
/// by the path of the node running it.
pub(super) fn is_script_class(meta: &Meta, name: &str) -> bool {
    let own = name.rsplit("::").next().unwrap_or(name);
    meta.class_file(name)
        .is_some_and(|file| balaur_bs::script_class(file) == own)
}

fn typed(kind: &str) -> Value {
    Value::Map(vec![(keys::TYPE.into(), Value::text(kind))])
}

fn spec_of(meta: &Meta, ty: &Ty, depth: usize) -> Option<Value> {
    if depth > DEEPEST {
        return None;
    }
    Some(match ty {
        Ty::Int => typed(kinds::INT),
        Ty::Float => typed(kinds::FLOAT),
        Ty::Bool => typed(kinds::BOOL),
        Ty::Str => typed(kinds::STRING),
        Ty::Node => typed(kinds::NODE),
        Ty::Optional(inner) => return spec_of(meta, inner, depth),
        Ty::Array(inner) => Value::Map(vec![
            (keys::TYPE.into(), Value::text(kinds::LIST)),
            (keys::OF.into(), spec_of(meta, inner, depth + 1)?),
        ]),
        Ty::Struct(name) => return struct_spec(meta, name, depth),
        _ => return None,
    })
}

fn struct_spec(meta: &Meta, name: &str, depth: usize) -> Option<Value> {
    match name {
        records::VEC2 => return Some(typed(kinds::VEC2)),
        records::VEC3 => return Some(typed(kinds::VEC3)),
        records::COLOR => return Some(typed(kinds::COLOR)),
        records::TRANSFORM_2D | records::TRANSFORM_3D => return None,
        _ => {}
    }
    if let Some((key, value)) = meta.map_types(name) {
        let key = match key {
            Ty::Str => kinds::STRING,
            Ty::Int => kinds::INT,
            _ => return None,
        };
        return Some(Value::Map(vec![
            (keys::TYPE.into(), Value::text(kinds::MAP)),
            (keys::KEY.into(), Value::text(key)),
            (keys::OF.into(), spec_of(meta, &value, depth + 1)?),
        ]));
    }
    if let Some(variants) = meta.variants(name) {
        return enum_spec(meta, name, variants, depth);
    }
    if is_script_class(meta, name) {
        return Some(typed(kinds::NODE));
    }
    let declared = meta.fields(name)?;
    let fields = meta
        .public_fields(name)
        .iter()
        .filter_map(|(field, _)| {
            let ty = &declared.iter().find(|(n, _)| n == field)?.1;
            Some((field.clone(), spec_of(meta, ty, depth + 1)?))
        })
        .collect();
    Some(record(fields))
}

fn record(fields: Vec<(String, Value)>) -> Value {
    Value::Map(vec![
        (keys::TYPE.into(), Value::text(kinds::RECORD)),
        (keys::FIELDS.into(), Value::Map(fields)),
    ])
}

/// A unit-only enum is one of its names; one whose variants carry fields is a
/// `variant`, each a record of its fields, tuple ones named `0`, `1`.
fn enum_spec(meta: &Meta, name: &str, variants: &[String], depth: usize) -> Option<Value> {
    let holder = meta.fields(name).unwrap_or(&[]);
    let payload = |variant: &str| -> Vec<(String, Ty)> {
        let prefix = balaur_bs::enum_slot(variant, "");
        holder
            .iter()
            .filter_map(|(slot, ty)| {
                let field = slot.strip_prefix(&prefix)?;
                let ty = match ty {
                    Ty::Optional(inner) => (**inner).clone(),
                    other => other.clone(),
                };
                Some((field.to_string(), ty))
            })
            .collect()
    };
    if variants.iter().all(|variant| payload(variant).is_empty()) {
        return Some(Value::Map(vec![
            (keys::TYPE.into(), Value::text(kinds::ENUM)),
            (
                keys::OPTIONS.into(),
                Value::List(variants.iter().map(Value::text).collect()),
            ),
        ]));
    }
    let mut shaped = Vec::new();
    for variant in variants {
        let mut fields = Vec::new();
        for (field, ty) in payload(variant) {
            fields.push((field, spec_of(meta, &ty, depth + 1)?));
        }
        shaped.push((variant.clone(), record(fields)));
    }
    Some(Value::Map(vec![
        (keys::TYPE.into(), Value::text(kinds::VARIANT)),
        (keys::VARIANTS.into(), Value::Map(shaped)),
    ]))
}

/// A field's literal default, a float field's whole number read as a float.
fn written(literal: &Literal, ty: &Ty) -> Value {
    match literal {
        Literal::Int(v) if *ty == Ty::Float => Value::Num(*v as f64),
        Literal::Int(v) => Value::Int(*v),
        Literal::Float(v) => Value::Num(*v),
        Literal::Bool(v) => Value::Bool(*v),
        Literal::Str(v) => Value::Str(v.clone()),
    }
}

/// What a field without a default starts at, as the schema reads the spec.
fn zero(spec: &Value) -> Option<Value> {
    let table = balaur_core::node_api::to_toml(spec).ok()?;
    balaur_core::node_api::from_toml(&balaur_core::components::zero_of(&table)).ok()
}

/// The typed map class a field holds, a `{ K: V }` or a `T?` of one.
pub(super) fn map_class(file: &File, ty: &Ty) -> Option<String> {
    let inner = match ty {
        Ty::Optional(inner) => &**inner,
        other => other,
    };
    match inner {
        Ty::Struct(name) if file.compiled.meta.map_types(name).is_some() => Some(name.clone()),
        _ => None,
    }
}

/// A scene's table as the typed map `class`, made by the map's own `of` so
/// its slots are filled as a literal's are. `None`, logged, for a table whose
/// keys the map cannot take.
pub(super) fn made_map(file: &File, class: &str, table: &Value, path: &str) -> Option<L> {
    let meta = &file.compiled.meta;
    let (key_ty, value_ty) = meta.map_types(class)?;
    let maker = meta.map_maker(class)?;
    let Value::Map(entries) = table else {
        return None;
    };
    let mut keys = Vec::with_capacity(entries.len());
    let mut values = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        keys.push(match key_ty {
            Ty::Int => L::Int(key.parse().ok()?),
            _ => L::Str(key.clone()),
        });
        values.push(from_neutral(value, &value_ty, meta));
    }
    let args = [L::Array(keys), L::Array(values)];
    let made = file.either(
        |rt| rt.in_unit(file.unit, |rt| rt.call(&maker, &args)),
        |re| re.call(file.unit, &maker, &args),
    );
    match made {
        Ok(made) => Some(made),
        Err(e) => {
            tracing::warn!("[{path}] a map property: {e}");
            None
        }
    }
}

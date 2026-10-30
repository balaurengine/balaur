//! A component written once: its struct is its schema (PLAN-property-types §5).
//!
//! `#[derive(Component)]` (from `balaur_derive`) writes [`Fields`] and
//! [`Component`] for a struct, and [`ComponentDef::of`] makes the definition
//! from them: the schema, a property a field in field order; `apply` and
//! `get` as a table in and out; and a read and a write of one property by its
//! index, which is how a script or an animation drives one value without the
//! component's whole table. A field's type says what the property is through
//! [`Property`], which the derive needs of every field.
//!
//! The struct holds what the component is asked for and nothing else. What a
//! system works out from it -- a shaped text, a built body -- is the system's
//! own, kept beside it.

use std::rc::Rc;

pub use anyhow::Result;
use anyhow::anyhow;
/// The derives, beside the traits they write: one import brings both.
pub use balaur_derive::{Component, Enum, Record};
use hecs::Entity;
pub use toml;

use super::{ComponentDef, Schema};
use crate::engine::Engine;

/// A value a property holds: what its spec says it is, and how it is written
/// in a file and read back.
pub trait Property: Sized {
    /// Write the keys that say what this type is into `spec`: `type`, and
    /// `options`, `of` or `fields` where it holds more. A key already there
    /// was written by the field's `#[property(...)]` and stays.
    fn describe(spec: &mut toml::map::Map<String, toml::Value>);
    fn to_value(&self) -> toml::Value;
    fn from_value(value: &toml::Value) -> Result<Self>;
}

/// The properties of a struct, a field each, by their place.
pub trait Fields: Default {
    /// Each property's name in field order: its index is its place here.
    const NAMES: &'static [&'static str];
    /// Each property's spec in field order, its `default` included.
    fn specs() -> Vec<toml::Value>;
    /// One property, by its index.
    fn read(&self, index: usize) -> Option<toml::Value>;
    /// Write one property, by its index. A value of the wrong shape is
    /// refused and the field keeps what it held.
    fn write(&mut self, index: usize, value: &toml::Value) -> Result<()>;
}

/// A struct that is a component: [`Fields`] and the name a scene keys it by.
pub trait Component: Fields + Send + Sync + 'static {
    const NAME: &'static str;
    /// What the component gives a node, for the generated reference: the
    /// struct's doc comment.
    const DOC: &'static str;
}

/// What a component does once it is written, beyond holding the values: a
/// system's cache to rebuild, a body to make, a refusal of what the types
/// alone cannot judge.
pub type AfterWriteFn = fn(&Engine, Entity, Written) -> Result<()>;

/// Whether a write gave the node the component or changed the one it had.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Written {
    Added,
    Changed,
}

/// Every property of `held`, as the table a file holds.
pub fn to_table<F: Fields>(held: &F) -> toml::Value {
    let mut out = toml::map::Map::new();
    for (index, name) in F::NAMES.iter().enumerate() {
        if let Some(value) = held.read(index) {
            out.insert((*name).to_owned(), value);
        }
    }
    toml::Value::Table(out)
}

/// A struct from a table: what the table names, the defaults for the rest.
pub fn from_table<F: Fields>(table: &toml::Value) -> Result<F> {
    let mut out = F::default();
    let Some(table) = table.as_table() else {
        return Err(anyhow!(
            "is {}, where a table was expected",
            table.type_str()
        ));
    };
    for (index, name) in F::NAMES.iter().enumerate() {
        if let Some(value) = table.get(*name) {
            out.write(index, value)?;
        }
    }
    Ok(out)
}

/// A `record` spec, its fields the struct's.
pub fn describe_record<F: Fields>(spec: &mut toml::map::Map<String, toml::Value>) {
    spec.entry("type")
        .or_insert_with(|| toml::Value::String("record".into()));
    let fields = F::NAMES
        .iter()
        .zip(F::specs())
        .map(|(name, field)| ((*name).to_owned(), field))
        .collect();
    spec.insert("fields".into(), toml::Value::Table(fields));
}

/// An `enum` spec offering `options`.
pub fn describe_enum(spec: &mut toml::map::Map<String, toml::Value>, options: &[&str]) {
    spec.entry("type")
        .or_insert_with(|| toml::Value::String("enum".into()));
    spec.entry("options").or_insert_with(|| {
        toml::Value::Array(
            options
                .iter()
                .map(|option| toml::Value::String((*option).to_owned()))
                .collect(),
        )
    });
}

/// The refusal of a value that is none of an enum's options.
pub fn not_an_option(value: &toml::Value, options: &[&str]) -> anyhow::Error {
    anyhow!("is {value}, which is none of {}", options.join(", "))
}

/// The refusal of an index past the last property.
pub fn no_property(index: usize) -> anyhow::Error {
    anyhow!("there is no property {index}")
}

fn set_type(spec: &mut toml::map::Map<String, toml::Value>, name: &str) {
    spec.entry("type")
        .or_insert_with(|| toml::Value::String(name.to_owned()));
}

fn wrong(value: &toml::Value, wanted: &str) -> anyhow::Error {
    anyhow!("is {}, where {wanted} was expected", value.type_str())
}

fn number(value: &toml::Value) -> Result<f64> {
    super::as_f64(value).ok_or_else(|| wrong(value, "a number"))
}

fn numbers<const N: usize>(value: &toml::Value) -> Result<[f32; N]> {
    // A colour may be written as `#rrggbb`, and a schema says which fields
    // are colours rather than the Rust type.
    if let Some(text) = value.as_str() {
        if N == 4
            && let Some(color) = super::schema::hex_rgba(text)
        {
            let mut out = [0.0; N];
            for (slot, channel) in out.iter_mut().zip(color) {
                *slot = channel as f32;
            }
            return Ok(out);
        }
        return Err(wrong(value, &format!("{N} numbers")));
    }
    let list = value
        .as_array()
        .ok_or_else(|| wrong(value, &format!("{N} numbers")))?;
    // A colour's alpha may be left out, and is then opaque.
    if list.len() != N && !(N == 4 && list.len() == 3) {
        return Err(anyhow!(
            "holds {} numbers, where {N} were expected",
            list.len()
        ));
    }
    let mut out = [1.0; N];
    for (slot, entry) in out.iter_mut().zip(list) {
        *slot = number(entry)? as f32;
    }
    Ok(out)
}

fn floats(values: &[f32]) -> toml::Value {
    toml::Value::Array(
        values
            .iter()
            .map(|value| toml::Value::Float(f64::from(*value)))
            .collect(),
    )
}

impl Property for f32 {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        set_type(spec, "float");
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::Float(f64::from(*self))
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        number(value).map(|number| number as Self)
    }
}

impl Property for f64 {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        set_type(spec, "float");
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::Float(*self)
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        number(value)
    }
}

macro_rules! whole_number {
    ($($kind:ty),*) => {$(
        impl Property for $kind {
            fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
                set_type(spec, "int");
            }

            fn to_value(&self) -> toml::Value {
                toml::Value::Integer(i64::try_from(*self).unwrap_or(i64::MAX))
            }

            fn from_value(value: &toml::Value) -> Result<Self> {
                let whole = value
                    .as_integer()
                    .ok_or_else(|| wrong(value, "a whole number"))?;
                <$kind>::try_from(whole)
                    .map_err(|_| anyhow!("is {whole}, which does not fit {}", stringify!($kind)))
            }
        }
    )*};
}

whole_number!(i32, i64, u8, u16, u32, u64, usize);

impl Property for bool {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        set_type(spec, "bool");
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::Boolean(*self)
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        value.as_bool().ok_or_else(|| wrong(value, "true or false"))
    }
}

impl Property for String {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        set_type(spec, "string");
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::String(self.clone())
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| wrong(value, "text"))
    }
}

impl Property for smol_str::SmolStr {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        set_type(spec, "string");
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::String(self.to_string())
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        value
            .as_str()
            .map(Self::new)
            .ok_or_else(|| wrong(value, "text"))
    }
}

macro_rules! vector {
    ($kind:ty, $size:literal, $name:literal) => {
        impl Property for $kind {
            fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
                set_type(spec, $name);
            }

            fn to_value(&self) -> toml::Value {
                floats(&<[f32; $size]>::from(*self))
            }

            fn from_value(value: &toml::Value) -> Result<Self> {
                numbers::<$size>(value).map(Self::from)
            }
        }
    };
}

vector!([f32; 2], 2, "vec2");
vector!([f32; 3], 3, "vec3");
vector!([f32; 4], 4, "vec4");
vector!(glamx::Vec2, 2, "vec2");
vector!(glamx::Vec3, 3, "vec3");
vector!(glamx::Vec4, 4, "vec4");

/// A rotation is written as euler angles in radians, x then y then z, which is
/// what a person can type; the struct keeps the quaternion it turns into.
impl Property for glamx::Quat {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        set_type(spec, "vec3");
    }

    fn to_value(&self) -> toml::Value {
        let (yaw, pitch, roll) = self.to_euler(glamx::EulerRot::ZYX);
        floats(&[roll, pitch, yaw])
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        let [x, y, z] = numbers::<3>(value)?;
        Ok(Self::from_euler(glamx::EulerRot::ZYX, z, y, x))
    }
}

/// A list, or with `flags` on the field a set of an enum's options: the
/// options then belong to the property rather than to what it holds.
impl<T: Property> Property for Vec<T> {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        let mut held = toml::map::Map::new();
        T::describe(&mut held);
        if spec.get("type").and_then(toml::Value::as_str) == Some("flags") {
            if let Some(options) = held.remove("options") {
                spec.entry("options").or_insert(options);
            }
            return;
        }
        set_type(spec, "list");
        spec.entry("of").or_insert(toml::Value::Table(held));
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::Array(self.iter().map(Property::to_value).collect())
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        value
            .as_array()
            .ok_or_else(|| wrong(value, "a list"))?
            .iter()
            .enumerate()
            .map(|(at, entry)| {
                T::from_value(entry).map_err(|why| why.context(format!("entry {at}")))
            })
            .collect()
    }
}

impl<T: Property> Property for indexmap::IndexMap<String, T> {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        set_type(spec, "map");
        spec.entry("of").or_insert_with(|| {
            let mut held = toml::map::Map::new();
            T::describe(&mut held);
            toml::Value::Table(held)
        });
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::Table(
            self.iter()
                .map(|(key, value)| (key.clone(), value.to_value()))
                .collect(),
        )
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        value
            .as_table()
            .ok_or_else(|| wrong(value, "a table"))?
            .iter()
            .map(|(key, entry)| {
                T::from_value(entry)
                    .map(|held| (key.clone(), held))
                    .map_err(|why| why.context(key.clone()))
            })
            .collect()
    }
}

impl ComponentDef {
    /// The definition of a derived component, its `apply`, `get`, `remove`
    /// and the reads and writes of one property all from `C`.
    ///
    /// `after_write` runs once a write has landed, whole or one property,
    /// for what the component
    /// does beyond holding its values. Everything the struct does not say --
    /// `tags`, `events`, `expects`, `warnings` -- is the caller's, written
    /// over this with `..`.
    #[must_use]
    pub fn of<C: Component>(after_write: Option<AfterWriteFn>) -> Self {
        Self {
            schema: schema_of::<C>(),
            doc: C::DOC,
            tags: &[],
            expects: &[],
            events: &[],
            warnings: None,
            apply: Box::new(move |eng, entity, params| {
                let next: C = from_table(params)?;
                let written = {
                    let mut world = eng.world_mut();
                    let next = match world.get::<&mut C>(entity) {
                        Ok(mut held) => {
                            *held = next;
                            None
                        }
                        Err(_) => Some(next),
                    };
                    match next {
                        Some(next) => {
                            world
                                .insert_one(entity, next)
                                .map_err(|_| anyhow!("node is dead"))?;
                            Written::Added
                        }
                        None => Written::Changed,
                    }
                };
                match after_write {
                    Some(after) => after(eng, entity, written),
                    None => Ok(()),
                }
            }),
            remove: Box::new(|eng, entity| {
                let _ = eng.world_mut().remove_one::<C>(entity);
                Ok(())
            }),
            get: Box::new(|eng, entity| {
                let world = eng.world();
                let held = world.get::<&C>(entity).ok()?;
                Some(to_table(&*held))
            }),
            read: Some(Box::new(|eng, entity, index| {
                let world = eng.world();
                let held = world.get::<&C>(entity).ok()?;
                held.read(index)
            })),
            write: Some(Box::new(move |eng, entity, index, value| {
                {
                    let world = eng.world_mut();
                    let Ok(mut held) = world.get::<&mut C>(entity) else {
                        return Ok(false);
                    };
                    held.write(index, value)?;
                }
                if let Some(after) = after_write {
                    after(eng, entity, Written::Changed)?;
                }
                Ok(true)
            })),
        }
    }
}

/// `C`'s schema, a spec a field in field order.
///
/// # Panics
/// On a spec the schema rules refuse, naming the component and the property:
/// a derive is the engine's code, so a bad one is a bug.
#[must_use]
pub fn schema_of<C: Component>() -> Rc<Schema> {
    let specs = C::NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .zip(C::specs())
        .collect();
    ComponentDef::schema_from(C::NAME, specs)
}

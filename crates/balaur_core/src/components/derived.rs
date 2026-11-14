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
//! `#[component(on_add = f, on_remove = g)]` beside its `name` declares the
//! component's [`Lifecycle`]: plain functions `fn(&Engine, Entity)`, run by
//! every write through the registry. `requires = [name, ..]` names the
//! components it brings, each put on at its defaults before its first write
//! where the node lacks it.
//!
//! The struct holds what the component is asked for and nothing else. What a
//! system works out from it -- a shaped text, a built body -- is the system's
//! own, kept beside it.

use std::rc::Rc;

pub use anyhow::Result;
use anyhow::anyhow;
/// The derives, beside the traits they write: one import brings both.
pub use balaur_derive::{Component, Enum, Record, Variant};
use hecs::Entity;
pub use toml;

use super::{ComponentDef, Schema, WriteEachFn, WriteFn};
use crate::engine::Engine;
use crate::vocabulary::keys;
use crate::vocabulary::words;

/// A property's value with nothing built: what a write hands a field and what
/// a field hands a read, for the kinds a script passes in its call's slots.
/// `Value` is everything else, as a file holds it.
#[derive(Clone, Copy, Debug)]
pub enum Given<'a> {
    Float(f64),
    Int(i64),
    Bool(bool),
    /// A vector's or a colour's numbers, and how many.
    Numbers([f32; 4], usize),
    Text(&'a str),
    Value(&'a toml::Value),
}

impl Given<'_> {
    /// The value a file would hold for it.
    #[must_use]
    pub fn to_value(self) -> toml::Value {
        match self {
            Self::Float(number) => toml::Value::Float(number),
            Self::Int(number) => toml::Value::Integer(number),
            Self::Bool(flag) => toml::Value::Boolean(flag),
            Self::Numbers(numbers, count) => toml::Value::Array(
                numbers[..count]
                    .iter()
                    .map(|number| toml::Value::Float(f64::from(*number)))
                    .collect(),
            ),
            Self::Text(text) => toml::Value::String(text.to_owned()),
            Self::Value(value) => value.clone(),
        }
    }
}

impl<'a> From<&'a toml::Value> for Given<'a> {
    fn from(value: &'a toml::Value) -> Self {
        Self::Value(value)
    }
}

/// A value read as a file's would be, for a kind a type has no quicker read of.
pub fn from_given<P: Property>(given: Given<'_>) -> Result<P> {
    match given {
        Given::Value(value) => P::from_value(value),
        other => P::from_value(&other.to_value()),
    }
}

/// A value a property holds: what its spec says it is, and how it is written
/// in a file and read back.
pub trait Property: Sized {
    /// Write the keys that say what this type is into `spec`: `type`, and
    /// `options`, `of` or `fields` where it holds more. A key already there
    /// was written by the field's `#[property(...)]` and stays.
    fn describe(spec: &mut toml::map::Map<String, toml::Value>);
    fn to_value(&self) -> toml::Value;
    fn from_value(value: &toml::Value) -> Result<Self>;
    /// A value from what a write hands it, which refuses what
    /// [`Property::from_value`] refuses.
    fn from_given(given: Given<'_>) -> Result<Self> {
        from_given(given)
    }
    /// What it holds with nothing built, for a read. `None` for a type with
    /// no such form, which [`Property::to_value`] reads.
    fn given(&self) -> Option<Given<'_>> {
        None
    }
}

/// The properties of a struct, a field each, by their place.
pub trait Fields: Default {
    /// Each property's name in field order: its index is its place here.
    const NAMES: &'static [&'static str];
    /// [`Fields::NAMES`] sorted, each with its index: how a table's keys,
    /// which come sorted, find their fields.
    const BY_NAME: &'static [(&'static str, usize)];
    /// Each property's spec in field order, its `default` included.
    fn specs() -> Vec<toml::Value>;
    /// One property, by its index.
    fn read(&self, index: usize) -> Option<toml::Value>;
    /// Write one property, by its index. A value of the wrong shape is
    /// refused and the field keeps what it held.
    fn write(&mut self, index: usize, value: Given<'_>) -> Result<()>;
    /// One property with nothing built: see [`Property::given`].
    fn given(&self, index: usize) -> Option<Given<'_>>;
}

/// A struct that is a component: [`Fields`] and the name a scene keys it by.
pub trait Component: Fields + Send + Sync + 'static {
    const NAME: &'static str;
    /// What the component gives a node, for the generated reference: the
    /// struct's doc comment.
    const DOC: &'static str;
    /// What it runs as it goes on, is written and comes off: the derive's
    /// `#[component(on_add = ..)]` and the rest.
    const LIFECYCLE: Lifecycle = Lifecycle::NONE;
    /// The components it brings: the derive's `#[component(requires = [..])]`.
    const REQUIRES: &'static [&'static str] = &[];
    /// The facets it is browsed under: `#[component(tags = [..])]`.
    const TAGS: &'static [&'static str] = &[];
    /// Components it needs something from: `#[component(expects = [..])]`.
    const EXPECTS: &'static [&'static str] = &[];
    /// What it announces from its node: `#[component(events = ..)]`.
    const EVENTS: &'static [(&'static str, &'static str)] = &[];
    /// What it does once a write has landed, whole or one property, beyond
    /// holding its values; a refusal puts back what the node held before, the
    /// component's absence included: `#[component(after_write = path)]`.
    const AFTER_WRITE: Option<AfterWriteFn> = None;
    /// What it says is off about it on a node: `#[component(warnings = path)]`.
    const WARNINGS: Option<WarningFn> = None;
}

/// What a component says is off about it on a node.
pub type WarningFn = fn(&Engine, Entity) -> Vec<crate::warnings::Warning>;

/// One lifecycle call: the node, and the engine to read it through.
pub type LifecycleFn = fn(&Engine, Entity);

/// What a component runs at each point of its life on a node (PLAN-ecs §2,
/// Bevy's component hooks), declared on the derive or on a hand-written
/// definition. Every write through the registry runs them, a script's, a
/// scene's and Rust's alike.
///
/// A call never refuses: judging a write is `after_write`'s, which runs
/// first. It never calls a script either: what a script should hear goes
/// through `events::announce`. Each `on_replace` is followed by an
/// `on_insert` or an `on_remove`, a refused write's included, whose `on_insert`
/// sees the values put back.
#[derive(Clone, Copy, Debug, Default)]
pub struct Lifecycle {
    /// It went onto the node, its first values in place.
    pub on_add: Option<LifecycleFn>,
    /// A write landed, whole or one property, the first one included.
    pub on_insert: Option<LifecycleFn>,
    /// What it holds is about to be written over or taken off, still there
    /// to read.
    pub on_replace: Option<LifecycleFn>,
    /// It is about to come off the node, still there to read.
    pub on_remove: Option<LifecycleFn>,
    /// Its node is being freed: before any of the node's `on_replace` and
    /// `on_remove`.
    pub on_despawn: Option<LifecycleFn>,
}

impl Lifecycle {
    /// Nothing at any point.
    pub const NONE: Self = Self {
        on_add: None,
        on_insert: None,
        on_replace: None,
        on_remove: None,
        on_despawn: None,
    };
}

/// What a component does once it is written, beyond holding the values: a
/// system's cache to rebuild, a body to make, a refusal of what the types
/// alone cannot judge.
pub type AfterWriteFn = fn(&Engine, Entity, Written) -> Result<()>;

/// What a write did: wrote the component whole, the first write included, or
/// one property of it, by its index, so a hook can redo only what that
/// property feeds. Going onto the node is the component's `on_add`
/// ([`Lifecycle`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Written {
    Whole,
    Property(usize),
}

/// A table of `rows`, built in one pass.
///
/// Not generic and never inlined: a map collected straight from an iterator
/// carries its own sort, and one per struct was 0.6 MB of a game.
#[inline(never)]
fn table_of(rows: Vec<(String, toml::Value)>) -> toml::map::Map<String, toml::Value> {
    rows.into_iter().collect()
}

/// Every property of `held`, as the table a file holds.
pub fn to_table<F: Fields>(held: &F) -> toml::Value {
    toml::Value::Table(table_of(
        F::BY_NAME
            .iter()
            .filter_map(|&(name, index)| Some((name.to_owned(), held.read(index)?)))
            .collect(),
    ))
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
    // A table's keys come sorted, so each one is found by stepping on from
    // the last: one walk over both. A key out of that order is searched for.
    let mut next = 0;
    for (key, value) in table {
        let rest = &F::BY_NAME[next.min(F::BY_NAME.len())..];
        let found = match rest.iter().position(|(name, _)| *name >= key.as_str()) {
            Some(at) if rest[at].0 == key => Some(next + at),
            _ => F::BY_NAME
                .binary_search_by(|(name, _)| (*name).cmp(key.as_str()))
                .ok(),
        };
        if let Some(at) = found {
            out.write(F::BY_NAME[at].1, value.into())?;
            next = at + 1;
        }
    }
    Ok(out)
}

pub fn describe_record<F: Fields>(spec: &mut toml::map::Map<String, toml::Value>) {
    spec.entry(keys::TYPE)
        .or_insert_with(|| toml::Value::String(words::datatypes::RECORD.into()));
    let fields = F::NAMES
        .iter()
        .zip(F::specs())
        .map(|(name, field)| ((*name).to_owned(), field))
        .collect();
    spec.insert(keys::FIELDS.into(), toml::Value::Table(table_of(fields)));
}

/// A `variant` spec: each of `variants` a record spec, by the name a value's
/// `kind` says.
pub fn describe_variant(
    spec: &mut toml::map::Map<String, toml::Value>,
    variants: toml::map::Map<String, toml::Value>,
) {
    spec.entry(keys::TYPE)
        .or_insert_with(|| toml::Value::String(words::datatypes::VARIANT.into()));
    spec.insert(keys::VARIANTS.into(), toml::Value::Table(variants));
}

/// One variant's record spec, its doc comment as its `description`.
pub fn variant_record<F: Fields>(doc: &str) -> toml::Value {
    let mut spec = toml::map::Map::new();
    describe_record::<F>(&mut spec);
    if !doc.is_empty() {
        spec.insert(
            keys::DESCRIPTION.into(),
            toml::Value::String(doc.to_owned()),
        );
    }
    toml::Value::Table(spec)
}

/// A variant that holds nothing but its name: a record with no fields.
pub fn variant_unit(doc: &str) -> toml::Value {
    let mut spec = toml::map::Map::new();
    spec.insert(
        keys::TYPE.into(),
        toml::Value::String(words::datatypes::RECORD.into()),
    );
    spec.insert(
        keys::FIELDS.into(),
        toml::Value::Table(toml::map::Map::new()),
    );
    if !doc.is_empty() {
        spec.insert(
            keys::DESCRIPTION.into(),
            toml::Value::String(doc.to_owned()),
        );
    }
    toml::Value::Table(spec)
}

/// A variant's value that holds nothing but its `kind`.
pub fn variant_name(kind: &str) -> toml::Value {
    let mut table = toml::map::Map::new();
    table.insert(super::KIND.into(), toml::Value::String(kind.to_owned()));
    toml::Value::Table(table)
}

/// A variant's value: its fields as a table, and the `kind` naming it.
pub fn variant_value<F: Fields>(kind: &str, held: &F) -> toml::Value {
    let mut table = to_table(held);
    if let Some(table) = table.as_table_mut() {
        table.insert(super::KIND.into(), toml::Value::String(kind.to_owned()));
    }
    table
}

/// The variant a value names, or the refusal of a value naming none.
pub fn variant_kind<'a>(value: &'a toml::Value, options: &[&str]) -> Result<&'a str> {
    let kind = value
        .get(super::KIND)
        .and_then(toml::Value::as_str)
        .ok_or_else(|| anyhow!("names no `{}`; one of {}", super::KIND, options.join(", ")))?;
    if options.contains(&kind) {
        Ok(kind)
    } else {
        Err(not_an_option(
            &toml::Value::String(kind.to_owned()),
            options,
        ))
    }
}

pub fn describe_enum(spec: &mut toml::map::Map<String, toml::Value>, options: &[&str]) {
    spec.entry(keys::TYPE)
        .or_insert_with(|| toml::Value::String(words::datatypes::ENUM.into()));
    spec.entry(keys::OPTIONS).or_insert_with(|| {
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
    spec.entry(keys::TYPE)
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

/// An `f32` as the `f64` its shortest decimal spells: `0.15` reads back as
/// 0.15, where widening it gives 0.15000000596046448, so a default and a
/// value read back are what was written.
fn widened(value: f32) -> f64 {
    use std::io::Write as _;
    let wide = f64::from(value);
    if !wide.is_finite() || wide.fract() == 0.0 {
        return wide;
    }
    let mut buffer = [0u8; 32];
    let mut cursor = std::io::Cursor::new(&mut buffer[..]);
    if write!(cursor, "{value}").is_err() {
        return wide;
    }
    let written = usize::try_from(cursor.position()).unwrap_or(0);
    std::str::from_utf8(&buffer[..written])
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(wide)
}

fn floats(values: &[f32]) -> toml::Value {
    toml::Value::Array(
        values
            .iter()
            .map(|value| toml::Value::Float(widened(*value)))
            .collect(),
    )
}

impl Property for f32 {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        set_type(spec, words::datatypes::FLOAT);
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::Float(widened(*self))
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        number(value).map(|number| number as Self)
    }

    fn from_given(given: Given<'_>) -> Result<Self> {
        match given {
            Given::Float(number) => Ok(number as Self),
            other => from_given(other),
        }
    }

    fn given(&self) -> Option<Given<'_>> {
        Some(Given::Float(widened(*self)))
    }
}

impl Property for f64 {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        set_type(spec, words::datatypes::FLOAT);
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::Float(*self)
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        number(value)
    }

    fn from_given(given: Given<'_>) -> Result<Self> {
        match given {
            Given::Float(number) => Ok(number),
            other => from_given(other),
        }
    }

    fn given(&self) -> Option<Given<'_>> {
        Some(Given::Float(*self))
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
                // `32.0` is a whole number too, as an inspector's number field
                // or a hand-written file may spell one.
                let whole = value
                    .as_integer()
                    .or_else(|| {
                        value
                            .as_float()
                            .filter(|number| number.fract() == 0.0 && number.abs() < 9.0e15)
                            .map(|number| number as i64)
                    })
                    .ok_or_else(|| wrong(value, "a whole number"))?;
                <$kind>::try_from(whole)
                    .map_err(|_| anyhow!("is {whole}, which does not fit {}", stringify!($kind)))
            }

            fn from_given(given: Given<'_>) -> Result<Self> {
                match given {
                    Given::Int(whole) => <$kind>::try_from(whole)
                        .map_err(|_| anyhow!("is {whole}, which does not fit {}", stringify!($kind))),
                    other => from_given(other),
                }
            }

            fn given(&self) -> Option<Given<'_>> {
                Some(Given::Int(i64::try_from(*self).unwrap_or(i64::MAX)))
            }
        }
    )*};
}

whole_number!(i32, i64, u8, u16, u32, u64, usize);

impl Property for bool {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        set_type(spec, words::datatypes::BOOL);
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::Boolean(*self)
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        value.as_bool().ok_or_else(|| wrong(value, "true or false"))
    }

    fn from_given(given: Given<'_>) -> Result<Self> {
        match given {
            Given::Bool(flag) => Ok(flag),
            other => from_given(other),
        }
    }

    fn given(&self) -> Option<Given<'_>> {
        Some(Given::Bool(*self))
    }
}

impl Property for String {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        set_type(spec, words::datatypes::STRING);
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::String(self.clone())
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| wrong(value, words::TEXT))
    }

    fn from_given(given: Given<'_>) -> Result<Self> {
        match given {
            Given::Text(text) => Ok(text.to_owned()),
            other => from_given(other),
        }
    }

    fn given(&self) -> Option<Given<'_>> {
        Some(Given::Text(self))
    }
}

impl Property for smol_str::SmolStr {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        set_type(spec, words::datatypes::STRING);
    }

    fn to_value(&self) -> toml::Value {
        toml::Value::String(self.to_string())
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        value
            .as_str()
            .map(Self::new)
            .ok_or_else(|| wrong(value, words::TEXT))
    }

    fn from_given(given: Given<'_>) -> Result<Self> {
        match given {
            Given::Text(text) => Ok(Self::new(text)),
            other => from_given(other),
        }
    }

    fn given(&self) -> Option<Given<'_>> {
        Some(Given::Text(self))
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

            fn from_given(given: Given<'_>) -> Result<Self> {
                match given {
                    Given::Numbers(numbers, $size) => {
                        let mut held = [0.0; $size];
                        held.copy_from_slice(&numbers[..$size]);
                        Ok(Self::from(held))
                    }
                    other => from_given(other),
                }
            }

            fn given(&self) -> Option<Given<'_>> {
                let mut numbers = [0.0; 4];
                numbers[..$size].copy_from_slice(&<[f32; $size]>::from(*self));
                Some(Given::Numbers(numbers, $size))
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
        set_type(spec, words::datatypes::VEC3);
    }

    fn to_value(&self) -> toml::Value {
        let (yaw, pitch, roll) = self.to_euler(glamx::EulerRot::ZYX);
        floats(&[roll, pitch, yaw])
    }

    fn from_value(value: &toml::Value) -> Result<Self> {
        let [x, y, z] = numbers::<3>(value)?;
        Ok(Self::from_euler(glamx::EulerRot::ZYX, z, y, x))
    }

    fn from_given(given: Given<'_>) -> Result<Self> {
        match given {
            Given::Numbers([x, y, z, _], 3) => Ok(Self::from_euler(glamx::EulerRot::ZYX, z, y, x)),
            other => from_given(other),
        }
    }
}

/// A list, or with `flags` on the field a set of an enum's options: the
/// options then belong to the property rather than to what it holds.
impl<T: Property> Property for Vec<T> {
    fn describe(spec: &mut toml::map::Map<String, toml::Value>) {
        let mut held = toml::map::Map::new();
        T::describe(&mut held);
        if spec.get(keys::TYPE).and_then(toml::Value::as_str) == Some(words::datatypes::FLAGS) {
            if let Some(options) = held.remove(keys::OPTIONS) {
                spec.entry(keys::OPTIONS).or_insert(options);
            }
            return;
        }
        set_type(spec, words::datatypes::LIST);
        spec.entry(keys::OF).or_insert(toml::Value::Table(held));
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
        set_type(spec, words::datatypes::MAP);
        spec.entry(keys::OF).or_insert_with(|| {
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
    #[must_use]
    pub fn of<C: Component>() -> Self {
        Self::written_by::<C>(C::AFTER_WRITE)
    }

    /// [`Self::of`], with `after_write` chosen at registration: one definition
    /// serving a family of components, each of its own type.
    #[must_use]
    pub fn written_by<C: Component>(after_write: Option<AfterWriteFn>) -> Self {
        Self {
            schema: schema_of::<C>(),
            doc: C::DOC,
            tags: C::TAGS,
            expects: C::EXPECTS,
            events: C::EVENTS,
            warnings: C::WARNINGS
                .map(|warn| -> Box<dyn Fn(&Engine, Entity) -> _> { Box::new(warn) }),
            lifecycle: C::LIFECYCLE,
            requires: C::REQUIRES,
            type_id: Some(std::any::TypeId::of::<C>()),
            ticks: Some(super::ticks::TickColumn::of::<C>()),
            apply: Box::new(move |eng, entity, params| {
                let next: C = from_table(params)?;
                // What the node held, to put back if the hook refuses: the
                // values before, or no component at all.
                let before = {
                    let mut world = eng.world_mut();
                    let replaced = match world.get::<&mut C>(entity) {
                        Ok(mut held) => Ok(std::mem::replace(&mut *held, next)),
                        Err(_) => Err(next),
                    };
                    match replaced {
                        Ok(before) => Some(before),
                        // With its tick column, in one move to the new archetype.
                        Err(next) => {
                            world
                                .insert(entity, (next, super::ticks::Ticked::<C>::at(0)))
                                .map_err(|_| anyhow!("node is dead"))?;
                            None
                        }
                    }
                };
                let Some(after) = after_write else {
                    return Ok(());
                };
                after(eng, entity, Written::Whole).inspect_err(|_| {
                    let mut world = eng.world_mut();
                    match before {
                        Some(before) => {
                            if let Ok(mut held) = world.get::<&mut C>(entity) {
                                *held = before;
                            }
                        }
                        None => {
                            let _ = world.remove::<(C, super::ticks::Ticked<C>)>(entity);
                        }
                    }
                })
            }),
            remove: Box::new(|eng, entity| {
                // With its tick column in one move; alone where the node never
                // had the column, as one put on around the registry.
                let mut world = eng.world_mut();
                if world
                    .remove::<(C, super::ticks::Ticked<C>)>(entity)
                    .is_err()
                {
                    let _ = world.remove_one::<C>(entity);
                }
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
            read_given: Some(Box::new(|eng, entity, index, out| {
                let world = eng.world();
                let Ok(held) = world.get::<&C>(entity) else {
                    return false;
                };
                let Some(given) = held.given(index) else {
                    return false;
                };
                out(given);
                true
            })),
            write: Some(write_one::<C>(after_write)),
            write_each: Some(write_each::<C>(after_write)),
        }
    }
}

/// One property of `C` written by its index, put back if the hook refuses.
fn write_one<C: Component>(after_write: Option<AfterWriteFn>) -> WriteFn {
    Box::new(move |eng, entity, index, value| {
        let before = {
            let world = eng.world_mut();
            let Ok(mut held) = world.get::<&mut C>(entity) else {
                return Ok(false);
            };
            // Only for a hook to refuse, and a vector's numbers without a
            // value built: a script moving a node writes every frame.
            let before = after_write.map(|_| Held::of(&*held, index));
            held.write(index, value)?;
            before
        };
        let Some(after) = after_write else {
            return Ok(true);
        };
        after(eng, entity, Written::Property(index)).inspect_err(|_| {
            let world = eng.world_mut();
            if let (Ok(mut held), Some(before)) = (world.get::<&mut C>(entity), &before) {
                before.put_back(&mut *held, index);
            }
        })?;
        Ok(true)
    })
}

/// Several properties of `C` written as one write, every one put back if a
/// value or the hook refuses.
fn write_each<C: Component>(after_write: Option<AfterWriteFn>) -> WriteEachFn {
    Box::new(move |eng, entity, each| {
        let put_back = |held: &mut C, before: &[Option<toml::Value>]| {
            for ((index, _), was) in each.iter().zip(before) {
                if let Some(was) = was {
                    let _ = held.write(*index, was.into());
                }
            }
        };
        let before = {
            let world = eng.world_mut();
            let Ok(mut held) = world.get::<&mut C>(entity) else {
                return Ok(false);
            };
            let before: Vec<_> = each.iter().map(|(index, _)| held.read(*index)).collect();
            for (at, (index, value)) in each.iter().enumerate() {
                if let Err(why) = held.write(*index, (*value).into()) {
                    put_back(&mut held, &before[..at]);
                    return Err(why);
                }
            }
            before
        };
        let Some(after) = after_write else {
            return Ok(true);
        };
        after(eng, entity, Written::Whole).inspect_err(|_| {
            let world = eng.world_mut();
            if let Ok(mut held) = world.get::<&mut C>(entity) {
                put_back(&mut held, &before);
            }
        })?;
        Ok(true)
    })
}

/// What one property held before a write a hook may refuse.
enum Held {
    Vector([f32; 4], usize),
    Value(Option<toml::Value>),
}

impl Held {
    fn of<F: Fields>(held: &F, index: usize) -> Self {
        match held.given(index) {
            Some(Given::Numbers(numbers, count)) => Self::Vector(numbers, count),
            _ => Self::Value(held.read(index)),
        }
    }

    fn put_back<F: Fields>(&self, held: &mut F, index: usize) {
        match self {
            Self::Vector(numbers, count) => {
                let _ = held.write(index, Given::Numbers(*numbers, *count));
            }
            Self::Value(Some(was)) => {
                let _ = held.write(index, was.into());
            }
            Self::Value(None) => {}
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

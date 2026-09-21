//! What a component's property table costs to build, three ways.
//!
//! A `get` hook hands back a `toml::Value::Table`, which is a `BTreeMap` with
//! an owned `String` per key and a `Vec` per vector value. Nothing in a
//! component's own state is shaped like that, so every read builds it and
//! every `patch` builds it to throw it away.
//!
//! This prices the alternatives against it without changing anything: the
//! keys a schema already spells as `&'static str` constants, and the values
//! as an enum wide enough to hold a `vec3` inline. A prototype, so the shapes
//! are written out by hand rather than generated.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

/// What a property holds, sized to the widest case a schema declares.
#[derive(Clone, Copy, PartialEq)]
enum PropValue {
    Float(f64),
    Vec3([f32; 3]),
}

/// The transform, as its `get` reports it: three vectors and a float.
const TRANSFORM: [(&str, PropValue); 4] = [
    ("position", PropValue::Vec3([1.0, 2.0, 3.0])),
    ("rotation_euler", PropValue::Vec3([0.0, 0.5, 0.0])),
    ("scale", PropValue::Vec3([1.0, 1.0, 1.0])),
    ("skew", PropValue::Float(0.0)),
];

/// A thirteen-property component, the shape `sprite` and `camera` are.
fn wide() -> Vec<(&'static str, PropValue)> {
    let mut out = Vec::with_capacity(13);
    for (i, name) in [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m",
    ]
    .into_iter()
    .enumerate()
    {
        out.push((name, PropValue::Float(i as f64)));
    }
    out
}

/// The shape a `get` hook builds today.
fn as_toml(props: &[(&'static str, PropValue)]) -> toml::Value {
    let mut out = toml::map::Map::new();
    for (key, value) in props {
        let value = match value {
            PropValue::Float(f) => toml::Value::Float(*f),
            PropValue::Vec3(v) => toml::Value::Array(
                v.iter()
                    .map(|n| toml::Value::Float(f64::from(*n)))
                    .collect(),
            ),
        };
        out.insert((*key).to_string(), value);
    }
    toml::Value::Table(out)
}

/// Half the change: the values unboxed but the keys still owned, to say
/// which of the two the cost is actually in.
fn as_owned_keys(props: &[(&'static str, PropValue)]) -> Vec<(String, PropValue)> {
    props.iter().map(|(k, v)| ((*k).to_string(), *v)).collect()
}

/// Only the keys borrowed, the values left as they are. The shape that
/// survives a `record`: a composite still nests a whole value, and its
/// fields are still named by the schema's own constants.
fn as_borrowed_keys(props: &[(&'static str, PropValue)]) -> Vec<(&'static str, toml::Value)> {
    props
        .iter()
        .map(|(key, value)| {
            let value = match value {
                PropValue::Float(f) => toml::Value::Float(*f),
                PropValue::Vec3(v) => toml::Value::Array(
                    v.iter()
                        .map(|n| toml::Value::Float(f64::from(*n)))
                        .collect(),
                ),
            };
            (*key, value)
        })
        .collect()
}

/// The same table with the keys borrowed and the values unboxed. One
/// allocation for the row list, none for the keys or the vectors.
fn as_rows(props: &[(&'static str, PropValue)]) -> Vec<(&'static str, PropValue)> {
    let mut out = Vec::with_capacity(props.len());
    out.extend_from_slice(props);
    out
}

/// The same again with the rows inline, so a component of eight properties
/// or fewer reaches the allocator not at all.
fn as_inline(
    props: &[(&'static str, PropValue)],
) -> ([Option<(&'static str, PropValue)>; 8], usize) {
    let mut out: [Option<(&'static str, PropValue)>; 8] = [None; 8];
    let n = props.len().min(8);
    for (slot, row) in out.iter_mut().zip(&props[..n]) {
        *slot = Some(*row);
    }
    (out, n)
}

fn build(c: &mut Criterion) {
    let mut group = c.benchmark_group("props_build");
    let wide = wide();
    group.bench_function("toml/4_properties", |b| {
        b.iter(|| as_toml(black_box(&TRANSFORM)));
    });
    group.bench_function("rows/4_properties", |b| {
        b.iter(|| as_rows(black_box(&TRANSFORM)));
    });
    group.bench_function("borrowed_keys/4_properties", |b| {
        b.iter(|| as_borrowed_keys(black_box(&TRANSFORM)));
    });
    group.bench_function("owned_keys/4_properties", |b| {
        b.iter(|| as_owned_keys(black_box(&TRANSFORM)));
    });
    group.bench_function("inline/4_properties", |b| {
        b.iter(|| as_inline(black_box(&TRANSFORM)));
    });
    group.bench_function("toml/13_properties", |b| {
        b.iter(|| as_toml(black_box(&wide)));
    });
    group.bench_function("rows/13_properties", |b| {
        b.iter(|| as_rows(black_box(&wide)));
    });
    group.bench_function("borrowed_keys/13_properties", |b| {
        b.iter(|| as_borrowed_keys(black_box(&wide)));
    });
    group.finish();
}

/// Building the table is only half of it: a caller then wants one key out.
fn build_and_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("props_read_one");
    group.bench_function("toml/4_properties", |b| {
        b.iter(|| {
            as_toml(black_box(&TRANSFORM))
                .get(black_box("scale"))
                .cloned()
        });
    });
    group.bench_function("rows/4_properties", |b| {
        b.iter(|| {
            as_rows(black_box(&TRANSFORM))
                .iter()
                .find(|(k, _)| *k == black_box("scale"))
                .map(|(_, v)| *v)
        });
    });
    group.bench_function("inline/4_properties", |b| {
        b.iter(|| {
            let (rows, n) = as_inline(black_box(&TRANSFORM));
            rows[..n]
                .iter()
                .flatten()
                .find(|(k, _)| *k == black_box("scale"))
                .map(|(_, v)| *v)
        });
    });
    group.finish();
}

criterion_group!(benches, build, build_and_read);
criterion_main!(benches);

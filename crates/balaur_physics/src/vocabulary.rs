//! The words both dimensions use, and the readers that turn them into
//! numbers.
//!
//! rapier2d and rapier3d are separate crates with incompatible types, so the
//! calls into them are written twice (see `dim2`). Everything *around* those
//! calls — schema property text, option lists, options-table reading, result
//! ordering — is `toml` and `Value` and belongs here, written once.

use balaur_core::components::as_f64;
use balaur_script::Value;

/// The closed sets of words a scene file, a script table and the inspector all
/// spell. Written once here so a matcher, a schema's `options` list and the
/// read-back cannot disagree about what a word is.
pub(crate) mod words {
    pub(crate) const DYNAMIC: &str = "dynamic";
    pub(crate) const STATIC: &str = "static";
    pub(crate) const KINEMATIC: &str = "kinematic";
    pub(crate) const KINEMATIC_VELOCITY: &str = "kinematic_velocity";
    /// The body kinds both dimensions accept (N14).
    pub(crate) const BODY_KINDS: &[&str] = &[DYNAMIC, STATIC, KINEMATIC, KINEMATIC_VELOCITY];

    pub(crate) const AVERAGE: &str = "average";
    pub(crate) const MIN: &str = "min";
    pub(crate) const MULTIPLY: &str = "multiply";
    pub(crate) const MAX: &str = "max";
    pub(crate) const CLAMPED_SUM: &str = "clamped_sum";
    pub(crate) const GEOMETRIC_MEAN: &str = "geometric_mean";
    /// How two surfaces' friction or bounciness are combined.
    pub(crate) const COMBINE_RULES: &[&str] =
        &[AVERAGE, MIN, MULTIPLY, MAX, CLAMPED_SUM, GEOMETRIC_MEAN];

    pub(crate) const BALL: &str = "ball";
    pub(crate) const CUBOID: &str = "cuboid";
    pub(crate) const CIRCLE: &str = "circle";
    pub(crate) const RECT: &str = "rect";
    pub(crate) const CAPSULE: &str = "capsule";
    pub(crate) const CYLINDER: &str = "cylinder";
    pub(crate) const CONE: &str = "cone";
    pub(crate) const TRIANGLE: &str = "triangle";
    pub(crate) const SEGMENT: &str = "segment";
    pub(crate) const HALFSPACE: &str = "halfspace";
    pub(crate) const TRIMESH: &str = "trimesh";
    pub(crate) const CONVEX_HULL: &str = "convex_hull";
    pub(crate) const CONVEX_DECOMPOSITION: &str = "convex_decomposition";
    pub(crate) const POLYLINE: &str = "polyline";
    pub(crate) const HEIGHTFIELD: &str = "heightfield";
    pub(crate) const VOXELS: &str = "voxels";
    pub(crate) const VOXELIZED_MESH: &str = "voxelized_mesh";
    pub(crate) const FIT: &str = "fit";
    /// The 3D collider shapes, in the order the inspector offers them.
    pub(crate) const SHAPES: &[&str] = &[
        BALL,
        CUBOID,
        CAPSULE,
        CYLINDER,
        CONE,
        TRIANGLE,
        SEGMENT,
        HALFSPACE,
        TRIMESH,
        CONVEX_HULL,
        CONVEX_DECOMPOSITION,
        POLYLINE,
        HEIGHTFIELD,
        VOXELS,
        VOXELIZED_MESH,
        FIT,
    ];
    /// The 2D shapes. A circle is not a ball and a rect is not a cuboid: the
    /// two worlds name their own shapes.
    pub(crate) const SHAPES_2D: &[&str] = &[
        CIRCLE,
        RECT,
        CAPSULE,
        TRIANGLE,
        SEGMENT,
        HALFSPACE,
        TRIMESH,
        CONVEX_HULL,
        POLYLINE,
        HEIGHTFIELD,
    ];

    pub(crate) const SOLID: &str = "solid";
    pub(crate) const SURFACE: &str = "surface";
    /// Whether voxelizing a mesh fills its inside or only its shell.
    pub(crate) const FILL_MODES: &[&str] = &[SOLID, SURFACE];

    pub(crate) const AABB: &str = "aabb";
    pub(crate) const OBB: &str = "obb";
    /// The shapes a mesh can be fitted to, when a collider's kind is `fit`.
    pub(crate) const FIT_MODES: &[&str] = &[CONVEX_HULL, AABB, OBB, CONVEX_DECOMPOSITION];

    pub(crate) const FIXED: &str = "fixed";
    pub(crate) const REVOLUTE: &str = "revolute";
    pub(crate) const PRISMATIC: &str = "prismatic";
    pub(crate) const SPHERICAL: &str = "spherical";
    pub(crate) const ROPE: &str = "rope";
    pub(crate) const SPRING: &str = "spring";
    pub(crate) const PIN_SLOT: &str = "pin_slot";
    pub(crate) const GENERIC: &str = "generic";
    /// The 3D joints. `spherical` needs three angular axes, so 2D has none.
    pub(crate) const JOINT_KINDS: &[&str] =
        &[FIXED, REVOLUTE, PRISMATIC, SPHERICAL, ROPE, SPRING, GENERIC];
    /// The 2D joints. `pin_slot` is rapier's, and 2D-only.
    pub(crate) const JOINT_KINDS_2D: &[&str] =
        &[FIXED, REVOLUTE, PRISMATIC, ROPE, SPRING, PIN_SLOT, GENERIC];

    pub(crate) const OFF: &str = "off";
    pub(crate) const VELOCITY: &str = "velocity";
    pub(crate) const POSITION: &str = "position";
    /// What a joint's motor drives towards, if anything.
    pub(crate) const MOTOR_MODES: &[&str] = &[OFF, VELOCITY, POSITION];

    pub(crate) const ACCELERATION: &str = "acceleration";
    pub(crate) const FORCE: &str = "force";
    /// Whether a motor's strength ignores mass.
    pub(crate) const MOTOR_MODELS: &[&str] = &[ACCELERATION, FORCE];

    pub(crate) const IMPULSE: &str = "impulse";
    pub(crate) const REDUCED: &str = "reduced";
    /// Which of rapier's two joint sets holds the joint.
    pub(crate) const JOINT_SOLVERS: &[&str] = &[IMPULSE, REDUCED];

    pub(crate) const ABSOLUTE: &str = "absolute";
    pub(crate) const RELATIVE: &str = "relative";
    /// Whether a character's lengths are world units or a fraction of it.
    pub(crate) const LENGTH_MODES: &[&str] = &[ABSOLUTE, RELATIVE];

    pub(crate) const X: &str = "x";
    pub(crate) const Y: &str = "y";
    pub(crate) const Z: &str = "z";
    pub(crate) const ANG_X: &str = "ang_x";
    pub(crate) const ANG_Y: &str = "ang_y";
    pub(crate) const ANG_Z: &str = "ang_z";
    /// The world axes a 3D body may lock, and the 2D pair.
    pub(crate) const LOCK_AXES: &[&str] = &[X, Y, Z];
    pub(crate) const LOCK_AXES_2D: &[&str] = &[X, Y];
    /// The freedoms a generic joint takes away: six in 3D, three in 2D.
    pub(crate) const JOINT_AXES: &[&str] = &[X, Y, Z, ANG_X, ANG_Y, ANG_Z];
    pub(crate) const JOINT_AXES_2D: &[&str] = &[X, Y, ANG_X];

    pub(crate) const COLLISION: &str = "collision";
    pub(crate) const CONTACT_FORCE: &str = "contact_force";

    pub(crate) const DYNAMIC_DYNAMIC: &str = "dynamic_dynamic";
    pub(crate) const DYNAMIC_KINEMATIC: &str = "dynamic_kinematic";
    pub(crate) const DYNAMIC_STATIC: &str = "dynamic_static";
    pub(crate) const KINEMATIC_KINEMATIC: &str = "kinematic_kinematic";
    pub(crate) const KINEMATIC_STATIC: &str = "kinematic_static";
    pub(crate) const STATIC_STATIC: &str = "static_static";
    /// The pairs a collider is tested against unless it asks for more.
    pub(crate) const DEFAULT_COLLISIONS: &[&str] =
        &[DYNAMIC_DYNAMIC, DYNAMIC_KINEMATIC, DYNAMIC_STATIC];
}

/// The words a schema property offers, as its `options` list.
pub(crate) fn options(words: &[&str]) -> String {
    words
        .iter()
        .map(|word| format!("\"{word}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A property of a component's TOML params table, as f32.
pub(crate) fn f(params: &toml::Value, key: &str, default: f32) -> f32 {
    params
        .get(key)
        .and_then(as_f64)
        .map_or(default, |v| v as f32)
}

pub(crate) fn boolean(params: &toml::Value, key: &str, default: bool) -> bool {
    params
        .get(key)
        .and_then(toml::Value::as_bool)
        .unwrap_or(default)
}

pub(crate) fn text<'a>(params: &'a toml::Value, key: &str, default: &'a str) -> &'a str {
    params
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or(default)
}

/// One component of a vector-typed property.
pub(crate) fn axis(params: &toml::Value, key: &str, i: usize, default: f32) -> f32 {
    params
        .get(key)
        .and_then(toml::Value::as_array)
        .and_then(|a| a.get(i))
        .and_then(as_f64)
        .map_or(default, |v| v as f32)
}

pub(crate) fn vec3(params: &toml::Value, key: &str, default: [f32; 3]) -> [f32; 3] {
    [
        axis(params, key, 0, default[0]),
        axis(params, key, 1, default[1]),
        axis(params, key, 2, default[2]),
    ]
}

pub(crate) fn vec2(params: &toml::Value, key: &str, default: [f32; 2]) -> [f32; 2] {
    [
        axis(params, key, 0, default[0]),
        axis(params, key, 1, default[1]),
    ]
}

/// Whether a `flags`-typed property holds `name`.
pub(crate) fn flag(params: &toml::Value, key: &str, name: &str) -> bool {
    balaur_core::components::has_flag(params.get(key), name)
}

/// A trailing options table, as scripts write it (`#{ max = 10.0 }`).
///
/// Every query, joint and character call takes one: they have more parameters
/// than a positional list can carry legibly, and most of them are optional
/// (N9's options-table idiom).
pub(crate) struct Opts<'a>(pub Option<&'a Value>);

impl<'a> Opts<'a> {
    pub(crate) fn get(&self, name: &str) -> Option<&'a Value> {
        match self.0 {
            Some(Value::Map(fields)) => fields.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(crate) fn boolean(&self, name: &str, default: bool) -> bool {
        match self.get(name) {
            Some(Value::Bool(b)) => *b,
            _ => default,
        }
    }

    pub(crate) fn f32(&self, name: &str, default: f32) -> f32 {
        match self.get(name) {
            Some(Value::Num(n)) => *n as f32,
            Some(Value::Int(i)) => *i as f32,
            _ => default,
        }
    }

    pub(crate) fn text(&self, name: &str) -> Option<&'a str> {
        match self.get(name) {
            Some(Value::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// A node argument, as `Value::Node` or as the node id a script holds.
    pub(crate) fn node(&self, name: &str) -> Option<u64> {
        match self.get(name) {
            Some(Value::Node(bits)) => Some(*bits),
            _ => None,
        }
    }

    pub(crate) fn list(&self, name: &str) -> Option<&'a [Value]> {
        match self.get(name) {
            Some(Value::List(items)) => Some(items.as_slice()),
            _ => None,
        }
    }

    /// A vector written as `[x, y, z]`, or the default when absent.
    pub(crate) fn vec3(&self, name: &str, default: [f32; 3]) -> [f32; 3] {
        match self.get(name) {
            Some(Value::Vec3(v)) => *v,
            Some(Value::List(items)) => {
                let at = |i: usize| match items.get(i) {
                    Some(Value::Num(n)) => *n as f32,
                    Some(Value::Int(n)) => *n as f32,
                    _ => default[i],
                };
                [at(0), at(1), at(2)]
            }
            _ => default,
        }
    }

    pub(crate) fn vec2(&self, name: &str, default: [f32; 2]) -> [f32; 2] {
        match self.get(name) {
            Some(Value::Vec2(v)) => *v,
            Some(Value::List(items)) => {
                let at = |i: usize| match items.get(i) {
                    Some(Value::Num(n)) => *n as f32,
                    Some(Value::Int(n)) => *n as f32,
                    _ => default[i],
                };
                [at(0), at(1)]
            }
            _ => default,
        }
    }
}

/// A `Value::Map` from pairs, which is what every query and character call
/// returns.
pub(crate) fn map<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    Value::Map(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

/// The flag tables both dimensions read and write.
///
/// The bit values come from rapier3d's own constants rather than being spelled
/// out here, and rapier2d's identically-named flags take the same bits — the
/// two crates are one source compiled twice. So a name means the same thing in
/// both dimensions by construction, not by a comment asking for it.
pub(crate) mod flags {
    use crate::rapier3d::prelude::{ActiveCollisionTypes, ActiveEvents};

    pub(crate) fn events() -> [(&'static str, u32); 2] {
        [
            (super::words::COLLISION, ActiveEvents::COLLISION_EVENTS.bits()),
            (
                super::words::CONTACT_FORCE,
                ActiveEvents::CONTACT_FORCE_EVENTS.bits(),
            ),
        ]
    }

    pub(crate) fn collision_types() -> [(&'static str, u16); 6] {
        [
            (
                super::words::DYNAMIC_DYNAMIC,
                ActiveCollisionTypes::DYNAMIC_DYNAMIC.bits(),
            ),
            (
                super::words::DYNAMIC_KINEMATIC,
                ActiveCollisionTypes::DYNAMIC_KINEMATIC.bits(),
            ),
            (
                super::words::DYNAMIC_STATIC,
                ActiveCollisionTypes::DYNAMIC_FIXED.bits(),
            ),
            (
                super::words::KINEMATIC_KINEMATIC,
                ActiveCollisionTypes::KINEMATIC_KINEMATIC.bits(),
            ),
            (
                super::words::KINEMATIC_STATIC,
                ActiveCollisionTypes::KINEMATIC_FIXED.bits(),
            ),
            (
                super::words::STATIC_STATIC,
                ActiveCollisionTypes::FIXED_FIXED.bits(),
            ),
        ]
    }
}

/// The bits a `flags` property sets, given the table for that property.
pub(crate) fn bits<T: Copy + std::ops::BitOrAssign + Default>(
    params: &toml::Value,
    key: &str,
    table: &[(&str, T)],
) -> T {
    let mut out = T::default();
    for (name, bit) in table {
        if flag(params, key, name) {
            out |= *bit;
        }
    }
    out
}

/// The names a bit set holds, as a `flags` property's array.
pub(crate) fn names<T: Copy + Into<u32>>(set: T, table: &[(&str, T)]) -> toml::Value {
    let set: u32 = set.into();
    toml::Value::Array(
        table
            .iter()
            .filter(|(_, bit)| {
                let bit: u32 = (*bit).into();
                bit != 0 && set & bit == bit
            })
            .map(|(name, _)| toml::Value::String((*name).to_string()))
            .collect(),
    )
}

/// The 32 collision layers a `flags` property names, as a bit set.
///
/// An empty membership means layer 0 and an empty filter means every layer:
/// the alternative is 32 strings in every scene file that wants the default.
pub(crate) fn layer_bits(params: &toml::Value, key: &str, empty_is_all: bool) -> u32 {
    let mut bits = 0u32;
    for name in balaur_core::components::as_flags(params.get(key)) {
        if let Ok(layer) = name.parse::<u32>() {
            if layer < 32 {
                bits |= 1 << layer;
            }
        }
    }
    if bits != 0 {
        bits
    } else if empty_is_all {
        u32::MAX
    } else {
        1
    }
}

/// A layer set as the numbers a `flags` property holds; every layer reads back
/// as the empty list, which is how the schema spells "everything".
pub(crate) fn layer_names(bits: u32) -> toml::Value {
    if bits == u32::MAX {
        return toml::Value::Array(Vec::new());
    }
    toml::Value::Array(
        (0..32)
            .filter(|bit| bits & (1 << bit) != 0)
            .map(|bit| toml::Value::String(bit.to_string()))
            .collect(),
    )
}

/// The 32 collision layers, as an `options` list for a `flags` property.
/// Numbers rather than names: a name would have to come from the project file,
/// and no other component resolves its options at inspector time.
pub(crate) fn layer_options() -> String {
    (0..32)
        .map(|i| format!("\"{i}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

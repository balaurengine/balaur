//! The `quat` script module: rotations as quaternions `{ x, y, z, w }`, for
//! what a transform's `rotation_euler` cannot compose. Through `glamx`, pinned
//! to `libm`, so every platform turns alike; the Euler order is the one a
//! transform's `rotation_euler` is read in.

use anyhow::{Result, anyhow};
use balaur_script::{Module, Value};
use glamx::{EulerRot, Quat, Vec3};

use crate::engine::Engine;

const RECORD: &str = "Quat";
const X: &str = "x";
const Y: &str = "y";
const Z: &str = "z";
const W: &str = "w";

const IDENTITY: &str = "identity";
const FROM_AXIS_ANGLE: &str = "from_axis_angle";
const FROM_ARC: &str = "from_arc";
const FROM_EULER: &str = "from_euler";
const TO_EULER: &str = "to_euler";
const MUL: &str = "mul";
const INVERSE: &str = "inverse";
const ROTATE: &str = "rotate";

fn quat_of(value: &Value) -> Result<Quat> {
    let Value::Map(fields) = value else {
        return Err(anyhow!("a quaternion is `{{ x, y, z, w }}`, got {value:?}"));
    };
    let part = |name: &str| -> Result<f32> {
        match fields.iter().find(|(key, _)| key == name).map(|(_, v)| v) {
            #[allow(
                clippy::cast_possible_truncation,
                reason = "a quaternion's parts are f32"
            )]
            Some(Value::Num(n)) => Ok(*n as f32),
            #[allow(clippy::cast_precision_loss, reason = "a whole part is small")]
            Some(Value::Int(n)) => Ok(*n as f32),
            other => Err(anyhow!(
                "a quaternion's `{name}` is a number, got {other:?}"
            )),
        }
    };
    Ok(Quat::from_xyzw(part(X)?, part(Y)?, part(Z)?, part(W)?))
}

fn value_of(q: Quat) -> Value {
    Value::Map(
        [(X, q.x), (Y, q.y), (Z, q.z), (W, q.w)]
            .into_iter()
            .map(|(name, part)| (name.to_string(), Value::Num(f64::from(part))))
            .collect(),
    )
}

fn vec3_of(value: &Value) -> Result<Vec3> {
    match value {
        Value::Vec3(v) => Ok(Vec3::from_array(*v)),
        other => Err(anyhow!("a direction is a Vec3, got {other:?}")),
    }
}

/// Declare the module.
pub fn install_quat_api(m: &mut Module<Engine>) {
    m.module_doc(
        "Rotations as quaternions `{ x, y, z, w }`: from an axis and an angle, from one direction to another or from a `rotation_euler`, composed, inverted and applied to a vector. Through `glamx` on `libm`, so every platform turns alike.",
    );
    m.records(&[(
        RECORD,
        "{ x: float, y: float, z: float, w: float }",
        "A rotation as a unit quaternion.",
    )]);
    m.describe(&[
        (IDENTITY, &[], "() -> Quat", "The rotation that turns nothing."),
        (FROM_AXIS_ANGLE, &[], "(axis: Vec3, angle: float) -> Quat", "A turn of `angle` radians about `axis`, which need not be unit length."),
        (FROM_ARC, &[], "(from: Vec3, to: Vec3) -> Quat", "The shortest turn that takes the direction `from` onto `to`."),
        (FROM_EULER, &[], "(euler: Vec3) -> Quat", "A transform's `rotation_euler`, in radians, as a quaternion."),
        (TO_EULER, &[], "(q: Quat) -> Vec3", "A quaternion as the `rotation_euler` a transform takes, in radians."),
        (MUL, &[], "(a: Quat, b: Quat) -> Quat", "`b`, then `a`: the composed rotation, as a child's turn under its parent's is `mul(parent, child)`."),
        (INVERSE, &[], "(q: Quat) -> Quat", "The turn that undoes `q`."),
        (ROTATE, &[], "(q: Quat, v: Vec3) -> Vec3", "`v` turned by `q`."),
    ]);
    m.function(IDENTITY, |_: &Engine, ()| Ok(value_of(Quat::IDENTITY)));
    m.function(
        FROM_AXIS_ANGLE,
        |_: &Engine, (axis, angle): (Value, f64)| {
            #[allow(clippy::cast_possible_truncation, reason = "an angle in f32")]
            let angle = angle as f32;
            Ok(value_of(Quat::from_axis_angle(
                vec3_of(&axis)?.normalize_or(Vec3::Y),
                angle,
            )))
        },
    );
    m.function(FROM_ARC, |_: &Engine, (from, to): (Value, Value)| {
        let (from, to) = (vec3_of(&from)?, vec3_of(&to)?);
        Ok(value_of(Quat::from_rotation_arc(
            from.normalize_or(Vec3::Y),
            to.normalize_or(Vec3::Y),
        )))
    });
    m.function(FROM_EULER, |_: &Engine, euler: Value| {
        let e = vec3_of(&euler)?;
        Ok(value_of(Quat::from_euler(EulerRot::ZYX, e.z, e.y, e.x)))
    });
    m.function(TO_EULER, |_: &Engine, q: Value| {
        let (z, y, x) = quat_of(&q)?.to_euler(EulerRot::ZYX);
        Ok(Value::Vec3([x, y, z]))
    });
    m.function(MUL, |_: &Engine, (a, b): (Value, Value)| {
        Ok(value_of(quat_of(&a)? * quat_of(&b)?))
    });
    m.function(INVERSE, |_: &Engine, q: Value| {
        Ok(value_of(quat_of(&q)?.inverse()))
    });
    m.function(ROTATE, |_: &Engine, (q, v): (Value, Value)| {
        Ok(Value::Vec3((quat_of(&q)? * vec3_of(&v)?).to_array()))
    });
}

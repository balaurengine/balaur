//! The maths types scripts hold, each glam's f64 or i64 type under a
//! script name. Their methods are glam's, bound in `glam_api.rs`; this file
//! keeps the shapes, the conversions and the operators.
//!
//! They are value types, as Godot's are: each answers the fork's `COPY`, so a
//! name bound to one, or a table storing one, holds its own copy.

use anyhow::{anyhow, bail};
use glamx::glam::{DAffine2, DAffine3, DQuat, DVec2, DVec3, DVec4, EulerRot, I64Vec2, I64Vec3};
use rune::alloc::fmt::TryWrite as _;
use rune::runtime::{Formatter, Protocol as P, VmResult};

use super::vm;

/// The value types this module installs, for the API probe: a script reads
/// them as `balaur::Vec2` and the rest.
pub(crate) const VALUE_TYPES: &[&str] = &[
    "Vec2",
    "Vec3",
    "Vec4",
    "IVec2",
    "IVec3",
    "Quat",
    "Transform2d",
    "Transform3d",
    "Color",
];

/// A script type and the glam type it stands for.
pub(crate) trait Glam: Sized {
    type G;
    fn g(&self) -> Self::G;
    fn of(g: Self::G) -> Self;
}

macro_rules! vector {
    ($name:ident, $glam:ty, $n:ty, [$($f:ident),+]) => {
        #[derive(rune::Any, rune::ToConstValue, Clone, Copy, Debug, PartialEq)]
        #[rune(item = ::balaur)]
        pub struct $name {
            $(
                #[rune(get, set)]
                pub $f: $n,
            )+
        }

        impl Glam for $name {
            type G = $glam;
            fn g(&self) -> $glam {
                <$glam>::new($(self.$f),+)
            }
            fn of(g: $glam) -> Self {
                Self { $($f: g.$f),+ }
            }
        }
    };
}

vector!(Vec2, DVec2, f64, [x, y]);
vector!(Vec3, DVec3, f64, [x, y, z]);
vector!(Vec4, DVec4, f64, [x, y, z, w]);
vector!(IVec2, I64Vec2, i64, [x, y]);
vector!(IVec3, I64Vec3, i64, [x, y, z]);

/// A rotation in 3D, glam's `DQuat`.
#[derive(rune::Any, rune::ToConstValue, Clone, Copy, Debug, PartialEq)]
#[rune(item = ::balaur)]
pub struct Quat {
    #[rune(get, set)]
    pub x: f64,
    #[rune(get, set)]
    pub y: f64,
    #[rune(get, set)]
    pub z: f64,
    #[rune(get, set)]
    pub w: f64,
}

impl Glam for Quat {
    type G = DQuat;
    fn g(&self) -> DQuat {
        DQuat::from_xyzw(self.x, self.y, self.z, self.w)
    }
    fn of(g: DQuat) -> Self {
        Self {
            x: g.x,
            y: g.y,
            z: g.z,
            w: g.w,
        }
    }
}

/// An affine transform held as glam's column numbers, so it can be a
/// constant; its axes are read through `x_axis`, `translation` and the rest.
macro_rules! affine {
    ($(#[$doc:meta])* $name:ident, $glam:ty, [$($c:ident),+]) => {
        $(#[$doc])*
        #[derive(rune::Any, rune::ToConstValue, Clone, Copy, Debug, PartialEq)]
        #[rune(item = ::balaur)]
        pub struct $name {
            $($c: f64),+
        }

        impl Glam for $name {
            type G = $glam;
            fn g(&self) -> $glam {
                <$glam>::from_cols_array(&[$(self.$c),+])
            }
            fn of(g: $glam) -> Self {
                let [$($c),+] = g.to_cols_array();
                Self { $($c),+ }
            }
        }
    };
}

affine!(
    /// A 2D affine transform, glam's `DAffine2`.
    Transform2d, DAffine2, [c0, c1, c2, c3, c4, c5]
);
affine!(
    /// A 3D affine transform, glam's `DAffine3`.
    Transform3d, DAffine3, [c0, c1, c2, c3, c4, c5, c6, c7, c8, c9, c10, c11]
);

/// glam's `EulerRot` by its name: `"XYZ"`, `"YXZ"`, `"ZXYEx"` and the rest.
pub(crate) fn euler(name: &str) -> VmResult<EulerRot> {
    use EulerRot as E;
    let order = match name {
        "ZYX" => E::ZYX,
        "ZXY" => E::ZXY,
        "YXZ" => E::YXZ,
        "YZX" => E::YZX,
        "XYZ" => E::XYZ,
        "XZY" => E::XZY,
        "ZYZ" => E::ZYZ,
        "ZXZ" => E::ZXZ,
        "YXY" => E::YXY,
        "YZY" => E::YZY,
        "XYX" => E::XYX,
        "XZX" => E::XZX,
        "ZYXEx" => E::ZYXEx,
        "ZXYEx" => E::ZXYEx,
        "YXZEx" => E::YXZEx,
        "YZXEx" => E::YZXEx,
        "XYZEx" => E::XYZEx,
        "XZYEx" => E::XZYEx,
        "ZYZEx" => E::ZYZEx,
        "ZXZEx" => E::ZXZEx,
        "YXYEx" => E::YXYEx,
        "YZYEx" => E::YZYEx,
        "XYXEx" => E::XYXEx,
        "XZXEx" => E::XZXEx,
        other => return vm(Err(anyhow!("`{other}` is not an Euler order"))),
    };
    VmResult::Ok(order)
}

/// A number an operator scales by: a float, or an int read as one.
#[allow(
    clippy::cast_precision_loss,
    reason = "a script integer used as a scale"
)]
fn number(value: &rune::Value) -> Option<f64> {
    value
        .as_float()
        .ok()
        .or_else(|| value.as_signed().ok().map(|i| i as f64))
}

/// The right side of a float vector's operator: the same type, or a number
/// for every lane.
fn lanes<T: Glam + rune::Any>(value: &rune::Value, splat: fn(f64) -> T::G) -> anyhow::Result<T::G> {
    if let Ok(other) = value.borrow_ref::<T>() {
        return Ok(other.g());
    }
    number(value)
        .map(splat)
        .ok_or_else(|| anyhow!("`{}` is not a number or the same vector", value.type_info()))
}

macro_rules! float_ops {
    ($m:expr, $t:ty, $glam:ty) => {{
        type T = $t;
        let s: fn(f64) -> $glam = <$glam>::splat;
        $m.associated_function(&P::ADD, move |a: &T, b: rune::Value| {
            vm(lanes::<T>(&b, s).map(|b| T::of(a.g() + b)))
        })?;
        $m.associated_function(&P::SUB, move |a: &T, b: rune::Value| {
            vm(lanes::<T>(&b, s).map(|b| T::of(a.g() - b)))
        })?;
        $m.associated_function(&P::MUL, move |a: &T, b: rune::Value| {
            vm(lanes::<T>(&b, s).map(|b| T::of(a.g() * b)))
        })?;
        $m.associated_function(&P::DIV, move |a: &T, b: rune::Value| {
            vm(lanes::<T>(&b, s).map(|b| T::of(a.g() / b)))
        })?;
        $m.associated_function(&P::REM, move |a: &T, b: rune::Value| {
            vm(lanes::<T>(&b, s).map(|b| T::of(a.g() % b)))
        })?;
        $m.associated_function(&P::NEG, |a: &T| T::of(-a.g()))?;
        eq_and_fmt!($m, T);
    }};
}

/// The right side of an int vector's operator.
fn ints<T: Glam + rune::Any>(value: &rune::Value, splat: fn(i64) -> T::G) -> anyhow::Result<T::G> {
    if let Ok(other) = value.borrow_ref::<T>() {
        return Ok(other.g());
    }
    value
        .as_signed()
        .map(splat)
        .map_err(|_| anyhow!("`{}` is not an int or the same vector", value.type_info()))
}

// Wrapping, as Godot's `Vector2i` does, so a debug and a release build
// agree; a zero divisor is an error rather than a panic.
macro_rules! int_ops {
    ($m:expr, $t:ty, $glam:ty) => {{
        type T = $t;
        let s: fn(i64) -> $glam = <$glam>::splat;
        $m.associated_function(&P::ADD, move |a: &T, b: rune::Value| {
            vm(ints::<T>(&b, s).map(|b| T::of(a.g().wrapping_add(b))))
        })?;
        $m.associated_function(&P::SUB, move |a: &T, b: rune::Value| {
            vm(ints::<T>(&b, s).map(|b| T::of(a.g().wrapping_sub(b))))
        })?;
        $m.associated_function(&P::MUL, move |a: &T, b: rune::Value| {
            vm(ints::<T>(&b, s).map(|b| T::of(a.g().wrapping_mul(b))))
        })?;
        $m.associated_function(&P::DIV, move |a: &T, b: rune::Value| {
            vm(ints::<T>(&b, s).and_then(|b| {
                a.g()
                    .checked_div(b)
                    .map(T::of)
                    .ok_or_else(|| anyhow!("division by zero"))
            }))
        })?;
        $m.associated_function(&P::REM, move |a: &T, b: rune::Value| {
            vm(ints::<T>(&b, s).and_then(|b| {
                if b.cmpeq(<$glam>::ZERO).any() {
                    bail!("division by zero");
                }
                Ok(T::of(a.g() % b))
            }))
        })?;
        $m.associated_function(&P::NEG, |a: &T| T::of(<$glam>::ZERO.wrapping_sub(a.g())))?;
        eq_and_fmt!($m, T);
    }};
}

macro_rules! eq_and_fmt {
    ($m:expr, $t:ty) => {{
        #[allow(clippy::float_cmp, reason = "`==` on a vector is exact, as glam's is")]
        fn same(a: &$t, b: &rune::Value) -> bool {
            b.borrow_ref::<$t>().is_ok_and(|b| *b == *a)
        }
        $m.associated_function(&P::PARTIAL_EQ, |a: &$t, b: rune::Value| same(a, &b))?;
        $m.associated_function(&P::EQ, |a: &$t, b: rune::Value| same(a, &b))?;
        $m.associated_function(&P::DISPLAY_FMT, |a: &$t, f: &mut Formatter| {
            rune::vm_write!(f, "{}", a.g())
        })?;
        $m.associated_function(&P::DEBUG_FMT, |a: &$t, f: &mut Formatter| {
            rune::vm_write!(f, "{:?}", a.g())
        })?;
    }};
}

/// `q * other`: a rotation composed, a vector turned, or every lane scaled.
fn quat_mul(q: &Quat, other: &rune::Value) -> anyhow::Result<rune::Value> {
    if let Ok(o) = other.borrow_ref::<Quat>() {
        return Ok(rune::to_value(Quat::of(q.g() * o.g()))?);
    }
    if let Ok(v) = other.borrow_ref::<Vec3>() {
        return Ok(rune::to_value(Vec3::of(q.g() * v.g()))?);
    }
    if let Some(n) = number(other) {
        return Ok(rune::to_value(Quat::of(q.g() * n))?);
    }
    Err(anyhow!(
        "`{}` is not a Quat, a Vec3 or a number",
        other.type_info()
    ))
}

/// `t * other`: two transforms composed, or a point carried through.
macro_rules! transform_ops {
    ($m:expr, $t:ty, $point:ty, $through:ident, $what:literal) => {{
        type T = $t;
        fn mul(t: &T, other: &rune::Value) -> anyhow::Result<rune::Value> {
            if let Ok(o) = other.borrow_ref::<T>() {
                return Ok(rune::to_value(T::of(t.g() * o.g()))?);
            }
            if let Ok(p) = other.borrow_ref::<$point>() {
                return Ok(rune::to_value(<$point>::of(t.g().$through(p.g())))?);
            }
            Err(anyhow!(concat!("`{}` is not ", $what), other.type_info()))
        }
        $m.associated_function(&P::MUL, |t: &T, o: rune::Value| vm(mul(t, &o)))?;
        // glam answers NaN for a transform that flattens space; a script
        // hears why instead.
        $m.associated_function("inverse", |t: &T| {
            let inverse = t.g().inverse();
            if !inverse.is_finite() {
                return vm(Err(anyhow!("this transform has no inverse")));
            }
            VmResult::Ok(T::of(inverse))
        })?;
        eq_and_fmt!($m, T);
    }};
}

pub(crate) fn install(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    m.ty::<Vec2>()?;
    m.ty::<Vec3>()?;
    m.ty::<Vec4>()?;
    m.ty::<IVec2>()?;
    m.ty::<IVec3>()?;
    m.ty::<Quat>()?;
    m.ty::<Transform2d>()?;
    m.ty::<Transform3d>()?;
    float_ops!(m, Vec2, DVec2);
    float_ops!(m, Vec3, DVec3);
    float_ops!(m, Vec4, DVec4);
    int_ops!(m, IVec2, I64Vec2);
    int_ops!(m, IVec3, I64Vec3);
    m.associated_function(&P::MUL, |q: &Quat, o: rune::Value| vm(quat_mul(q, &o)))?;
    m.associated_function(&P::ADD, |a: &Quat, b: &Quat| Quat::of(a.g() + b.g()))?;
    m.associated_function(&P::SUB, |a: &Quat, b: &Quat| Quat::of(a.g() - b.g()))?;
    m.associated_function(&P::DIV, |a: &Quat, n: f64| Quat::of(a.g() / n))?;
    m.associated_function(&P::NEG, |a: &Quat| Quat::of(-a.g()))?;
    eq_and_fmt!(m, Quat);
    transform_ops!(
        m,
        Transform2d,
        Vec2,
        transform_point2,
        "a Transform2d or a Vec2"
    );
    transform_ops!(
        m,
        Transform3d,
        Vec3,
        transform_point3,
        "a Transform3d or a Vec3"
    );
    m.function("new", |x: &Vec2, y: &Vec2, t: &Vec2| {
        Transform2d::of(DAffine2::from_cols(x.g(), y.g(), t.g()))
    })
    .build_associated::<Transform2d>()?;
    m.function("new", |x: &Vec3, y: &Vec3, z: &Vec3, t: &Vec3| {
        Transform3d::of(DAffine3::from_cols(x.g(), y.g(), z.g(), t.g()))
    })
    .build_associated::<Transform3d>()?;
    // Every maths type is a value type.
    copy!(
        m,
        Vec2,
        Vec3,
        Vec4,
        IVec2,
        IVec3,
        Quat,
        Transform2d,
        Transform3d
    );
    transform_fields(m)?;
    m.function("new", |x: f64, y: f64, z: f64, w: f64| Quat { x, y, z, w })
        .build_associated::<Quat>()?;
    Ok(())
}

/// A transform's columns and translation, read and written by name.
fn transform_fields(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    m.field_function(&P::SET, "x_axis", |t: &mut Transform2d, v: &Vec2| {
        let mut g = t.g();
        g.matrix2.x_axis = v.g();
        *t = Transform2d::of(g);
    })?;
    m.field_function(&P::SET, "y_axis", |t: &mut Transform2d, v: &Vec2| {
        let mut g = t.g();
        g.matrix2.y_axis = v.g();
        *t = Transform2d::of(g);
    })?;
    m.field_function(&P::SET, "translation", |t: &mut Transform2d, v: &Vec2| {
        let mut g = t.g();
        g.translation = v.g();
        *t = Transform2d::of(g);
    })?;
    m.field_function(&P::SET, "x_axis", |t: &mut Transform3d, v: &Vec3| {
        let mut g = t.g();
        g.matrix3.x_axis = v.g();
        *t = Transform3d::of(g);
    })?;
    m.field_function(&P::SET, "y_axis", |t: &mut Transform3d, v: &Vec3| {
        let mut g = t.g();
        g.matrix3.y_axis = v.g();
        *t = Transform3d::of(g);
    })?;
    m.field_function(&P::SET, "z_axis", |t: &mut Transform3d, v: &Vec3| {
        let mut g = t.g();
        g.matrix3.z_axis = v.g();
        *t = Transform3d::of(g);
    })?;
    m.field_function(&P::SET, "translation", |t: &mut Transform3d, v: &Vec3| {
        let mut g = t.g();
        g.translation = v.g();
        *t = Transform3d::of(g);
    })?;
    m.field_function(&P::GET, "x_axis", |t: &Transform2d| {
        Vec2::of(t.g().matrix2.x_axis)
    })?;
    m.field_function(&P::GET, "y_axis", |t: &Transform2d| {
        Vec2::of(t.g().matrix2.y_axis)
    })?;
    m.field_function(&P::GET, "translation", |t: &Transform2d| {
        Vec2::of(t.g().translation)
    })?;
    m.field_function(&P::GET, "x_axis", |t: &Transform3d| {
        Vec3::of(t.g().matrix3.x_axis)
    })?;
    m.field_function(&P::GET, "y_axis", |t: &Transform3d| {
        Vec3::of(t.g().matrix3.y_axis)
    })?;
    m.field_function(&P::GET, "z_axis", |t: &Transform3d| {
        Vec3::of(t.g().matrix3.z_axis)
    })?;
    m.field_function(&P::GET, "translation", |t: &Transform3d| {
        Vec3::of(t.g().translation)
    })?;
    Ok(())
}

/// The fork's `COPY`: whatever binds or stores one of these takes this copy.
macro_rules! copy {
    ($m:expr, $($t:ty),+) => {{
        $($m.associated_function(&rune::runtime::Protocol::COPY, |t: &$t| *t)?;)+
    }};
}
pub(crate) use copy;

//! `balaur::Transform2d`: a 2D affine transform, glam's `DAffine2` under the
//! script names glam gives it. Read-only: every method answers a new one.

use super::{Vec2, vm};
use anyhow::{anyhow, bail};
use glamx::glam::{DAffine2, DVec2};

#[derive(rune::Any, Clone, Copy)]
#[rune(item = ::balaur)]
pub struct Transform2d {
    pub(crate) inner: DAffine2,
}

fn d(v: &Vec2) -> DVec2 {
    DVec2::new(v.x, v.y)
}

fn s(v: DVec2) -> Vec2 {
    Vec2 { x: v.x, y: v.y }
}

const fn t(inner: DAffine2) -> Transform2d {
    Transform2d { inner }
}

impl Transform2d {
    /// `t * other`: a point through it, or the two composed.
    fn mul(&self, other: &rune::Value) -> anyhow::Result<rune::Value> {
        if let Ok(p) = other.borrow_ref::<Vec2>() {
            return Ok(rune::to_value(s(self.inner.transform_point2(d(&p))))?);
        }
        if let Ok(o) = other.borrow_ref::<Self>() {
            return Ok(rune::to_value(t(self.inner * o.inner))?);
        }
        Err(anyhow!(
            "`{}` is not a Vec2 or a Transform2d",
            other.type_info()
        ))
    }

    /// glam answers NaN for a transform that flattens the plane; a script
    /// hears why instead.
    fn inverse(&self) -> anyhow::Result<Self> {
        if self.inner.matrix2.determinant() == 0.0 {
            bail!("a Transform2d with a zero determinant has no inverse");
        }
        Ok(t(self.inner.inverse()))
    }

    fn same(&self, other: &rune::Value) -> bool {
        other
            .borrow_ref::<Self>()
            .is_ok_and(|o| o.inner == self.inner)
    }
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    clippy::needless_pass_by_value,
    reason = "associated functions registered with Rune take the receiver by reference"
)]
pub(crate) fn install(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    use rune::runtime::Protocol as P;
    type T = Transform2d;
    m.ty::<T>()?;
    m.function("new", |x: &Vec2, y: &Vec2, translation: &Vec2| {
        t(DAffine2::from_cols(d(x), d(y), d(translation)))
    })
    .build_associated::<T>()?;
    m.function("identity", || t(DAffine2::IDENTITY))
        .build_associated::<T>()?;
    m.function("from_scale", |v: &Vec2| t(DAffine2::from_scale(d(v))))
        .build_associated::<T>()?;
    m.function("from_angle", |a: f64| t(DAffine2::from_angle(a)))
        .build_associated::<T>()?;
    m.function("from_translation", |v: &Vec2| t(DAffine2::from_translation(d(v))))
        .build_associated::<T>()?;
    m.function("from_angle_translation", |a: f64, v: &Vec2| {
        t(DAffine2::from_angle_translation(a, d(v)))
    })
    .build_associated::<T>()?;
    m.function("from_scale_angle_translation", |sc: &Vec2, a: f64, v: &Vec2| {
        t(DAffine2::from_scale_angle_translation(d(sc), a, d(v)))
    })
    .build_associated::<T>()?;
    m.field_function(&P::GET, "x_axis", |x: &T| s(x.inner.matrix2.x_axis))?;
    m.field_function(&P::GET, "y_axis", |x: &T| s(x.inner.matrix2.y_axis))?;
    m.field_function(&P::GET, "translation", |x: &T| s(x.inner.translation))?;
    m.associated_function(&P::MUL, |x: &T, o: rune::Value| vm(x.mul(&o)))?;
    m.associated_function(&P::PARTIAL_EQ, |x: &T, o: rune::Value| x.same(&o))?;
    m.associated_function(&P::EQ, |x: &T, o: rune::Value| x.same(&o))?;
    m.associated_function("transform_point2", |x: &T, p: &Vec2| {
        s(x.inner.transform_point2(d(p)))
    })?;
    m.associated_function("transform_vector2", |x: &T, p: &Vec2| {
        s(x.inner.transform_vector2(d(p)))
    })?;
    m.associated_function("inverse", |x: &T| vm(x.inverse()))?;
    m.associated_function("determinant", |x: &T| x.inner.matrix2.determinant())?;
    // `(scale, angle, translation)`, glam's decomposition: the scale's x
    // carries a mirror's sign.
    m.associated_function("to_scale_angle_translation", |x: &T| {
        let (scale, angle, translation) = x.inner.to_scale_angle_translation();
        (s(scale), angle, s(translation))
    })?;
    m.associated_function("to_cols_array", |x: &T| x.inner.to_cols_array().to_vec())?;
    m.associated_function("is_finite", |x: &T| x.inner.is_finite())?;
    m.associated_function("is_nan", |x: &T| x.inner.is_nan())?;
    m.associated_function("abs_diff_eq", |x: &T, o: &T, most: f64| {
        x.inner.abs_diff_eq(o.inner, most)
    })?;
    Ok(())
}

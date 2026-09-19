//! `balaur::Transform2d`: a 2D affine transform as Godot's `Transform2D`
//! spells one, columns `x`, `y` and `origin`.

use super::{Vec2, vm};
use anyhow::anyhow;

#[derive(rune::Any, Clone, Copy)]
#[rune(item = ::balaur)]
pub struct Transform2d {
    #[rune(get, set, copy)]
    pub x: Vec2,
    #[rune(get, set, copy)]
    pub y: Vec2,
    #[rune(get, set, copy)]
    pub origin: Vec2,
}

const fn v(x: f64, y: f64) -> Vec2 {
    Vec2 { x, y }
}

impl Transform2d {
    const IDENTITY: Self = Self {
        x: v(1.0, 0.0),
        y: v(0.0, 1.0),
        origin: v(0.0, 0.0),
    };

    fn basis_xform(&self, p: Vec2) -> Vec2 {
        v(
            self.x.x * p.x + self.y.x * p.y,
            self.x.y * p.x + self.y.y * p.y,
        )
    }

    fn xform(&self, p: Vec2) -> Vec2 {
        let b = self.basis_xform(p);
        v(b.x + self.origin.x, b.y + self.origin.y)
    }

    fn compose(&self, o: &Self) -> Self {
        Self {
            x: self.basis_xform(o.x),
            y: self.basis_xform(o.y),
            origin: self.xform(o.origin),
        }
    }

    fn determinant(&self) -> f64 {
        self.x.x * self.y.y - self.x.y * self.y.x
    }

    /// The inverse of any invertible transform; a degenerate one answers the
    /// identity, as Godot's refuses to divide by zero.
    fn affine_inverse(&self) -> Self {
        let det = self.determinant();
        if det == 0.0 {
            return Self::IDENTITY;
        }
        let inv = 1.0 / det;
        let x = v(self.y.y * inv, -self.x.y * inv);
        let y = v(-self.y.x * inv, self.x.x * inv);
        let basis = Self {
            x,
            y,
            origin: v(0.0, 0.0),
        };
        let o = basis.basis_xform(self.origin);
        Self {
            x,
            y,
            origin: v(-o.x, -o.y),
        }
    }

    fn rotation(&self) -> f64 {
        balaur_core::libm::atan2(self.x.y, self.x.x)
    }

    fn scale(&self) -> Vec2 {
        let sign = self.determinant().signum();
        v(
            balaur_core::libm::hypot(self.x.x, self.x.y),
            sign * balaur_core::libm::hypot(self.y.x, self.y.y),
        )
    }

    fn from_parts(rotation: f64, scale: Vec2, origin: Vec2) -> Self {
        let (s, c) = (
            balaur_core::libm::sin(rotation),
            balaur_core::libm::cos(rotation),
        );
        Self {
            x: v(c * scale.x, s * scale.x),
            y: v(-s * scale.y, c * scale.y),
            origin,
        }
    }

    /// `t * other`: a point through it, or the two composed.
    fn mul(&self, other: &rune::Value) -> anyhow::Result<rune::Value> {
        if let Ok(p) = other.borrow_ref::<Vec2>() {
            return Ok(rune::to_value(self.xform(*p))?);
        }
        if let Ok(t) = other.borrow_ref::<Self>() {
            return Ok(rune::to_value(self.compose(&t))?);
        }
        Err(anyhow!(
            "`{}` is not a Vec2 or a Transform2d",
            other.type_info()
        ))
    }
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "associated functions registered with Rune take the receiver by reference"
)]
pub(crate) fn install(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    use rune::runtime::Protocol as P;
    m.ty::<Transform2d>()?;
    m.function("new", |x: &Vec2, y: &Vec2, origin: &Vec2| Transform2d {
        x: *x,
        y: *y,
        origin: *origin,
    })
    .build_associated::<Transform2d>()?;
    m.function("identity", || Transform2d::IDENTITY)
        .build_associated::<Transform2d>()?;
    m.function("from_parts", |turn: f64, scale: &Vec2, origin: &Vec2| {
        Transform2d::from_parts(turn, *scale, *origin)
    })
    .build_associated::<Transform2d>()?;
    m.associated_function(&P::MUL, |t: &Transform2d, o: rune::Value| vm(t.mul(&o)))?;
    m.associated_function("xform", |t: &Transform2d, p: &Vec2| t.xform(*p))?;
    m.associated_function("basis_xform", |t: &Transform2d, p: &Vec2| t.basis_xform(*p))?;
    m.associated_function("affine_inverse", |t: &Transform2d| t.affine_inverse())?;
    m.associated_function("inverse", |t: &Transform2d| t.affine_inverse())?;
    m.associated_function("get_origin", |t: &Transform2d| t.origin)?;
    m.associated_function("get_rotation", |t: &Transform2d| t.rotation())?;
    m.associated_function("get_scale", |t: &Transform2d| t.scale())?;
    m.associated_function("translated", |t: &Transform2d, by: &Vec2| Transform2d {
        origin: v(t.origin.x + by.x, t.origin.y + by.y),
        ..*t
    })?;
    Ok(())
}

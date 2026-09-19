//! Written by `scripts/gen_glam_api.py` from glam's source: every method
//! glam gives these types, under glam's own name. Edit the script, not this.

#![allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "one generated registration per glam method"
)]

use glamx::glam::{DAffine2, DAffine3, DQuat, DVec2, DVec3, DVec4, I64Vec2, I64Vec3};
use rune::runtime::VmResult;
use rune::vm_try;

use super::glam_types::{Glam as _, IVec2, IVec3, Quat, Transform2d, Transform3d, Vec4, euler as euler_order};
use super::{Vec2, Vec3};

pub(crate) fn install(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    vec2_1(m)?;
    vec2_2(m)?;
    vec3_1(m)?;
    vec3_2(m)?;
    vec4_1(m)?;
    vec4_2(m)?;
    quat_1(m)?;
    transform_2d_1(m)?;
    transform_3d_1(m)?;
    ivec2_1(m)?;
    ivec3_1(m)?;
    Ok(())
}

fn vec2_1(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    m.constant("ZERO", Vec2::of(DVec2::ZERO))
        .build_associated::<Vec2>()?;
    m.constant("ONE", Vec2::of(DVec2::ONE))
        .build_associated::<Vec2>()?;
    m.constant("NEG_ONE", Vec2::of(DVec2::NEG_ONE))
        .build_associated::<Vec2>()?;
    m.constant("MIN", Vec2::of(DVec2::MIN))
        .build_associated::<Vec2>()?;
    m.constant("MAX", Vec2::of(DVec2::MAX))
        .build_associated::<Vec2>()?;
    m.constant("NAN", Vec2::of(DVec2::NAN))
        .build_associated::<Vec2>()?;
    m.constant("INFINITY", Vec2::of(DVec2::INFINITY))
        .build_associated::<Vec2>()?;
    m.constant("NEG_INFINITY", Vec2::of(DVec2::NEG_INFINITY))
        .build_associated::<Vec2>()?;
    m.constant("X", Vec2::of(DVec2::X))
        .build_associated::<Vec2>()?;
    m.constant("Y", Vec2::of(DVec2::Y))
        .build_associated::<Vec2>()?;
    m.constant("NEG_X", Vec2::of(DVec2::NEG_X))
        .build_associated::<Vec2>()?;
    m.constant("NEG_Y", Vec2::of(DVec2::NEG_Y))
        .build_associated::<Vec2>()?;
    m.function("new", |x: f64, y: f64| -> Vec2 { Vec2::of(DVec2::new(x, y)) })
        .build_associated::<Vec2>()?;
    m.function("splat", |v: f64| -> Vec2 { Vec2::of(DVec2::splat(v)) })
        .build_associated::<Vec2>()?;
    m.associated_function("to_array", |this: &Vec2| -> Vec<f64> { this.g().to_array().to_vec() })?;
    m.associated_function("extend", |this: &Vec2, z: f64| -> Vec3 { Vec3::of(this.g().extend(z)) })?;
    m.associated_function("with_x", |this: &Vec2, x: f64| -> Vec2 { Vec2::of(this.g().with_x(x)) })?;
    m.associated_function("with_y", |this: &Vec2, y: f64| -> Vec2 { Vec2::of(this.g().with_y(y)) })?;
    m.associated_function("dot", |this: &Vec2, rhs: &Vec2| -> f64 { this.g().dot(rhs.g()) })?;
    m.associated_function("dot_into_vec", |this: &Vec2, rhs: &Vec2| -> Vec2 { Vec2::of(this.g().dot_into_vec(rhs.g())) })?;
    m.associated_function("min", |this: &Vec2, rhs: &Vec2| -> Vec2 { Vec2::of(this.g().min(rhs.g())) })?;
    m.associated_function("max", |this: &Vec2, rhs: &Vec2| -> Vec2 { Vec2::of(this.g().max(rhs.g())) })?;
    m.associated_function("clamp", |this: &Vec2, min: &Vec2, max: &Vec2| -> Vec2 { Vec2::of(this.g().clamp(min.g(), max.g())) })?;
    m.associated_function("min_element", |this: &Vec2| -> f64 { this.g().min_element() })?;
    m.associated_function("max_element", |this: &Vec2| -> f64 { this.g().max_element() })?;
    m.associated_function("min_position", |this: &Vec2| -> i64 { i64::try_from(this.g().min_position()).unwrap_or(i64::MAX) })?;
    m.associated_function("max_position", |this: &Vec2| -> i64 { i64::try_from(this.g().max_position()).unwrap_or(i64::MAX) })?;
    m.associated_function("element_sum", |this: &Vec2| -> f64 { this.g().element_sum() })?;
    m.associated_function("element_product", |this: &Vec2| -> f64 { this.g().element_product() })?;
    m.associated_function("abs", |this: &Vec2| -> Vec2 { Vec2::of(this.g().abs()) })?;
    m.associated_function("signum", |this: &Vec2| -> Vec2 { Vec2::of(this.g().signum()) })?;
    m.associated_function("copysign", |this: &Vec2, rhs: &Vec2| -> Vec2 { Vec2::of(this.g().copysign(rhs.g())) })?;
    m.associated_function("is_finite", |this: &Vec2| -> bool { this.g().is_finite() })?;
    m.associated_function("is_nan", |this: &Vec2| -> bool { this.g().is_nan() })?;
    m.associated_function("length", |this: &Vec2| -> f64 { this.g().length() })?;
    m.associated_function("length_squared", |this: &Vec2| -> f64 { this.g().length_squared() })?;
    m.associated_function("length_recip", |this: &Vec2| -> f64 { this.g().length_recip() })?;
    m.associated_function("distance", |this: &Vec2, rhs: &Vec2| -> f64 { this.g().distance(rhs.g()) })?;
    m.associated_function("distance_squared", |this: &Vec2, rhs: &Vec2| -> f64 { this.g().distance_squared(rhs.g()) })?;
    m.associated_function("div_euclid", |this: &Vec2, rhs: &Vec2| -> Vec2 { Vec2::of(this.g().div_euclid(rhs.g())) })?;
    m.associated_function("rem_euclid", |this: &Vec2, rhs: &Vec2| -> Vec2 { Vec2::of(this.g().rem_euclid(rhs.g())) })?;
    m.associated_function("normalize", |this: &Vec2| -> Vec2 { Vec2::of(this.g().normalize()) })?;
    m.associated_function("try_normalize", |this: &Vec2| -> Option<Vec2> { this.g().try_normalize().map(Vec2::of) })?;
    m.associated_function("normalize_or", |this: &Vec2, fallback: &Vec2| -> Vec2 { Vec2::of(this.g().normalize_or(fallback.g())) })?;
    m.associated_function("normalize_or_zero", |this: &Vec2| -> Vec2 { Vec2::of(this.g().normalize_or_zero()) })?;
    m.associated_function("normalize_and_length", |this: &Vec2| -> (Vec2, f64) { { let r = this.g().normalize_and_length(); (Vec2::of(r.0), r.1) } })?;
    m.associated_function("is_normalized", |this: &Vec2| -> bool { this.g().is_normalized() })?;
    m.associated_function("project_onto", |this: &Vec2, rhs: &Vec2| -> Vec2 { Vec2::of(this.g().project_onto(rhs.g())) })?;
    m.associated_function("reject_from", |this: &Vec2, rhs: &Vec2| -> Vec2 { Vec2::of(this.g().reject_from(rhs.g())) })?;
    m.associated_function("project_onto_normalized", |this: &Vec2, rhs: &Vec2| -> Vec2 { Vec2::of(this.g().project_onto_normalized(rhs.g())) })?;
    m.associated_function("reject_from_normalized", |this: &Vec2, rhs: &Vec2| -> Vec2 { Vec2::of(this.g().reject_from_normalized(rhs.g())) })?;
    m.associated_function("round", |this: &Vec2| -> Vec2 { Vec2::of(this.g().round()) })?;
    m.associated_function("floor", |this: &Vec2| -> Vec2 { Vec2::of(this.g().floor()) })?;
    m.associated_function("ceil", |this: &Vec2| -> Vec2 { Vec2::of(this.g().ceil()) })?;
    m.associated_function("trunc", |this: &Vec2| -> Vec2 { Vec2::of(this.g().trunc()) })?;
    m.associated_function("step", |this: &Vec2, rhs: &Vec2| -> Vec2 { Vec2::of(this.g().step(rhs.g())) })?;
    m.associated_function("smoothstep", |this: &Vec2, edge0: &Vec2, edge1: &Vec2| -> Vec2 { Vec2::of(this.g().smoothstep(edge0.g(), edge1.g())) })?;
    m.associated_function("saturate", |this: &Vec2| -> Vec2 { Vec2::of(this.g().saturate()) })?;
    m.associated_function("fract", |this: &Vec2| -> Vec2 { Vec2::of(this.g().fract()) })?;
    m.associated_function("fract_gl", |this: &Vec2| -> Vec2 { Vec2::of(this.g().fract_gl()) })?;
    m.associated_function("exp", |this: &Vec2| -> Vec2 { Vec2::of(this.g().exp()) })?;
    m.associated_function("exp2", |this: &Vec2| -> Vec2 { Vec2::of(this.g().exp2()) })?;
    m.associated_function("ln", |this: &Vec2| -> Vec2 { Vec2::of(this.g().ln()) })?;
    m.associated_function("log2", |this: &Vec2| -> Vec2 { Vec2::of(this.g().log2()) })?;
    m.associated_function("powf", |this: &Vec2, n: f64| -> Vec2 { Vec2::of(this.g().powf(n)) })?;
    m.associated_function("sqrt", |this: &Vec2| -> Vec2 { Vec2::of(this.g().sqrt()) })?;
    m.associated_function("cos", |this: &Vec2| -> Vec2 { Vec2::of(this.g().cos()) })?;
    m.associated_function("sin", |this: &Vec2| -> Vec2 { Vec2::of(this.g().sin()) })?;
    m.associated_function("sin_cos", |this: &Vec2| -> (Vec2, Vec2) { { let r = this.g().sin_cos(); (Vec2::of(r.0), Vec2::of(r.1)) } })?;
    m.associated_function("recip", |this: &Vec2| -> Vec2 { Vec2::of(this.g().recip()) })?;
    m.associated_function("lerp", |this: &Vec2, rhs: &Vec2, s: f64| -> Vec2 { Vec2::of(this.g().lerp(rhs.g(), s)) })?;
    m.associated_function("move_towards", |this: &Vec2, rhs: &Vec2, d: f64| -> Vec2 { Vec2::of(this.g().move_towards(rhs.g(), d)) })?;
    m.associated_function("midpoint", |this: &Vec2, rhs: &Vec2| -> Vec2 { Vec2::of(this.g().midpoint(rhs.g())) })?;
    m.associated_function("abs_diff_eq", |this: &Vec2, rhs: &Vec2, max_abs_diff: f64| -> bool { this.g().abs_diff_eq(rhs.g(), max_abs_diff) })?;
    m.associated_function("clamp_length", |this: &Vec2, min: f64, max: f64| -> Vec2 { Vec2::of(this.g().clamp_length(min, max)) })?;
    m.associated_function("clamp_length_max", |this: &Vec2, max: f64| -> Vec2 { Vec2::of(this.g().clamp_length_max(max)) })?;
    Ok(())
}

fn vec2_2(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    m.associated_function("clamp_length_min", |this: &Vec2, min: f64| -> Vec2 { Vec2::of(this.g().clamp_length_min(min)) })?;
    m.associated_function("mul_add", |this: &Vec2, a: &Vec2, b: &Vec2| -> Vec2 { Vec2::of(this.g().mul_add(a.g(), b.g())) })?;
    m.associated_function("reflect", |this: &Vec2, normal: &Vec2| -> Vec2 { Vec2::of(this.g().reflect(normal.g())) })?;
    m.associated_function("refract", |this: &Vec2, normal: &Vec2, eta: f64| -> Vec2 { Vec2::of(this.g().refract(normal.g(), eta)) })?;
    m.function("from_angle", |angle: f64| -> Vec2 { Vec2::of(DVec2::from_angle(angle)) })
        .build_associated::<Vec2>()?;
    m.associated_function("to_angle", |this: &Vec2| -> f64 { this.g().to_angle() })?;
    m.associated_function("angle_to", |this: &Vec2, rhs: &Vec2| -> f64 { this.g().angle_to(rhs.g()) })?;
    m.associated_function("perp", |this: &Vec2| -> Vec2 { Vec2::of(this.g().perp()) })?;
    m.associated_function("perp_dot", |this: &Vec2, rhs: &Vec2| -> f64 { this.g().perp_dot(rhs.g()) })?;
    m.associated_function("rotate", |this: &Vec2, rhs: &Vec2| -> Vec2 { Vec2::of(this.g().rotate(rhs.g())) })?;
    m.associated_function("rotate_angle", |this: &Vec2, angle: f64| -> Vec2 { Vec2::of(this.g().rotate_angle(angle)) })?;
    m.associated_function("rotate_towards", |this: &Vec2, rhs: &Vec2, max_angle: f64| -> Vec2 { Vec2::of(this.g().rotate_towards(rhs.g(), max_angle)) })?;
    m.associated_function("as_ivec2", |this: &Vec2| -> IVec2 { IVec2::of(this.g().as_i64vec2()) })?;
    Ok(())
}

fn vec3_1(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    m.constant("ZERO", Vec3::of(DVec3::ZERO))
        .build_associated::<Vec3>()?;
    m.constant("ONE", Vec3::of(DVec3::ONE))
        .build_associated::<Vec3>()?;
    m.constant("NEG_ONE", Vec3::of(DVec3::NEG_ONE))
        .build_associated::<Vec3>()?;
    m.constant("MIN", Vec3::of(DVec3::MIN))
        .build_associated::<Vec3>()?;
    m.constant("MAX", Vec3::of(DVec3::MAX))
        .build_associated::<Vec3>()?;
    m.constant("NAN", Vec3::of(DVec3::NAN))
        .build_associated::<Vec3>()?;
    m.constant("INFINITY", Vec3::of(DVec3::INFINITY))
        .build_associated::<Vec3>()?;
    m.constant("NEG_INFINITY", Vec3::of(DVec3::NEG_INFINITY))
        .build_associated::<Vec3>()?;
    m.constant("X", Vec3::of(DVec3::X))
        .build_associated::<Vec3>()?;
    m.constant("Y", Vec3::of(DVec3::Y))
        .build_associated::<Vec3>()?;
    m.constant("Z", Vec3::of(DVec3::Z))
        .build_associated::<Vec3>()?;
    m.constant("NEG_X", Vec3::of(DVec3::NEG_X))
        .build_associated::<Vec3>()?;
    m.constant("NEG_Y", Vec3::of(DVec3::NEG_Y))
        .build_associated::<Vec3>()?;
    m.constant("NEG_Z", Vec3::of(DVec3::NEG_Z))
        .build_associated::<Vec3>()?;
    m.function("new", |x: f64, y: f64, z: f64| -> Vec3 { Vec3::of(DVec3::new(x, y, z)) })
        .build_associated::<Vec3>()?;
    m.function("splat", |v: f64| -> Vec3 { Vec3::of(DVec3::splat(v)) })
        .build_associated::<Vec3>()?;
    m.associated_function("to_array", |this: &Vec3| -> Vec<f64> { this.g().to_array().to_vec() })?;
    m.associated_function("extend", |this: &Vec3, w: f64| -> Vec4 { Vec4::of(this.g().extend(w)) })?;
    m.associated_function("truncate", |this: &Vec3| -> Vec2 { Vec2::of(this.g().truncate()) })?;
    m.function("from_homogeneous", |v: &Vec4| -> Vec3 { Vec3::of(DVec3::from_homogeneous(v.g())) })
        .build_associated::<Vec3>()?;
    m.associated_function("to_homogeneous", |this: &Vec3| -> Vec4 { Vec4::of(this.g().to_homogeneous()) })?;
    m.associated_function("with_x", |this: &Vec3, x: f64| -> Vec3 { Vec3::of(this.g().with_x(x)) })?;
    m.associated_function("with_y", |this: &Vec3, y: f64| -> Vec3 { Vec3::of(this.g().with_y(y)) })?;
    m.associated_function("with_z", |this: &Vec3, z: f64| -> Vec3 { Vec3::of(this.g().with_z(z)) })?;
    m.associated_function("dot", |this: &Vec3, rhs: &Vec3| -> f64 { this.g().dot(rhs.g()) })?;
    m.associated_function("dot_into_vec", |this: &Vec3, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().dot_into_vec(rhs.g())) })?;
    m.associated_function("cross", |this: &Vec3, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().cross(rhs.g())) })?;
    m.associated_function("min", |this: &Vec3, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().min(rhs.g())) })?;
    m.associated_function("max", |this: &Vec3, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().max(rhs.g())) })?;
    m.associated_function("clamp", |this: &Vec3, min: &Vec3, max: &Vec3| -> Vec3 { Vec3::of(this.g().clamp(min.g(), max.g())) })?;
    m.associated_function("min_element", |this: &Vec3| -> f64 { this.g().min_element() })?;
    m.associated_function("max_element", |this: &Vec3| -> f64 { this.g().max_element() })?;
    m.associated_function("min_position", |this: &Vec3| -> i64 { i64::try_from(this.g().min_position()).unwrap_or(i64::MAX) })?;
    m.associated_function("max_position", |this: &Vec3| -> i64 { i64::try_from(this.g().max_position()).unwrap_or(i64::MAX) })?;
    m.associated_function("element_sum", |this: &Vec3| -> f64 { this.g().element_sum() })?;
    m.associated_function("element_product", |this: &Vec3| -> f64 { this.g().element_product() })?;
    m.associated_function("abs", |this: &Vec3| -> Vec3 { Vec3::of(this.g().abs()) })?;
    m.associated_function("signum", |this: &Vec3| -> Vec3 { Vec3::of(this.g().signum()) })?;
    m.associated_function("copysign", |this: &Vec3, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().copysign(rhs.g())) })?;
    m.associated_function("is_finite", |this: &Vec3| -> bool { this.g().is_finite() })?;
    m.associated_function("is_nan", |this: &Vec3| -> bool { this.g().is_nan() })?;
    m.associated_function("length", |this: &Vec3| -> f64 { this.g().length() })?;
    m.associated_function("length_squared", |this: &Vec3| -> f64 { this.g().length_squared() })?;
    m.associated_function("length_recip", |this: &Vec3| -> f64 { this.g().length_recip() })?;
    m.associated_function("distance", |this: &Vec3, rhs: &Vec3| -> f64 { this.g().distance(rhs.g()) })?;
    m.associated_function("distance_squared", |this: &Vec3, rhs: &Vec3| -> f64 { this.g().distance_squared(rhs.g()) })?;
    m.associated_function("div_euclid", |this: &Vec3, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().div_euclid(rhs.g())) })?;
    m.associated_function("rem_euclid", |this: &Vec3, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().rem_euclid(rhs.g())) })?;
    m.associated_function("normalize", |this: &Vec3| -> Vec3 { Vec3::of(this.g().normalize()) })?;
    m.associated_function("try_normalize", |this: &Vec3| -> Option<Vec3> { this.g().try_normalize().map(Vec3::of) })?;
    m.associated_function("normalize_or", |this: &Vec3, fallback: &Vec3| -> Vec3 { Vec3::of(this.g().normalize_or(fallback.g())) })?;
    m.associated_function("normalize_or_zero", |this: &Vec3| -> Vec3 { Vec3::of(this.g().normalize_or_zero()) })?;
    m.associated_function("normalize_and_length", |this: &Vec3| -> (Vec3, f64) { { let r = this.g().normalize_and_length(); (Vec3::of(r.0), r.1) } })?;
    m.associated_function("is_normalized", |this: &Vec3| -> bool { this.g().is_normalized() })?;
    m.associated_function("project_onto", |this: &Vec3, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().project_onto(rhs.g())) })?;
    m.associated_function("reject_from", |this: &Vec3, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().reject_from(rhs.g())) })?;
    m.associated_function("project_onto_normalized", |this: &Vec3, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().project_onto_normalized(rhs.g())) })?;
    m.associated_function("reject_from_normalized", |this: &Vec3, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().reject_from_normalized(rhs.g())) })?;
    m.associated_function("round", |this: &Vec3| -> Vec3 { Vec3::of(this.g().round()) })?;
    m.associated_function("floor", |this: &Vec3| -> Vec3 { Vec3::of(this.g().floor()) })?;
    m.associated_function("ceil", |this: &Vec3| -> Vec3 { Vec3::of(this.g().ceil()) })?;
    m.associated_function("trunc", |this: &Vec3| -> Vec3 { Vec3::of(this.g().trunc()) })?;
    m.associated_function("step", |this: &Vec3, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().step(rhs.g())) })?;
    m.associated_function("smoothstep", |this: &Vec3, edge0: &Vec3, edge1: &Vec3| -> Vec3 { Vec3::of(this.g().smoothstep(edge0.g(), edge1.g())) })?;
    m.associated_function("saturate", |this: &Vec3| -> Vec3 { Vec3::of(this.g().saturate()) })?;
    m.associated_function("fract", |this: &Vec3| -> Vec3 { Vec3::of(this.g().fract()) })?;
    m.associated_function("fract_gl", |this: &Vec3| -> Vec3 { Vec3::of(this.g().fract_gl()) })?;
    m.associated_function("exp", |this: &Vec3| -> Vec3 { Vec3::of(this.g().exp()) })?;
    m.associated_function("exp2", |this: &Vec3| -> Vec3 { Vec3::of(this.g().exp2()) })?;
    m.associated_function("ln", |this: &Vec3| -> Vec3 { Vec3::of(this.g().ln()) })?;
    m.associated_function("log2", |this: &Vec3| -> Vec3 { Vec3::of(this.g().log2()) })?;
    m.associated_function("powf", |this: &Vec3, n: f64| -> Vec3 { Vec3::of(this.g().powf(n)) })?;
    m.associated_function("sqrt", |this: &Vec3| -> Vec3 { Vec3::of(this.g().sqrt()) })?;
    Ok(())
}

fn vec3_2(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    m.associated_function("cos", |this: &Vec3| -> Vec3 { Vec3::of(this.g().cos()) })?;
    m.associated_function("sin", |this: &Vec3| -> Vec3 { Vec3::of(this.g().sin()) })?;
    m.associated_function("sin_cos", |this: &Vec3| -> (Vec3, Vec3) { { let r = this.g().sin_cos(); (Vec3::of(r.0), Vec3::of(r.1)) } })?;
    m.associated_function("recip", |this: &Vec3| -> Vec3 { Vec3::of(this.g().recip()) })?;
    m.associated_function("lerp", |this: &Vec3, rhs: &Vec3, s: f64| -> Vec3 { Vec3::of(this.g().lerp(rhs.g(), s)) })?;
    m.associated_function("move_towards", |this: &Vec3, rhs: &Vec3, d: f64| -> Vec3 { Vec3::of(this.g().move_towards(rhs.g(), d)) })?;
    m.associated_function("midpoint", |this: &Vec3, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().midpoint(rhs.g())) })?;
    m.associated_function("abs_diff_eq", |this: &Vec3, rhs: &Vec3, max_abs_diff: f64| -> bool { this.g().abs_diff_eq(rhs.g(), max_abs_diff) })?;
    m.associated_function("clamp_length", |this: &Vec3, min: f64, max: f64| -> Vec3 { Vec3::of(this.g().clamp_length(min, max)) })?;
    m.associated_function("clamp_length_max", |this: &Vec3, max: f64| -> Vec3 { Vec3::of(this.g().clamp_length_max(max)) })?;
    m.associated_function("clamp_length_min", |this: &Vec3, min: f64| -> Vec3 { Vec3::of(this.g().clamp_length_min(min)) })?;
    m.associated_function("mul_add", |this: &Vec3, a: &Vec3, b: &Vec3| -> Vec3 { Vec3::of(this.g().mul_add(a.g(), b.g())) })?;
    m.associated_function("reflect", |this: &Vec3, normal: &Vec3| -> Vec3 { Vec3::of(this.g().reflect(normal.g())) })?;
    m.associated_function("refract", |this: &Vec3, normal: &Vec3, eta: f64| -> Vec3 { Vec3::of(this.g().refract(normal.g(), eta)) })?;
    m.associated_function("angle_between", |this: &Vec3, rhs: &Vec3| -> f64 { this.g().angle_between(rhs.g()) })?;
    m.associated_function("angle_to", |this: &Vec3, rhs: &Vec3, axis: &Vec3| -> f64 { this.g().angle_to(rhs.g(), axis.g()) })?;
    m.associated_function("rotate_x", |this: &Vec3, angle: f64| -> Vec3 { Vec3::of(this.g().rotate_x(angle)) })?;
    m.associated_function("rotate_y", |this: &Vec3, angle: f64| -> Vec3 { Vec3::of(this.g().rotate_y(angle)) })?;
    m.associated_function("rotate_z", |this: &Vec3, angle: f64| -> Vec3 { Vec3::of(this.g().rotate_z(angle)) })?;
    m.associated_function("rotate_axis", |this: &Vec3, axis: &Vec3, angle: f64| -> Vec3 { Vec3::of(this.g().rotate_axis(axis.g(), angle)) })?;
    m.associated_function("rotate_towards", |this: &Vec3, rhs: &Vec3, max_angle: f64| -> Vec3 { Vec3::of(this.g().rotate_towards(rhs.g(), max_angle)) })?;
    m.associated_function("any_orthogonal_vector", |this: &Vec3| -> Vec3 { Vec3::of(this.g().any_orthogonal_vector()) })?;
    m.associated_function("any_orthonormal_vector", |this: &Vec3| -> Vec3 { Vec3::of(this.g().any_orthonormal_vector()) })?;
    m.associated_function("any_orthonormal_pair", |this: &Vec3| -> (Vec3, Vec3) { { let r = this.g().any_orthonormal_pair(); (Vec3::of(r.0), Vec3::of(r.1)) } })?;
    m.associated_function("slerp", |this: &Vec3, rhs: &Vec3, s: f64| -> Vec3 { Vec3::of(this.g().slerp(rhs.g(), s)) })?;
    m.associated_function("as_ivec3", |this: &Vec3| -> IVec3 { IVec3::of(this.g().as_i64vec3()) })?;
    Ok(())
}

fn vec4_1(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    m.constant("ZERO", Vec4::of(DVec4::ZERO))
        .build_associated::<Vec4>()?;
    m.constant("ONE", Vec4::of(DVec4::ONE))
        .build_associated::<Vec4>()?;
    m.constant("NEG_ONE", Vec4::of(DVec4::NEG_ONE))
        .build_associated::<Vec4>()?;
    m.constant("MIN", Vec4::of(DVec4::MIN))
        .build_associated::<Vec4>()?;
    m.constant("MAX", Vec4::of(DVec4::MAX))
        .build_associated::<Vec4>()?;
    m.constant("NAN", Vec4::of(DVec4::NAN))
        .build_associated::<Vec4>()?;
    m.constant("INFINITY", Vec4::of(DVec4::INFINITY))
        .build_associated::<Vec4>()?;
    m.constant("NEG_INFINITY", Vec4::of(DVec4::NEG_INFINITY))
        .build_associated::<Vec4>()?;
    m.constant("X", Vec4::of(DVec4::X))
        .build_associated::<Vec4>()?;
    m.constant("Y", Vec4::of(DVec4::Y))
        .build_associated::<Vec4>()?;
    m.constant("Z", Vec4::of(DVec4::Z))
        .build_associated::<Vec4>()?;
    m.constant("W", Vec4::of(DVec4::W))
        .build_associated::<Vec4>()?;
    m.constant("NEG_X", Vec4::of(DVec4::NEG_X))
        .build_associated::<Vec4>()?;
    m.constant("NEG_Y", Vec4::of(DVec4::NEG_Y))
        .build_associated::<Vec4>()?;
    m.constant("NEG_Z", Vec4::of(DVec4::NEG_Z))
        .build_associated::<Vec4>()?;
    m.constant("NEG_W", Vec4::of(DVec4::NEG_W))
        .build_associated::<Vec4>()?;
    m.function("new", |x: f64, y: f64, z: f64, w: f64| -> Vec4 { Vec4::of(DVec4::new(x, y, z, w)) })
        .build_associated::<Vec4>()?;
    m.function("splat", |v: f64| -> Vec4 { Vec4::of(DVec4::splat(v)) })
        .build_associated::<Vec4>()?;
    m.associated_function("to_array", |this: &Vec4| -> Vec<f64> { this.g().to_array().to_vec() })?;
    m.associated_function("truncate", |this: &Vec4| -> Vec3 { Vec3::of(this.g().truncate()) })?;
    m.associated_function("project", |this: &Vec4| -> Vec3 { Vec3::of(this.g().project()) })?;
    m.associated_function("with_x", |this: &Vec4, x: f64| -> Vec4 { Vec4::of(this.g().with_x(x)) })?;
    m.associated_function("with_y", |this: &Vec4, y: f64| -> Vec4 { Vec4::of(this.g().with_y(y)) })?;
    m.associated_function("with_z", |this: &Vec4, z: f64| -> Vec4 { Vec4::of(this.g().with_z(z)) })?;
    m.associated_function("with_w", |this: &Vec4, w: f64| -> Vec4 { Vec4::of(this.g().with_w(w)) })?;
    m.associated_function("dot", |this: &Vec4, rhs: &Vec4| -> f64 { this.g().dot(rhs.g()) })?;
    m.associated_function("dot_into_vec", |this: &Vec4, rhs: &Vec4| -> Vec4 { Vec4::of(this.g().dot_into_vec(rhs.g())) })?;
    m.associated_function("min", |this: &Vec4, rhs: &Vec4| -> Vec4 { Vec4::of(this.g().min(rhs.g())) })?;
    m.associated_function("max", |this: &Vec4, rhs: &Vec4| -> Vec4 { Vec4::of(this.g().max(rhs.g())) })?;
    m.associated_function("clamp", |this: &Vec4, min: &Vec4, max: &Vec4| -> Vec4 { Vec4::of(this.g().clamp(min.g(), max.g())) })?;
    m.associated_function("min_element", |this: &Vec4| -> f64 { this.g().min_element() })?;
    m.associated_function("max_element", |this: &Vec4| -> f64 { this.g().max_element() })?;
    m.associated_function("min_position", |this: &Vec4| -> i64 { i64::try_from(this.g().min_position()).unwrap_or(i64::MAX) })?;
    m.associated_function("max_position", |this: &Vec4| -> i64 { i64::try_from(this.g().max_position()).unwrap_or(i64::MAX) })?;
    m.associated_function("element_sum", |this: &Vec4| -> f64 { this.g().element_sum() })?;
    m.associated_function("element_product", |this: &Vec4| -> f64 { this.g().element_product() })?;
    m.associated_function("abs", |this: &Vec4| -> Vec4 { Vec4::of(this.g().abs()) })?;
    m.associated_function("signum", |this: &Vec4| -> Vec4 { Vec4::of(this.g().signum()) })?;
    m.associated_function("copysign", |this: &Vec4, rhs: &Vec4| -> Vec4 { Vec4::of(this.g().copysign(rhs.g())) })?;
    m.associated_function("is_finite", |this: &Vec4| -> bool { this.g().is_finite() })?;
    m.associated_function("is_nan", |this: &Vec4| -> bool { this.g().is_nan() })?;
    m.associated_function("length", |this: &Vec4| -> f64 { this.g().length() })?;
    m.associated_function("length_squared", |this: &Vec4| -> f64 { this.g().length_squared() })?;
    m.associated_function("length_recip", |this: &Vec4| -> f64 { this.g().length_recip() })?;
    m.associated_function("distance", |this: &Vec4, rhs: &Vec4| -> f64 { this.g().distance(rhs.g()) })?;
    m.associated_function("distance_squared", |this: &Vec4, rhs: &Vec4| -> f64 { this.g().distance_squared(rhs.g()) })?;
    m.associated_function("div_euclid", |this: &Vec4, rhs: &Vec4| -> Vec4 { Vec4::of(this.g().div_euclid(rhs.g())) })?;
    m.associated_function("rem_euclid", |this: &Vec4, rhs: &Vec4| -> Vec4 { Vec4::of(this.g().rem_euclid(rhs.g())) })?;
    m.associated_function("normalize", |this: &Vec4| -> Vec4 { Vec4::of(this.g().normalize()) })?;
    m.associated_function("try_normalize", |this: &Vec4| -> Option<Vec4> { this.g().try_normalize().map(Vec4::of) })?;
    m.associated_function("normalize_or", |this: &Vec4, fallback: &Vec4| -> Vec4 { Vec4::of(this.g().normalize_or(fallback.g())) })?;
    m.associated_function("normalize_or_zero", |this: &Vec4| -> Vec4 { Vec4::of(this.g().normalize_or_zero()) })?;
    m.associated_function("normalize_and_length", |this: &Vec4| -> (Vec4, f64) { { let r = this.g().normalize_and_length(); (Vec4::of(r.0), r.1) } })?;
    m.associated_function("is_normalized", |this: &Vec4| -> bool { this.g().is_normalized() })?;
    m.associated_function("project_onto", |this: &Vec4, rhs: &Vec4| -> Vec4 { Vec4::of(this.g().project_onto(rhs.g())) })?;
    m.associated_function("reject_from", |this: &Vec4, rhs: &Vec4| -> Vec4 { Vec4::of(this.g().reject_from(rhs.g())) })?;
    m.associated_function("project_onto_normalized", |this: &Vec4, rhs: &Vec4| -> Vec4 { Vec4::of(this.g().project_onto_normalized(rhs.g())) })?;
    m.associated_function("reject_from_normalized", |this: &Vec4, rhs: &Vec4| -> Vec4 { Vec4::of(this.g().reject_from_normalized(rhs.g())) })?;
    m.associated_function("round", |this: &Vec4| -> Vec4 { Vec4::of(this.g().round()) })?;
    m.associated_function("floor", |this: &Vec4| -> Vec4 { Vec4::of(this.g().floor()) })?;
    m.associated_function("ceil", |this: &Vec4| -> Vec4 { Vec4::of(this.g().ceil()) })?;
    m.associated_function("trunc", |this: &Vec4| -> Vec4 { Vec4::of(this.g().trunc()) })?;
    m.associated_function("step", |this: &Vec4, rhs: &Vec4| -> Vec4 { Vec4::of(this.g().step(rhs.g())) })?;
    m.associated_function("smoothstep", |this: &Vec4, edge0: &Vec4, edge1: &Vec4| -> Vec4 { Vec4::of(this.g().smoothstep(edge0.g(), edge1.g())) })?;
    m.associated_function("saturate", |this: &Vec4| -> Vec4 { Vec4::of(this.g().saturate()) })?;
    m.associated_function("fract", |this: &Vec4| -> Vec4 { Vec4::of(this.g().fract()) })?;
    m.associated_function("fract_gl", |this: &Vec4| -> Vec4 { Vec4::of(this.g().fract_gl()) })?;
    m.associated_function("exp", |this: &Vec4| -> Vec4 { Vec4::of(this.g().exp()) })?;
    m.associated_function("exp2", |this: &Vec4| -> Vec4 { Vec4::of(this.g().exp2()) })?;
    m.associated_function("ln", |this: &Vec4| -> Vec4 { Vec4::of(this.g().ln()) })?;
    m.associated_function("log2", |this: &Vec4| -> Vec4 { Vec4::of(this.g().log2()) })?;
    m.associated_function("powf", |this: &Vec4, n: f64| -> Vec4 { Vec4::of(this.g().powf(n)) })?;
    Ok(())
}

fn vec4_2(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    m.associated_function("sqrt", |this: &Vec4| -> Vec4 { Vec4::of(this.g().sqrt()) })?;
    m.associated_function("cos", |this: &Vec4| -> Vec4 { Vec4::of(this.g().cos()) })?;
    m.associated_function("sin", |this: &Vec4| -> Vec4 { Vec4::of(this.g().sin()) })?;
    m.associated_function("sin_cos", |this: &Vec4| -> (Vec4, Vec4) { { let r = this.g().sin_cos(); (Vec4::of(r.0), Vec4::of(r.1)) } })?;
    m.associated_function("recip", |this: &Vec4| -> Vec4 { Vec4::of(this.g().recip()) })?;
    m.associated_function("lerp", |this: &Vec4, rhs: &Vec4, s: f64| -> Vec4 { Vec4::of(this.g().lerp(rhs.g(), s)) })?;
    m.associated_function("move_towards", |this: &Vec4, rhs: &Vec4, d: f64| -> Vec4 { Vec4::of(this.g().move_towards(rhs.g(), d)) })?;
    m.associated_function("midpoint", |this: &Vec4, rhs: &Vec4| -> Vec4 { Vec4::of(this.g().midpoint(rhs.g())) })?;
    m.associated_function("abs_diff_eq", |this: &Vec4, rhs: &Vec4, max_abs_diff: f64| -> bool { this.g().abs_diff_eq(rhs.g(), max_abs_diff) })?;
    m.associated_function("clamp_length", |this: &Vec4, min: f64, max: f64| -> Vec4 { Vec4::of(this.g().clamp_length(min, max)) })?;
    m.associated_function("clamp_length_max", |this: &Vec4, max: f64| -> Vec4 { Vec4::of(this.g().clamp_length_max(max)) })?;
    m.associated_function("clamp_length_min", |this: &Vec4, min: f64| -> Vec4 { Vec4::of(this.g().clamp_length_min(min)) })?;
    m.associated_function("mul_add", |this: &Vec4, a: &Vec4, b: &Vec4| -> Vec4 { Vec4::of(this.g().mul_add(a.g(), b.g())) })?;
    m.associated_function("reflect", |this: &Vec4, normal: &Vec4| -> Vec4 { Vec4::of(this.g().reflect(normal.g())) })?;
    m.associated_function("refract", |this: &Vec4, normal: &Vec4, eta: f64| -> Vec4 { Vec4::of(this.g().refract(normal.g(), eta)) })?;
    Ok(())
}

fn quat_1(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    m.constant("IDENTITY", Quat::of(DQuat::IDENTITY))
        .build_associated::<Quat>()?;
    m.constant("NAN", Quat::of(DQuat::NAN))
        .build_associated::<Quat>()?;
    m.function("from_xyzw", |x: f64, y: f64, z: f64, w: f64| -> Quat { Quat::of(DQuat::from_xyzw(x, y, z, w)) })
        .build_associated::<Quat>()?;
    m.function("from_vec4", |v: &Vec4| -> Quat { Quat::of(DQuat::from_vec4(v.g())) })
        .build_associated::<Quat>()?;
    m.function("from_axis_angle", |axis: &Vec3, angle: f64| -> Quat { Quat::of(DQuat::from_axis_angle(axis.g(), angle)) })
        .build_associated::<Quat>()?;
    m.function("from_scaled_axis", |v: &Vec3| -> Quat { Quat::of(DQuat::from_scaled_axis(v.g())) })
        .build_associated::<Quat>()?;
    m.function("from_rotation_x", |angle: f64| -> Quat { Quat::of(DQuat::from_rotation_x(angle)) })
        .build_associated::<Quat>()?;
    m.function("from_rotation_y", |angle: f64| -> Quat { Quat::of(DQuat::from_rotation_y(angle)) })
        .build_associated::<Quat>()?;
    m.function("from_rotation_z", |angle: f64| -> Quat { Quat::of(DQuat::from_rotation_z(angle)) })
        .build_associated::<Quat>()?;
    m.function("from_euler", |euler: &str, a: f64, b: f64, c: f64| -> VmResult<Quat> { VmResult::Ok(Quat::of(DQuat::from_euler(vm_try!(euler_order(euler)), a, b, c))) })
        .build_associated::<Quat>()?;
    m.function("from_rotation_axes", |x_axis: &Vec3, y_axis: &Vec3, z_axis: &Vec3| -> Quat { Quat::of(DQuat::from_rotation_axes(x_axis.g(), y_axis.g(), z_axis.g())) })
        .build_associated::<Quat>()?;
    m.function("from_rotation_arc", |from: &Vec3, to: &Vec3| -> Quat { Quat::of(DQuat::from_rotation_arc(from.g(), to.g())) })
        .build_associated::<Quat>()?;
    m.function("from_rotation_arc_colinear", |from: &Vec3, to: &Vec3| -> Quat { Quat::of(DQuat::from_rotation_arc_colinear(from.g(), to.g())) })
        .build_associated::<Quat>()?;
    m.function("from_rotation_arc_2d", |from: &Vec2, to: &Vec2| -> Quat { Quat::of(DQuat::from_rotation_arc_2d(from.g(), to.g())) })
        .build_associated::<Quat>()?;
    m.associated_function("to_axis_angle", |this: &Quat| -> (Vec3, f64) { { let r = this.g().to_axis_angle(); (Vec3::of(r.0), r.1) } })?;
    m.associated_function("to_scaled_axis", |this: &Quat| -> Vec3 { Vec3::of(this.g().to_scaled_axis()) })?;
    m.associated_function("to_euler", |this: &Quat, order: &str| -> VmResult<(f64, f64, f64)> { VmResult::Ok({ let r = this.g().to_euler(vm_try!(euler_order(order))); (r.0, r.1, r.2) }) })?;
    m.associated_function("to_array", |this: &Quat| -> Vec<f64> { this.g().to_array().to_vec() })?;
    m.associated_function("xyz", |this: &Quat| -> Vec3 { Vec3::of(this.g().xyz()) })?;
    m.associated_function("conjugate", |this: &Quat| -> Quat { Quat::of(this.g().conjugate()) })?;
    m.associated_function("inverse", |this: &Quat| -> Quat { Quat::of(this.g().inverse()) })?;
    m.associated_function("dot", |this: &Quat, rhs: &Quat| -> f64 { this.g().dot(rhs.g()) })?;
    m.associated_function("length", |this: &Quat| -> f64 { this.g().length() })?;
    m.associated_function("length_squared", |this: &Quat| -> f64 { this.g().length_squared() })?;
    m.associated_function("length_recip", |this: &Quat| -> f64 { this.g().length_recip() })?;
    m.associated_function("normalize", |this: &Quat| -> Quat { Quat::of(this.g().normalize()) })?;
    m.associated_function("is_finite", |this: &Quat| -> bool { this.g().is_finite() })?;
    m.associated_function("is_nan", |this: &Quat| -> bool { this.g().is_nan() })?;
    m.associated_function("is_normalized", |this: &Quat| -> bool { this.g().is_normalized() })?;
    m.associated_function("is_near_identity", |this: &Quat| -> bool { this.g().is_near_identity() })?;
    m.associated_function("angle_between", |this: &Quat, rhs: &Quat| -> f64 { this.g().angle_between(rhs.g()) })?;
    m.associated_function("rotate_towards", |this: &Quat, rhs: &Quat, max_angle: f64| -> Quat { Quat::of(this.g().rotate_towards(rhs.g(), max_angle)) })?;
    m.associated_function("abs_diff_eq", |this: &Quat, rhs: &Quat, max_abs_diff: f64| -> bool { this.g().abs_diff_eq(rhs.g(), max_abs_diff) })?;
    m.associated_function("lerp", |this: &Quat, end: &Quat, s: f64| -> Quat { Quat::of(this.g().lerp(end.g(), s)) })?;
    m.associated_function("slerp", |this: &Quat, end: &Quat, s: f64| -> Quat { Quat::of(this.g().slerp(end.g(), s)) })?;
    m.associated_function("slerp_long", |this: &Quat, end: &Quat, s: f64| -> Quat { Quat::of(this.g().slerp_long(end.g(), s)) })?;
    m.associated_function("mul_vec3", |this: &Quat, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().mul_vec3(rhs.g())) })?;
    m.associated_function("mul_quat", |this: &Quat, rhs: &Quat| -> Quat { Quat::of(this.g().mul_quat(rhs.g())) })?;
    Ok(())
}

fn transform_2d_1(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    m.constant("ZERO", Transform2d::of(DAffine2::ZERO))
        .build_associated::<Transform2d>()?;
    m.constant("IDENTITY", Transform2d::of(DAffine2::IDENTITY))
        .build_associated::<Transform2d>()?;
    m.constant("NAN", Transform2d::of(DAffine2::NAN))
        .build_associated::<Transform2d>()?;
    m.function("from_cols", |x_axis: &Vec2, y_axis: &Vec2, z_axis: &Vec2| -> Transform2d { Transform2d::of(DAffine2::from_cols(x_axis.g(), y_axis.g(), z_axis.g())) })
        .build_associated::<Transform2d>()?;
    m.associated_function("to_cols_array", |this: &Transform2d| -> Vec<f64> { this.g().to_cols_array().to_vec() })?;
    m.function("from_scale", |scale: &Vec2| -> Transform2d { Transform2d::of(DAffine2::from_scale(scale.g())) })
        .build_associated::<Transform2d>()?;
    m.function("from_angle", |angle: f64| -> Transform2d { Transform2d::of(DAffine2::from_angle(angle)) })
        .build_associated::<Transform2d>()?;
    m.function("from_translation", |translation: &Vec2| -> Transform2d { Transform2d::of(DAffine2::from_translation(translation.g())) })
        .build_associated::<Transform2d>()?;
    m.function("from_scale_angle_translation", |scale: &Vec2, angle: f64, translation: &Vec2| -> Transform2d { Transform2d::of(DAffine2::from_scale_angle_translation(scale.g(), angle, translation.g())) })
        .build_associated::<Transform2d>()?;
    m.function("from_angle_translation", |angle: f64, translation: &Vec2| -> Transform2d { Transform2d::of(DAffine2::from_angle_translation(angle, translation.g())) })
        .build_associated::<Transform2d>()?;
    m.associated_function("to_scale_angle_translation", |this: &Transform2d| -> (Vec2, f64, Vec2) { { let r = this.g().to_scale_angle_translation(); (Vec2::of(r.0), r.1, Vec2::of(r.2)) } })?;
    m.associated_function("transform_point2", |this: &Transform2d, rhs: &Vec2| -> Vec2 { Vec2::of(this.g().transform_point2(rhs.g())) })?;
    m.associated_function("transform_vector2", |this: &Transform2d, rhs: &Vec2| -> Vec2 { Vec2::of(this.g().transform_vector2(rhs.g())) })?;
    m.associated_function("is_finite", |this: &Transform2d| -> bool { this.g().is_finite() })?;
    m.associated_function("is_nan", |this: &Transform2d| -> bool { this.g().is_nan() })?;
    m.associated_function("abs_diff_eq", |this: &Transform2d, rhs: &Transform2d, max_abs_diff: f64| -> bool { this.g().abs_diff_eq(rhs.g(), max_abs_diff) })?;
    Ok(())
}

fn transform_3d_1(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    m.constant("ZERO", Transform3d::of(DAffine3::ZERO))
        .build_associated::<Transform3d>()?;
    m.constant("IDENTITY", Transform3d::of(DAffine3::IDENTITY))
        .build_associated::<Transform3d>()?;
    m.constant("NAN", Transform3d::of(DAffine3::NAN))
        .build_associated::<Transform3d>()?;
    m.function("from_cols", |x_axis: &Vec3, y_axis: &Vec3, z_axis: &Vec3, w_axis: &Vec3| -> Transform3d { Transform3d::of(DAffine3::from_cols(x_axis.g(), y_axis.g(), z_axis.g(), w_axis.g())) })
        .build_associated::<Transform3d>()?;
    m.function("from_scale", |scale: &Vec3| -> Transform3d { Transform3d::of(DAffine3::from_scale(scale.g())) })
        .build_associated::<Transform3d>()?;
    m.function("from_quat", |rotation: &Quat| -> Transform3d { Transform3d::of(DAffine3::from_quat(rotation.g())) })
        .build_associated::<Transform3d>()?;
    m.function("from_axis_angle", |axis: &Vec3, angle: f64| -> Transform3d { Transform3d::of(DAffine3::from_axis_angle(axis.g(), angle)) })
        .build_associated::<Transform3d>()?;
    m.function("from_rotation_x", |angle: f64| -> Transform3d { Transform3d::of(DAffine3::from_rotation_x(angle)) })
        .build_associated::<Transform3d>()?;
    m.function("from_rotation_y", |angle: f64| -> Transform3d { Transform3d::of(DAffine3::from_rotation_y(angle)) })
        .build_associated::<Transform3d>()?;
    m.function("from_rotation_z", |angle: f64| -> Transform3d { Transform3d::of(DAffine3::from_rotation_z(angle)) })
        .build_associated::<Transform3d>()?;
    m.function("from_translation", |translation: &Vec3| -> Transform3d { Transform3d::of(DAffine3::from_translation(translation.g())) })
        .build_associated::<Transform3d>()?;
    m.function("from_scale_rotation_translation", |scale: &Vec3, rotation: &Quat, translation: &Vec3| -> Transform3d { Transform3d::of(DAffine3::from_scale_rotation_translation(scale.g(), rotation.g(), translation.g())) })
        .build_associated::<Transform3d>()?;
    m.function("from_rotation_translation", |rotation: &Quat, translation: &Vec3| -> Transform3d { Transform3d::of(DAffine3::from_rotation_translation(rotation.g(), translation.g())) })
        .build_associated::<Transform3d>()?;
    m.associated_function("to_scale_rotation_translation", |this: &Transform3d| -> (Vec3, Quat, Vec3) { { let r = this.g().to_scale_rotation_translation(); (Vec3::of(r.0), Quat::of(r.1), Vec3::of(r.2)) } })?;
    m.associated_function("transform_point3", |this: &Transform3d, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().transform_point3(rhs.g())) })?;
    m.associated_function("transform_vector3", |this: &Transform3d, rhs: &Vec3| -> Vec3 { Vec3::of(this.g().transform_vector3(rhs.g())) })?;
    m.associated_function("is_finite", |this: &Transform3d| -> bool { this.g().is_finite() })?;
    m.associated_function("is_nan", |this: &Transform3d| -> bool { this.g().is_nan() })?;
    m.associated_function("abs_diff_eq", |this: &Transform3d, rhs: &Transform3d, max_abs_diff: f64| -> bool { this.g().abs_diff_eq(rhs.g(), max_abs_diff) })?;
    Ok(())
}

fn ivec2_1(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    m.constant("ZERO", IVec2::of(I64Vec2::ZERO))
        .build_associated::<IVec2>()?;
    m.constant("ONE", IVec2::of(I64Vec2::ONE))
        .build_associated::<IVec2>()?;
    m.constant("NEG_ONE", IVec2::of(I64Vec2::NEG_ONE))
        .build_associated::<IVec2>()?;
    m.constant("MIN", IVec2::of(I64Vec2::MIN))
        .build_associated::<IVec2>()?;
    m.constant("MAX", IVec2::of(I64Vec2::MAX))
        .build_associated::<IVec2>()?;
    m.constant("X", IVec2::of(I64Vec2::X))
        .build_associated::<IVec2>()?;
    m.constant("Y", IVec2::of(I64Vec2::Y))
        .build_associated::<IVec2>()?;
    m.constant("NEG_X", IVec2::of(I64Vec2::NEG_X))
        .build_associated::<IVec2>()?;
    m.constant("NEG_Y", IVec2::of(I64Vec2::NEG_Y))
        .build_associated::<IVec2>()?;
    m.function("new", |x: i64, y: i64| -> IVec2 { IVec2::of(I64Vec2::new(x, y)) })
        .build_associated::<IVec2>()?;
    m.function("splat", |v: i64| -> IVec2 { IVec2::of(I64Vec2::splat(v)) })
        .build_associated::<IVec2>()?;
    m.associated_function("to_array", |this: &IVec2| -> Vec<i64> { this.g().to_array().to_vec() })?;
    m.associated_function("extend", |this: &IVec2, z: i64| -> IVec3 { IVec3::of(this.g().extend(z)) })?;
    m.associated_function("with_x", |this: &IVec2, x: i64| -> IVec2 { IVec2::of(this.g().with_x(x)) })?;
    m.associated_function("with_y", |this: &IVec2, y: i64| -> IVec2 { IVec2::of(this.g().with_y(y)) })?;
    m.associated_function("dot", |this: &IVec2, rhs: &IVec2| -> i64 { this.g().dot(rhs.g()) })?;
    m.associated_function("dot_into_vec", |this: &IVec2, rhs: &IVec2| -> IVec2 { IVec2::of(this.g().dot_into_vec(rhs.g())) })?;
    m.associated_function("min", |this: &IVec2, rhs: &IVec2| -> IVec2 { IVec2::of(this.g().min(rhs.g())) })?;
    m.associated_function("max", |this: &IVec2, rhs: &IVec2| -> IVec2 { IVec2::of(this.g().max(rhs.g())) })?;
    m.associated_function("clamp", |this: &IVec2, min: &IVec2, max: &IVec2| -> IVec2 { IVec2::of(this.g().clamp(min.g(), max.g())) })?;
    m.associated_function("min_element", |this: &IVec2| -> i64 { this.g().min_element() })?;
    m.associated_function("max_element", |this: &IVec2| -> i64 { this.g().max_element() })?;
    m.associated_function("min_position", |this: &IVec2| -> i64 { i64::try_from(this.g().min_position()).unwrap_or(i64::MAX) })?;
    m.associated_function("max_position", |this: &IVec2| -> i64 { i64::try_from(this.g().max_position()).unwrap_or(i64::MAX) })?;
    m.associated_function("element_sum", |this: &IVec2| -> i64 { this.g().element_sum() })?;
    m.associated_function("element_product", |this: &IVec2| -> i64 { this.g().element_product() })?;
    m.associated_function("abs", |this: &IVec2| -> IVec2 { IVec2::of(this.g().abs()) })?;
    m.associated_function("signum", |this: &IVec2| -> IVec2 { IVec2::of(this.g().signum()) })?;
    m.associated_function("length_squared", |this: &IVec2| -> i64 { this.g().length_squared() })?;
    m.associated_function("distance_squared", |this: &IVec2, rhs: &IVec2| -> i64 { this.g().distance_squared(rhs.g()) })?;
    m.associated_function("div_euclid", |this: &IVec2, rhs: &IVec2| -> IVec2 { IVec2::of(this.g().div_euclid(rhs.g())) })?;
    m.associated_function("rem_euclid", |this: &IVec2, rhs: &IVec2| -> IVec2 { IVec2::of(this.g().rem_euclid(rhs.g())) })?;
    m.associated_function("perp", |this: &IVec2| -> IVec2 { IVec2::of(this.g().perp()) })?;
    m.associated_function("perp_dot", |this: &IVec2, rhs: &IVec2| -> i64 { this.g().perp_dot(rhs.g()) })?;
    m.associated_function("rotate", |this: &IVec2, rhs: &IVec2| -> IVec2 { IVec2::of(this.g().rotate(rhs.g())) })?;
    m.associated_function("as_vec2", |this: &IVec2| -> Vec2 { Vec2::of(this.g().as_dvec2()) })?;
    m.associated_function("checked_add", |this: &IVec2, rhs: &IVec2| -> Option<IVec2> { this.g().checked_add(rhs.g()).map(IVec2::of) })?;
    m.associated_function("checked_sub", |this: &IVec2, rhs: &IVec2| -> Option<IVec2> { this.g().checked_sub(rhs.g()).map(IVec2::of) })?;
    m.associated_function("checked_mul", |this: &IVec2, rhs: &IVec2| -> Option<IVec2> { this.g().checked_mul(rhs.g()).map(IVec2::of) })?;
    m.associated_function("checked_div", |this: &IVec2, rhs: &IVec2| -> Option<IVec2> { this.g().checked_div(rhs.g()).map(IVec2::of) })?;
    m.associated_function("wrapping_add", |this: &IVec2, rhs: &IVec2| -> IVec2 { IVec2::of(this.g().wrapping_add(rhs.g())) })?;
    m.associated_function("wrapping_sub", |this: &IVec2, rhs: &IVec2| -> IVec2 { IVec2::of(this.g().wrapping_sub(rhs.g())) })?;
    m.associated_function("wrapping_mul", |this: &IVec2, rhs: &IVec2| -> IVec2 { IVec2::of(this.g().wrapping_mul(rhs.g())) })?;
    m.associated_function("wrapping_div", |this: &IVec2, rhs: &IVec2| -> IVec2 { IVec2::of(this.g().wrapping_div(rhs.g())) })?;
    m.associated_function("saturating_add", |this: &IVec2, rhs: &IVec2| -> IVec2 { IVec2::of(this.g().saturating_add(rhs.g())) })?;
    m.associated_function("saturating_sub", |this: &IVec2, rhs: &IVec2| -> IVec2 { IVec2::of(this.g().saturating_sub(rhs.g())) })?;
    m.associated_function("saturating_mul", |this: &IVec2, rhs: &IVec2| -> IVec2 { IVec2::of(this.g().saturating_mul(rhs.g())) })?;
    m.associated_function("saturating_div", |this: &IVec2, rhs: &IVec2| -> IVec2 { IVec2::of(this.g().saturating_div(rhs.g())) })?;
    Ok(())
}

fn ivec3_1(m: &mut rune::Module) -> Result<(), rune::ContextError> {
    m.constant("ZERO", IVec3::of(I64Vec3::ZERO))
        .build_associated::<IVec3>()?;
    m.constant("ONE", IVec3::of(I64Vec3::ONE))
        .build_associated::<IVec3>()?;
    m.constant("NEG_ONE", IVec3::of(I64Vec3::NEG_ONE))
        .build_associated::<IVec3>()?;
    m.constant("MIN", IVec3::of(I64Vec3::MIN))
        .build_associated::<IVec3>()?;
    m.constant("MAX", IVec3::of(I64Vec3::MAX))
        .build_associated::<IVec3>()?;
    m.constant("X", IVec3::of(I64Vec3::X))
        .build_associated::<IVec3>()?;
    m.constant("Y", IVec3::of(I64Vec3::Y))
        .build_associated::<IVec3>()?;
    m.constant("Z", IVec3::of(I64Vec3::Z))
        .build_associated::<IVec3>()?;
    m.constant("NEG_X", IVec3::of(I64Vec3::NEG_X))
        .build_associated::<IVec3>()?;
    m.constant("NEG_Y", IVec3::of(I64Vec3::NEG_Y))
        .build_associated::<IVec3>()?;
    m.constant("NEG_Z", IVec3::of(I64Vec3::NEG_Z))
        .build_associated::<IVec3>()?;
    m.function("new", |x: i64, y: i64, z: i64| -> IVec3 { IVec3::of(I64Vec3::new(x, y, z)) })
        .build_associated::<IVec3>()?;
    m.function("splat", |v: i64| -> IVec3 { IVec3::of(I64Vec3::splat(v)) })
        .build_associated::<IVec3>()?;
    m.associated_function("to_array", |this: &IVec3| -> Vec<i64> { this.g().to_array().to_vec() })?;
    m.associated_function("truncate", |this: &IVec3| -> IVec2 { IVec2::of(this.g().truncate()) })?;
    m.associated_function("with_x", |this: &IVec3, x: i64| -> IVec3 { IVec3::of(this.g().with_x(x)) })?;
    m.associated_function("with_y", |this: &IVec3, y: i64| -> IVec3 { IVec3::of(this.g().with_y(y)) })?;
    m.associated_function("with_z", |this: &IVec3, z: i64| -> IVec3 { IVec3::of(this.g().with_z(z)) })?;
    m.associated_function("dot", |this: &IVec3, rhs: &IVec3| -> i64 { this.g().dot(rhs.g()) })?;
    m.associated_function("dot_into_vec", |this: &IVec3, rhs: &IVec3| -> IVec3 { IVec3::of(this.g().dot_into_vec(rhs.g())) })?;
    m.associated_function("cross", |this: &IVec3, rhs: &IVec3| -> IVec3 { IVec3::of(this.g().cross(rhs.g())) })?;
    m.associated_function("min", |this: &IVec3, rhs: &IVec3| -> IVec3 { IVec3::of(this.g().min(rhs.g())) })?;
    m.associated_function("max", |this: &IVec3, rhs: &IVec3| -> IVec3 { IVec3::of(this.g().max(rhs.g())) })?;
    m.associated_function("clamp", |this: &IVec3, min: &IVec3, max: &IVec3| -> IVec3 { IVec3::of(this.g().clamp(min.g(), max.g())) })?;
    m.associated_function("min_element", |this: &IVec3| -> i64 { this.g().min_element() })?;
    m.associated_function("max_element", |this: &IVec3| -> i64 { this.g().max_element() })?;
    m.associated_function("min_position", |this: &IVec3| -> i64 { i64::try_from(this.g().min_position()).unwrap_or(i64::MAX) })?;
    m.associated_function("max_position", |this: &IVec3| -> i64 { i64::try_from(this.g().max_position()).unwrap_or(i64::MAX) })?;
    m.associated_function("element_sum", |this: &IVec3| -> i64 { this.g().element_sum() })?;
    m.associated_function("element_product", |this: &IVec3| -> i64 { this.g().element_product() })?;
    m.associated_function("abs", |this: &IVec3| -> IVec3 { IVec3::of(this.g().abs()) })?;
    m.associated_function("signum", |this: &IVec3| -> IVec3 { IVec3::of(this.g().signum()) })?;
    m.associated_function("length_squared", |this: &IVec3| -> i64 { this.g().length_squared() })?;
    m.associated_function("distance_squared", |this: &IVec3, rhs: &IVec3| -> i64 { this.g().distance_squared(rhs.g()) })?;
    m.associated_function("div_euclid", |this: &IVec3, rhs: &IVec3| -> IVec3 { IVec3::of(this.g().div_euclid(rhs.g())) })?;
    m.associated_function("rem_euclid", |this: &IVec3, rhs: &IVec3| -> IVec3 { IVec3::of(this.g().rem_euclid(rhs.g())) })?;
    m.associated_function("as_vec3", |this: &IVec3| -> Vec3 { Vec3::of(this.g().as_dvec3()) })?;
    m.associated_function("checked_add", |this: &IVec3, rhs: &IVec3| -> Option<IVec3> { this.g().checked_add(rhs.g()).map(IVec3::of) })?;
    m.associated_function("checked_sub", |this: &IVec3, rhs: &IVec3| -> Option<IVec3> { this.g().checked_sub(rhs.g()).map(IVec3::of) })?;
    m.associated_function("checked_mul", |this: &IVec3, rhs: &IVec3| -> Option<IVec3> { this.g().checked_mul(rhs.g()).map(IVec3::of) })?;
    m.associated_function("checked_div", |this: &IVec3, rhs: &IVec3| -> Option<IVec3> { this.g().checked_div(rhs.g()).map(IVec3::of) })?;
    m.associated_function("wrapping_add", |this: &IVec3, rhs: &IVec3| -> IVec3 { IVec3::of(this.g().wrapping_add(rhs.g())) })?;
    m.associated_function("wrapping_sub", |this: &IVec3, rhs: &IVec3| -> IVec3 { IVec3::of(this.g().wrapping_sub(rhs.g())) })?;
    m.associated_function("wrapping_mul", |this: &IVec3, rhs: &IVec3| -> IVec3 { IVec3::of(this.g().wrapping_mul(rhs.g())) })?;
    m.associated_function("wrapping_div", |this: &IVec3, rhs: &IVec3| -> IVec3 { IVec3::of(this.g().wrapping_div(rhs.g())) })?;
    m.associated_function("saturating_add", |this: &IVec3, rhs: &IVec3| -> IVec3 { IVec3::of(this.g().saturating_add(rhs.g())) })?;
    m.associated_function("saturating_sub", |this: &IVec3, rhs: &IVec3| -> IVec3 { IVec3::of(this.g().saturating_sub(rhs.g())) })?;
    m.associated_function("saturating_mul", |this: &IVec3, rhs: &IVec3| -> IVec3 { IVec3::of(this.g().saturating_mul(rhs.g())) })?;
    m.associated_function("saturating_div", |this: &IVec3, rhs: &IVec3| -> IVec3 { IVec3::of(this.g().saturating_div(rhs.g())) })?;
    Ok(())
}

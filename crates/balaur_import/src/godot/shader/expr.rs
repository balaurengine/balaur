//! The expression half of the emitter: what an expression, a call and a
//! sampler read become in WGSL.

use std::fmt::Write as _;

use anyhow::{Result, anyhow, bail};

use super::{Emitter, PPU, Stage, swizzle};
use crate::godot::shader_names::{UNSUPPORTED_BUILTINS, sanitize, wgsl_type};
use crate::godot::shader_syntax::Expr;

impl Emitter {
    /// An expression used for what it does: an assignment, a step, a call.
    pub(super) fn effect(&mut self, e: &Expr) -> Result<String> {
        match e {
            Expr::Assign(op, lhs, rhs) => {
                if let Expr::Member(base, member) = lhs.as_ref()
                    && member.len() > 1
                    && let Some(components) = swizzle(member)
                {
                    let base = self.expr(base)?;
                    let value = if *op == "=" {
                        self.expr(rhs)?
                    } else {
                        format!(
                            "{base}.{components} {} ({})",
                            &op[..op.len() - 1],
                            self.expr(rhs)?
                        )
                    };
                    self.temps += 1;
                    let temp = format!("godot_t{}", self.temps);
                    let mut assigns = String::new();
                    for (i, c) in components.chars().enumerate() {
                        let from = ['x', 'y', 'z', 'w'][i];
                        write!(assigns, " {base}.{c} = {temp}.{from};")?;
                    }
                    return Ok(format!("{{ let {temp} = {value};{assigns} }}"));
                }
                Ok(format!("{} {op} {};", self.expr(lhs)?, self.expr(rhs)?))
            }
            Expr::Step(op, target) => {
                let sign = if *op == "++" { "+" } else { "-" };
                Ok(format!("{} {sign}= 1;", self.expr(target)?))
            }
            Expr::Call(name, _) => {
                let call = self.expr(e)?;
                let returns = self.returns.get(name).is_some_and(|r| r != "void");
                Ok(if returns {
                    format!("_ = {call};")
                } else {
                    format!("{call};")
                })
            }
            other => Ok(format!("_ = {};", self.expr(other)?)),
        }
    }

    pub(super) fn ident(&mut self, name: &str) -> Result<String> {
        if self.refs.contains(name) {
            return Ok(format!("(*{})", sanitize(name)));
        }
        if let Some(uniform) = self.uniforms.get(name) {
            let field = format!("params.{}", sanitize(name));
            return Ok(match uniform.ty.as_str() {
                "bool" => format!("({field} != 0.0)"),
                "int" => format!("i32({field})"),
                "uint" => format!("u32({field})"),
                ty if ty.starts_with("sampler") => {
                    bail!("the sampler `{name}` is read outside a texture call")
                }
                _ => field,
            });
        }
        let stage = self.stage;
        let builtin = match name {
            "TIME" => {
                self.imports.insert("time");
                "time()".to_string()
            }
            "PI" => "3.141592653589793".into(),
            "TAU" => "6.283185307179586".into(),
            "E" => "2.718281828459045".into(),
            "UV" | "COLOR" if stage != Stage::Helper => name.to_string(),
            "VERTEX" if stage == Stage::Vertex => "VERTEX".into(),
            "VERTEX" if stage == Stage::Fragment => {
                self.fragment_vertex = true;
                "in.godot_vertex".into()
            }
            "TEXTURE_PIXEL_SIZE" => {
                self.imports.insert("texture_pixel_size");
                "texture_pixel_size()".into()
            }
            "SCREEN_PIXEL_SIZE" => {
                self.imports.insert("screen_pixel_size");
                "screen_pixel_size()".into()
            }
            "SCREEN_UV" if stage == Stage::Fragment => {
                self.imports.insert("screen_uv");
                "screen_uv(in.clip_position)".into()
            }
            "FRAGCOORD" if stage == Stage::Fragment => "in.clip_position".into(),
            "MODEL_MATRIX" => {
                self.imports.insert("model_matrix_pixels");
                format!("model_matrix_pixels({PPU})")
            }
            "TEXTURE" | "SCREEN_TEXTURE" => bail!("`{name}` is read outside a texture call"),
            "UV" | "COLOR" | "VERTEX" | "SCREEN_UV" | "FRAGCOORD" => {
                bail!("`{name}` is read where this cannot reach it")
            }
            other if UNSUPPORTED_BUILTINS.contains(&other) => {
                bail!("the built-in `{other}` has no equivalent")
            }
            "true" | "false" => name.to_string(),
            other => sanitize(other),
        };
        Ok(builtin)
    }

    /// The texture and sampler a `texture()` call's first argument names.
    pub(super) fn sampler(&mut self, e: &Expr) -> Result<(&'static str, &'static str)> {
        let Expr::Ident(name) = e else {
            bail!("a texture call on something that is not a sampler");
        };
        if name == "TEXTURE" {
            self.imports.extend(["albedo_texture", "albedo_sampler"]);
            return Ok(("albedo_texture", "albedo_sampler"));
        }
        let uniform = self
            .uniforms
            .get(name)
            .ok_or_else(|| anyhow!("`{name}` is not a sampler this shader declares"))?;
        if uniform.screen || name == "SCREEN_TEXTURE" {
            self.screen = true;
            self.imports.extend(["screen_texture", "screen_sampler"]);
            return Ok(("screen_texture", "screen_sampler"));
        }
        let slot = uniform
            .slot
            .ok_or_else(|| anyhow!("`{name}` is not a sampler"))?;
        let (texture, sampler) = [
            ("texture_1", "sampler_1"),
            ("texture_2", "sampler_2"),
            ("texture_3", "sampler_3"),
            ("texture_4", "sampler_4"),
        ][slot - 1];
        self.imports.extend([texture, sampler]);
        Ok((texture, sampler))
    }

    pub(super) fn args(&mut self, args: &[Expr]) -> Result<Vec<String>> {
        args.iter().map(|a| self.expr(a)).collect()
    }

    pub(super) fn expr(&mut self, e: &Expr) -> Result<String> {
        Ok(match e {
            Expr::Ident(name) => self.ident(name)?,
            // `1.0f` is GLSL's float suffix; a hex literal's `f` is a digit.
            Expr::Num(n) if !n.starts_with("0x") && n.contains(['.', 'e', 'E']) => {
                n.trim_end_matches('f').to_string()
            }
            Expr::Num(n) => n.clone(),
            Expr::Array(ty, size, args) => {
                let values = self.args(args)?;
                let size = size.unwrap_or(args.len());
                format!("array<{}, {size}>({})", wgsl_type(ty)?, values.join(", "))
            }
            Expr::Index(base, index) => format!("{}[{}]", self.expr(base)?, self.expr(index)?),
            Expr::Member(base, member) => {
                let base_text = self.expr(base)?;
                let is_struct_field = !member.chars().all(|c| "xyzwrgbastpq".contains(c));
                if is_struct_field {
                    format!("{base_text}.{}", sanitize(member))
                } else {
                    let components =
                        swizzle(member).ok_or_else(|| anyhow!("the swizzle `.{member}`"))?;
                    format!("{base_text}.{components}")
                }
            }
            Expr::Unary(op, inner) => format!("{op}({})", self.expr(inner)?),
            Expr::Binary(op, a, b) => {
                let (a, b) = (self.expr(a)?, self.expr(b)?);
                match *op {
                    "^^" => format!("(({a}) != ({b}))"),
                    _ => format!("({a} {op} {b})"),
                }
            }
            Expr::Ternary(cond, then, other) => format!(
                "select({}, {}, {})",
                self.expr(other)?,
                self.expr(then)?,
                self.expr(cond)?
            ),
            Expr::Assign(..) | Expr::Step(..) => {
                bail!("an assignment inside an expression has no equivalent")
            }
            Expr::Call(name, args) => self.call(name, args)?,
        })
    }

    pub(super) fn call(&mut self, name: &str, args: &[Expr]) -> Result<String> {
        match name {
            "texture" | "textureLod" => {
                let [sampler, uv, rest @ ..] = args else {
                    bail!("`{name}` with too few arguments");
                };
                let (texture, sampler) = self.sampler(sampler)?;
                let uv = self.expr(uv)?;
                let level = match (name, rest.first()) {
                    ("textureLod", Some(level)) => self.expr(level)?,
                    _ => "0.0".into(),
                };
                return Ok(format!(
                    "textureSampleLevel({texture}, {sampler}, {uv}, {level})"
                ));
            }
            "texelFetch" => {
                let [sampler, at, level] = args else {
                    bail!("`texelFetch` takes three arguments");
                };
                let (texture, _) = self.sampler(sampler)?;
                return Ok(format!(
                    "textureLoad({texture}, {}, {})",
                    self.expr(at)?,
                    self.expr(level)?
                ));
            }
            "textureSize" => {
                let [sampler, level] = args else {
                    bail!("`textureSize` takes two arguments");
                };
                let (texture, _) = self.sampler(sampler)?;
                return Ok(format!(
                    "vec2<i32>(textureDimensions({texture}, {}))",
                    self.expr(level)?
                ));
            }
            _ => {}
        }
        let values = self.args(args)?;
        let joined = values.join(", ");
        Ok(match name {
            "float" | "int" | "uint" | "bool" | "vec2" | "vec3" | "vec4" | "ivec2" | "ivec3"
            | "ivec4" | "uvec2" | "uvec3" | "uvec4" | "bvec2" | "bvec3" | "bvec4" | "mat2"
            | "mat3" | "mat4" => format!("{}({joined})", wgsl_type(name)?),
            "atan" if values.len() == 2 => format!("atan2({joined})"),
            "mod" => {
                let [a, b] = values.as_slice() else {
                    bail!("`mod` takes two arguments");
                };
                format!("(({a}) - ({b}) * floor(({a}) / ({b})))")
            }
            "inversesqrt" => format!("inverseSqrt({joined})"),
            "dFdx" => format!("dpdx({joined})"),
            "dFdy" => format!("dpdy({joined})"),
            "roundEven" => format!("round({joined})"),
            "lessThan" | "greaterThan" | "lessThanEqual" | "greaterThanEqual" | "equal"
            | "notEqual" => {
                let [a, b] = values.as_slice() else {
                    bail!("`{name}` takes two arguments");
                };
                let op = match name {
                    "lessThan" => "<",
                    "greaterThan" => ">",
                    "lessThanEqual" => "<=",
                    "greaterThanEqual" => ">=",
                    "equal" => "==",
                    _ => "!=",
                };
                format!("({a} {op} {b})")
            }
            "inverse" | "isnan" | "isinf" | "outerProduct" | "matrixCompMult" | "textureGrad"
            | "textureProj" | "textureGather" | "packHalf2x16" | "unpackHalf2x16" => {
                bail!("`{name}` has no equivalent")
            }
            other => {
                let refs = self.by_ref.get(other).cloned().unwrap_or_default();
                let values: Vec<String> = values
                    .into_iter()
                    .enumerate()
                    .map(|(i, v)| {
                        if !refs.get(i).copied().unwrap_or(false) {
                            return v;
                        }
                        // A pointer the caller was itself handed passes on as it is.
                        match v.strip_prefix("(*").and_then(|p| p.strip_suffix(')')) {
                            Some(pointer) => pointer.to_string(),
                            None => format!("&{v}"),
                        }
                    })
                    .collect();
                format!("{}({})", sanitize(other), values.join(", "))
            }
        })
    }
}

//! A Godot `canvas_item` shader as WESL over `package::sprite`.
//!
//! Godot's shading language is GLSL with its own built-ins; WGSL differs in
//! spelling more than in meaning, so this parses the source into statements
//! and expressions and writes each back the way WGSL says it. What moves:
//!
//! - A `uniform` is a field of `Params`, read as `params.name`. WGSL's
//!   uniform block has no bool or int, so those are stored as `f32` and read
//!   back as the type the shader expects; a `sampler2D` is one of the four
//!   image slots a 2D material binds, and `hint_screen_texture` is the frame
//!   so far.
//! - `vertex()` and `fragment()` become `vs_main` and `fs_main`, with the
//!   built-ins they read as locals: `COLOR` starts as the texture times the
//!   tint, as Godot's does; `VERTEX` and `MODEL_MATRIX` are in pixels, y down,
//!   through the contract's pixel helpers.
//! - A `varying` rides a vertex output struct of the shader's own.
//! - `texture()` samples at level zero, which WGSL allows anywhere, where
//!   Godot's implicit level is only allowed in uniform control flow.
//! - Assigning several swizzled components, which WGSL cannot, goes through
//!   a temporary one component at a time.
//!
//! Anything with no equivalent (a light pass, a blend mode, a matrix
//! inverse) fails the translation with the reason, and the importer reports
//! it rather than writing a shader that draws something else.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use anyhow::{Result, anyhow, bail};

use crate::godot::shader_names::{sanitize, wgsl_type};
use crate::godot::shader_syntax::{Array, Expr, Function, Module, Parser, Stmt, lex};

mod expr;

/// Texture pixels per world unit, as every converted 2D node uses.
const PPU: &str = "100.0";

/// How many images a 2D material binds beside the node's own.
const SLOTS: usize = 4;

/// One `uniform`, as a material sets it.
#[derive(Clone, Debug)]
pub(crate) struct Uniform {
    pub name: String,
    /// Godot's type: `float`, `vec4`, `bool`, `sampler2D`, ...
    pub ty: String,
    pub source_color: bool,
    /// The declaration's default, flattened to numbers; a bool is 1 or 0.
    pub default: Option<Vec<f64>>,
    /// The image slot a sampler binds, from 1.
    pub slot: Option<usize>,
    pub screen: bool,
}

impl Uniform {
    /// Whether it is a field of `Params`, rather than a binding.
    fn numeric(&self) -> bool {
        !self.ty.starts_with("sampler")
    }
}

/// A translated shader.
pub(crate) struct Translated {
    pub wesl: String,
    pub uniforms: Vec<Uniform>,
    /// Whether it reads the frame so far, which its material has to ask for.
    pub screen: bool,
    pub notes: Vec<String>,
}

/// Translate one shader's source.
///
/// # Errors
/// When the shader is not a `canvas_item` one, will not parse, or uses
/// something WGSL or the sprite contract has no equivalent for.
pub(crate) fn translate(source: &str) -> Result<Translated> {
    let tokens = lex(source)?;
    let mut parser = Parser {
        tokens,
        at: 0,
        structs: BTreeSet::new(),
    };
    let module = parser.module()?;
    emit(&module)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Vertex,
    Fragment,
    Helper,
}

fn swizzle(member: &str) -> Option<String> {
    if member.is_empty() || member.len() > 4 {
        return None;
    }
    member
        .chars()
        .map(|c| match c {
            'x' | 'r' | 's' => Some('x'),
            'y' | 'g' | 't' => Some('y'),
            'z' | 'b' | 'p' => Some('z'),
            'w' | 'a' | 'q' => Some('w'),
            _ => None,
        })
        .collect()
}

struct Emitter {
    uniforms: BTreeMap<String, Uniform>,
    stage: Stage,
    /// `out`/`inout` parameters of the function being written, read through.
    refs: BTreeSet<String>,
    /// Each function's by-reference parameters, for its call sites.
    by_ref: BTreeMap<String, Vec<bool>>,
    returns: BTreeMap<String, String>,
    imports: BTreeSet<&'static str>,
    screen: bool,
    /// Whether the fragment stage reads `VERTEX`, which then rides a varying.
    fragment_vertex: bool,
    /// Every varying the vertex stage hands on, as `(type, name)`, once the
    /// fragment stage has said whether it reads `VERTEX`.
    outputs: Vec<(String, String)>,
    temps: usize,
}

fn emit(module: &Module) -> Result<Translated> {
    let mut notes = render_modes(&module.render_modes);
    let (uniforms, listed) = uniforms_of(module)?;
    let mut e = Emitter {
        uniforms,
        stage: Stage::Helper,
        refs: BTreeSet::new(),
        by_ref: module
            .functions
            .iter()
            .map(|f| (f.name.clone(), f.params.iter().map(|p| p.by_ref).collect()))
            .collect(),
        returns: module
            .functions
            .iter()
            .map(|f| (f.name.clone(), f.ret.clone()))
            .collect(),
        imports: BTreeSet::new(),
        screen: false,
        fragment_vertex: false,
        outputs: Vec::new(),
        temps: 0,
    };
    let mut body = String::new();
    // Structs, then consts, then helpers, then the stages: WGSL takes them in
    // any order, and this is the order a reader expects.
    for (name, fields) in &module.structs {
        writeln!(body, "struct {} {{", sanitize(name))?;
        for (ty, field, array) in fields {
            writeln!(
                body,
                "    {}: {},",
                sanitize(field),
                typed(ty, *array, None)?
            )?;
        }
        writeln!(body, "}}\n")?;
    }
    for decl in &module.consts {
        e.global_const(decl, &mut body)?;
    }
    let mut vertex = None;
    let mut fragment = None;
    for function in &module.functions {
        match function.name.as_str() {
            "vertex" => vertex = Some(function),
            "fragment" => fragment = Some(function),
            "light" => {
                notes.push("light(): lights here do not run a shader of their own; dropped".into());
            }
            _ => {
                e.stage = Stage::Helper;
                e.helper(function, &mut body)?;
            }
        }
    }
    // The fragment first: whether it reads `VERTEX` decides the varyings
    // the vertex stage hands on.
    e.stage = Stage::Fragment;
    let fragment_body = match fragment {
        Some(f) => e.block_of(&f.body, 1)?,
        None => String::new(),
    };
    let mut varyings: Vec<(String, String)> = module
        .varyings
        .iter()
        .map(|(ty, name)| Ok((wgsl_type(ty)?, sanitize(name))))
        .collect::<Result<_>>()?;
    if e.fragment_vertex {
        varyings.push(("vec2<f32>".into(), "godot_vertex".into()));
    }
    e.outputs.clone_from(&varyings);
    e.stage = Stage::Vertex;
    let vertex_body = match vertex {
        Some(f) => Some(e.block_of(&f.body, 1)?),
        None => None,
    };
    let mut stages = e.vertex_stage(vertex_body.as_deref(), &varyings)?;
    stages.push_str(&e.fragment_stage(&fragment_body, &varyings)?);

    let mut out = String::from("// Translated from a Godot shader by `balaur import`.\n");
    let imports: Vec<&str> = e.imports.iter().copied().collect();
    writeln!(out, "import package::sprite::{{{}}};\n", imports.join(", "))?;
    out.push_str(&params_block(&listed)?);
    out.push_str(&body);
    out.push_str(&stages);
    Ok(Translated {
        wesl: out,
        uniforms: listed,
        screen: e.screen,
        notes,
    })
}

/// What a shader's `render_mode` asks that a material here does not do.
fn render_modes(modes: &[String]) -> Vec<String> {
    modes
        .iter()
        .filter_map(|mode| match mode.as_str() {
            "blend_mix" | "unshaded" => None,
            "blend_add" | "blend_sub" | "blend_mul" | "blend_premul_alpha" | "blend_disabled" => {
                Some(format!(
                    "render_mode {mode}: materials here blend as `blend_mix`"
                ))
            }
            other => Some(format!(
                "render_mode {other} has no equivalent and was dropped"
            )),
        })
        .collect()
}

/// Every uniform with its default read and its image slot given, by name
/// and in declaration order.
fn uniforms_of(module: &Module) -> Result<(BTreeMap<String, Uniform>, Vec<Uniform>)> {
    let mut uniforms = BTreeMap::new();
    let mut slot = 0;
    let mut listed = Vec::new();
    for (uniform, init) in &module.uniforms {
        let mut uniform = uniform.clone();
        uniform.default = init.as_ref().and_then(constant);
        if uniform.ty.starts_with("sampler") && !uniform.screen {
            slot += 1;
            if slot > SLOTS {
                bail!("more than {SLOTS} images: a 2D material binds four");
            }
            uniform.slot = Some(slot);
        }
        if uniform.ty.starts_with("mat") {
            bail!("a matrix uniform has no equivalent");
        }
        listed.push(uniform.clone());
        uniforms.insert(uniform.name.clone(), uniform);
    }
    Ok((uniforms, listed))
}

/// The `Params` struct and its binding, for a shader with numeric uniforms.
fn params_block(listed: &[Uniform]) -> Result<String> {
    let fields: Vec<&Uniform> = listed.iter().filter(|u| u.numeric()).collect();
    let mut out = String::new();
    if fields.is_empty() {
        return Ok(out);
    }
    writeln!(out, "struct Params {{")?;
    for uniform in &fields {
        let ty = match uniform.ty.as_str() {
            "bool" | "int" | "uint" | "float" => "f32".to_string(),
            "vec2" | "vec3" | "vec4" => format!("{}<f32>", uniform.ty),
            other => bail!("a `{other}` uniform has no equivalent"),
        };
        writeln!(out, "    {}: {ty},", sanitize(&uniform.name))?;
    }
    writeln!(out, "}}")?;
    writeln!(out, "@group(3) @binding(0) var<uniform> params: Params;\n")?;
    Ok(out)
}

/// A declared type, as an array when it carries a suffix; one left unsized
/// takes its initializer's count.
fn typed(ty: &str, array: Option<Array>, init: Option<&Expr>) -> Result<String> {
    let base = wgsl_type(ty)?;
    let Some(array) = array else {
        return Ok(base);
    };
    let size = match (array, init) {
        (Array::Sized(n), _) => n,
        (Array::Unsized, Some(Expr::Array(_, n, args))) => n.unwrap_or(args.len()),
        (Array::Unsized, _) => bail!("an array with no size"),
    };
    Ok(format!("array<{base}, {size}>"))
}

/// The vertex stage's last word: every output handed to `godot_finish`.
fn finish(varyings: &[(String, String)]) -> String {
    let extra = varyings.iter().fold(String::new(), |mut extra, (_, name)| {
        let name = if name == "godot_vertex" {
            "VERTEX"
        } else {
            name
        };
        let _ = write!(extra, ", {name}");
        extra
    });
    format!("return godot_finish(in, godot_base, VERTEX, UV, COLOR{extra});")
}

/// Every name a statement assigns to, through a swizzle or an index too.
fn assigned_in(stmt: &Stmt, names: &mut BTreeSet<String>) {
    fn root(e: &Expr) -> Option<&str> {
        match e {
            Expr::Ident(name) => Some(name),
            Expr::Member(base, _) | Expr::Index(base, _) => root(base),
            _ => None,
        }
    }
    fn expr(e: &Expr, names: &mut BTreeSet<String>) {
        if let Expr::Assign(_, target, _) | Expr::Step(_, target) = e
            && let Some(name) = root(target)
        {
            names.insert(name.to_string());
        }
    }
    match stmt {
        Stmt::Expr(e) => expr(e, names),
        Stmt::Block(body) => {
            for s in body {
                assigned_in(s, names);
            }
        }
        Stmt::If(_, then, other) => {
            assigned_in(then, names);
            if let Some(other) = other {
                assigned_in(other, names);
            }
        }
        Stmt::For(init, _, step, body) => {
            if let Some(init) = init {
                assigned_in(init, names);
            }
            if let Some(step) = step {
                expr(step, names);
            }
            assigned_in(body, names);
        }
        Stmt::While(_, body) | Stmt::DoWhile(body, _) => assigned_in(body, names),
        Stmt::Switch(_, arms) => {
            for (_, body) in arms {
                for s in body {
                    assigned_in(s, names);
                }
            }
        }
        Stmt::Decl { .. } | Stmt::Return(_) | Stmt::Break | Stmt::Continue | Stmt::Discard => {}
    }
}

/// A uniform's default, when it is a constant this can read: a number, a
/// bool, or a vector of them.
fn constant(expr: &Expr) -> Option<Vec<f64>> {
    match expr {
        Expr::Num(n) => Some(vec![n.trim_end_matches(['f', 'u']).parse().ok()?]),
        Expr::Ident(word) if word == "true" => Some(vec![1.0]),
        Expr::Ident(word) if word == "false" => Some(vec![0.0]),
        Expr::Unary("-", inner) => Some(constant(inner)?.into_iter().map(|v| -v).collect()),
        Expr::Call(ty, args) => {
            let values: Vec<f64> = args
                .iter()
                .map(constant)
                .collect::<Option<Vec<_>>>()?
                .into_iter()
                .flatten()
                .collect();
            let width = match ty.as_str() {
                "vec2" | "ivec2" => 2,
                "vec3" | "ivec3" => 3,
                "vec4" | "ivec4" => 4,
                "float" | "int" => 1,
                _ => return None,
            };
            match values.len() {
                1 => Some(vec![values[0]; width]),
                n if n == width => Some(values),
                _ => None,
            }
        }
        _ => None,
    }
}

impl Emitter {
    /// `vs_main`, and the vertex output struct and the finishing function a
    /// Godot `vertex()` or a varying needs; `body` is `vertex()`'s, written.
    fn vertex_stage(
        &mut self,
        body: Option<&str>,
        varyings: &[(String, String)],
    ) -> Result<String> {
        let mut out = String::new();
        let own_output = !varyings.is_empty();
        let output = if own_output {
            "Varyings"
        } else {
            "VertexOutput"
        };
        self.imports.extend(["VertexInput", "VertexOutput"]);
        if own_output {
            writeln!(out, "struct Varyings {{")?;
            writeln!(out, "    @builtin(position) clip_position: vec4<f32>,")?;
            writeln!(out, "    @location(0) uv: vec2<f32>,")?;
            writeln!(out, "    @location(1) color: vec4<f32>,")?;
            writeln!(out, "    @location(2) world: vec2<f32>,")?;
            for (i, (ty, name)) in varyings.iter().enumerate() {
                let flat = if ty.contains("i32") || ty.contains("u32") {
                    " @interpolate(flat)"
                } else {
                    ""
                };
                writeln!(out, "    @location({}){flat} {name}: {ty},", i + 3)?;
            }
            writeln!(out, "}}\n")?;
        }
        if body.is_none() && !own_output {
            self.imports.insert("vertex");
            writeln!(
                out,
                "@vertex fn vs_main(in: VertexInput) -> VertexOutput {{"
            )?;
            writeln!(out, "    return vertex(in);\n}}\n")?;
            return Ok(out);
        }
        self.imports
            .extend(["place", "vertex_pixels", "pixels_to_offset"]);
        let extra = varyings
            .iter()
            .fold(String::new(), |mut extra, (ty, name)| {
                let _ = write!(extra, ", {name}: {ty}");
                extra
            });
        writeln!(
            out,
            "fn godot_finish(in: VertexInput, base: vec2<f32>, VERTEX: vec2<f32>, UV: vec2<f32>, COLOR: vec4<f32>{extra}) -> {output} {{"
        )?;
        writeln!(
            out,
            "    let placed = place(in, pixels_to_offset(VERTEX - base, {PPU}));"
        )?;
        writeln!(out, "    var out: {output};")?;
        writeln!(out, "    out.clip_position = placed.clip_position;")?;
        writeln!(out, "    out.uv = UV;")?;
        writeln!(out, "    out.color = COLOR;")?;
        writeln!(out, "    out.world = placed.world;")?;
        for (_, name) in varyings {
            writeln!(out, "    out.{name} = {name};")?;
        }
        writeln!(out, "    return out;\n}}\n")?;
        writeln!(out, "@vertex fn vs_main(in: VertexInput) -> {output} {{")?;
        writeln!(out, "    let godot_base = vertex_pixels(in, {PPU});")?;
        writeln!(out, "    var VERTEX = godot_base;")?;
        writeln!(out, "    var UV = in.uv;")?;
        writeln!(out, "    var COLOR = in.instance_color;")?;
        for (ty, name) in varyings.iter().filter(|(_, name)| name != "godot_vertex") {
            writeln!(out, "    var {name}: {ty};")?;
        }
        out.push_str(body.unwrap_or_default());
        writeln!(out, "    {}", finish(varyings))?;
        writeln!(out, "}}\n")?;
        Ok(out)
    }

    /// `fs_main`: `COLOR` starts as the texture times the tint, as Godot's
    /// does, and is what it returns; `body` is `fragment()`'s, written.
    fn fragment_stage(&mut self, body: &str, varyings: &[(String, String)]) -> Result<String> {
        let mut out = String::new();
        let own_output = !varyings.is_empty();
        let output = if own_output {
            "Varyings"
        } else {
            "VertexOutput"
        };
        self.imports.extend(["sample_albedo", "tint"]);
        writeln!(
            out,
            "@fragment fn fs_main(in: {output}) -> @location(0) vec4<f32> {{"
        )?;
        let base = if own_output {
            writeln!(out, "    var godot_in: VertexOutput;")?;
            for field in ["clip_position", "uv", "color", "world"] {
                writeln!(out, "    godot_in.{field} = in.{field};")?;
            }
            "godot_in"
        } else {
            "in"
        };
        writeln!(out, "    var UV = in.uv;")?;
        writeln!(out, "    var COLOR = sample_albedo(in.uv) * tint({base});")?;
        for (_, name) in varyings.iter().filter(|(_, name)| name != "godot_vertex") {
            writeln!(out, "    let {name} = in.{name};")?;
        }
        out.push_str(body);
        writeln!(out, "    return COLOR;\n}}")?;
        Ok(out)
    }

    fn global_const(&mut self, decl: &Stmt, out: &mut String) -> Result<()> {
        let Stmt::Decl {
            ty, array, vars, ..
        } = decl
        else {
            return Ok(());
        };
        for var in vars {
            let array = var.array.or(*array);
            let init = var
                .init
                .as_ref()
                .ok_or_else(|| anyhow!("const `{}` has no value", var.name))?;
            let ty = typed(ty, array, Some(init))?;
            let value = self.expr(init)?;
            // An array is indexed at run time, which a WGSL const cannot be.
            if array.is_some() {
                writeln!(
                    out,
                    "var<private> {}: {ty} = {value};\n",
                    sanitize(&var.name)
                )?;
            } else {
                writeln!(out, "const {}: {ty} = {value};\n", sanitize(&var.name))?;
            }
        }
        Ok(())
    }

    fn helper(&mut self, function: &Function, out: &mut String) -> Result<()> {
        self.refs = function
            .params
            .iter()
            .filter(|p| p.by_ref)
            .map(|p| p.name.clone())
            .collect();
        // A GLSL parameter is the function's own copy and may be assigned;
        // a WGSL one may not, so one the body assigns is copied in.
        let mut assigned = BTreeSet::new();
        for stmt in &function.body {
            assigned_in(stmt, &mut assigned);
        }
        let mut copies = String::new();
        let params: Vec<String> = function
            .params
            .iter()
            .map(|p| {
                let ty = wgsl_type(&p.ty)?;
                let name = sanitize(&p.name);
                Ok(if p.by_ref {
                    format!("{name}: ptr<function, {ty}>")
                } else if assigned.contains(&p.name) {
                    writeln!(copies, "    var {name} = {name}_in;")?;
                    format!("{name}_in: {ty}")
                } else {
                    format!("{name}: {ty}")
                })
            })
            .collect::<Result<_>>()?;
        let ret = if function.ret == "void" {
            String::new()
        } else {
            format!(" -> {}", wgsl_type(&function.ret)?)
        };
        writeln!(
            out,
            "fn {}({}){ret} {{",
            sanitize(&function.name),
            params.join(", ")
        )?;
        out.push_str(&copies);
        out.push_str(&self.block_of(&function.body, 1)?);
        writeln!(out, "}}\n")?;
        self.refs.clear();
        Ok(())
    }

    fn block_of(&mut self, body: &[Stmt], depth: usize) -> Result<String> {
        let mut out = String::new();
        for stmt in body {
            self.statement(stmt, depth, &mut out)?;
        }
        Ok(out)
    }

    fn statement(&mut self, stmt: &Stmt, depth: usize, out: &mut String) -> Result<()> {
        let pad = "    ".repeat(depth);
        match stmt {
            Stmt::Block(body) => {
                writeln!(out, "{pad}{{")?;
                out.push_str(&self.block_of(body, depth + 1)?);
                writeln!(out, "{pad}}}")?;
            }
            Stmt::Decl {
                constant,
                ty,
                array,
                vars,
            } => {
                for var in vars {
                    let array = var.array.or(*array);
                    let ty = typed(ty, array, var.init.as_ref())?;
                    let keyword = if *constant { "let" } else { "var" };
                    match &var.init {
                        Some(init) => writeln!(
                            out,
                            "{pad}{keyword} {}: {ty} = {};",
                            sanitize(&var.name),
                            self.expr(init)?
                        )?,
                        None => writeln!(out, "{pad}var {}: {ty};", sanitize(&var.name))?,
                    }
                }
            }
            Stmt::Expr(e) => writeln!(out, "{pad}{}", self.effect(e)?)?,
            Stmt::If(cond, then, other) => {
                writeln!(out, "{pad}if ({}) {{", self.expr(cond)?)?;
                self.body(then, depth + 1, out)?;
                match other {
                    Some(other) => {
                        writeln!(out, "{pad}}} else {{")?;
                        self.body(other, depth + 1, out)?;
                        writeln!(out, "{pad}}}")?;
                    }
                    None => writeln!(out, "{pad}}}")?,
                }
            }
            Stmt::For(init, cond, step, body) => {
                let head = self.for_head(init.as_deref(), cond.as_ref(), step.as_ref())?;
                writeln!(out, "{pad}for ({head}) {{")?;
                self.body(body, depth + 1, out)?;
                writeln!(out, "{pad}}}")?;
            }
            Stmt::While(cond, body) => {
                writeln!(out, "{pad}while ({}) {{", self.expr(cond)?)?;
                self.body(body, depth + 1, out)?;
                writeln!(out, "{pad}}}")?;
            }
            Stmt::DoWhile(body, cond) => {
                writeln!(out, "{pad}loop {{")?;
                self.body(body, depth + 1, out)?;
                writeln!(out, "{pad}    if !({}) {{ break; }}", self.expr(cond)?)?;
                writeln!(out, "{pad}}}")?;
            }
            Stmt::Switch(on, arms) => self.switch(on, arms, depth, out)?,
            Stmt::Return(value) => match (self.stage, value) {
                (Stage::Fragment, None) => writeln!(out, "{pad}return COLOR;")?,
                (Stage::Vertex, None) => {
                    writeln!(out, "{pad}{}", finish(&self.outputs))?;
                }
                (_, Some(v)) => writeln!(out, "{pad}return {};", self.expr(v)?)?,
                (Stage::Helper, None) => writeln!(out, "{pad}return;")?,
            },
            Stmt::Break => writeln!(out, "{pad}break;")?,
            Stmt::Continue => writeln!(out, "{pad}continue;")?,
            Stmt::Discard => writeln!(out, "{pad}discard;")?,
        }
        Ok(())
    }

    /// What goes between a `for`'s parentheses: one declaration or effect,
    /// the condition, and the step.
    fn for_head(
        &mut self,
        init: Option<&Stmt>,
        cond: Option<&Expr>,
        step: Option<&Expr>,
    ) -> Result<String> {
        let init = match init {
            Some(Stmt::Decl { ty, vars, .. }) if vars.len() == 1 => {
                let var = &vars[0];
                let value = match &var.init {
                    Some(v) => format!(" = {}", self.expr(v)?),
                    None => String::new(),
                };
                format!("var {}: {}{value}", sanitize(&var.name), wgsl_type(ty)?)
            }
            Some(Stmt::Expr(e)) => self.effect(e)?.trim_end_matches(';').to_string(),
            Some(_) => bail!("a `for` that declares more than one variable"),
            None => String::new(),
        };
        let cond = cond.map(|c| self.expr(c)).transpose()?.unwrap_or_default();
        let step = match step {
            Some(e) => self.effect(e)?.trim_end_matches(';').to_string(),
            None => String::new(),
        };
        Ok(format!("{init}; {cond}; {step}"))
    }

    /// A switch, its fall-through labels gathered onto the arm that has a
    /// body, since a WGSL case never falls through.
    fn switch(
        &mut self,
        on: &Expr,
        arms: &[(Option<Expr>, Vec<Stmt>)],
        depth: usize,
        out: &mut String,
    ) -> Result<()> {
        let pad = "    ".repeat(depth);
        writeln!(out, "{pad}switch ({}) {{", self.expr(on)?)?;
        let mut labels: Vec<String> = Vec::new();
        for (label, body) in arms {
            labels.push(match label {
                Some(e) => self.expr(e)?,
                None => "default".into(),
            });
            if body.is_empty() {
                continue;
            }
            let body: Vec<Stmt> = body
                .iter()
                .filter(|s| !matches!(s, Stmt::Break))
                .cloned()
                .collect();
            let all = labels.join(", ");
            let head = if all == "default" {
                "default".to_string()
            } else {
                format!("case {all}")
            };
            writeln!(out, "{pad}    {head}: {{")?;
            out.push_str(&self.block_of(&body, depth + 2)?);
            writeln!(out, "{pad}    }}")?;
            labels.clear();
        }
        writeln!(out, "{pad}}}")?;
        Ok(())
    }

    /// A nested statement's body, braces already written by the caller.
    fn body(&mut self, stmt: &Stmt, depth: usize, out: &mut String) -> Result<()> {
        match stmt {
            Stmt::Block(body) => out.push_str(&self.block_of(body, depth)?),
            other => self.statement(other, depth, out)?,
        }
        Ok(())
    }
}

/// The `Params` field a uniform is read from, which a material names.
pub(crate) fn field_name(uniform: &str) -> String {
    sanitize(uniform)
}

/// Link a translation against the sprite contract and type-check the WGSL
/// it links to: what `create_shader_module` would otherwise find on a GPU.
pub(crate) fn check(translated: &Translated) -> Result<()> {
    let material = balaur_render::material::Material {
        shader: "shader.wesl".into(),
        features: vec![("screen".into(), translated.screen)],
        ..Default::default()
    };
    let compiled = balaur_render::material::compile(&material, &translated.wesl)?;
    let module = naga::front::wgsl::parse_str(&compiled.wgsl)
        .map_err(|why| anyhow!("{}", why.emit_to_string(&compiled.wgsl)))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .map_err(|why| anyhow!("{}", why.emit_to_string(&compiled.wgsl)))?;
    Ok(())
}

/// sRGB to linear, for a `source_color` a material sets: the engine blends
/// in linear light and Godot's canvas does not.
pub(crate) fn linear(channel: f64) -> f64 {
    if channel <= 0.040_45 {
        channel / 12.92
    } else {
        balaur_core::libm::pow((channel + 0.055) / 1.055, 2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::translate;

    /// Every translation has to link against the real contract and pass
    /// naga's checks.
    fn compiles(source: &str) -> String {
        let translated = translate(source).unwrap_or_else(|why| panic!("{why:#}"));
        super::check(&translated).unwrap_or_else(|why| panic!("{why:#}\n{}", translated.wesl));
        translated.wesl
    }

    #[test]
    fn a_tint_shader_reads_its_uniforms_through_params() {
        let wesl = compiles(
            "shader_type canvas_item;
             uniform vec4 glow_color : source_color = vec4(1.0, 1.0, 0.5, 1.0);
             uniform float glow_intensity : hint_range(0.0, 10.0) = 2.0;
             void fragment() {
                 vec4 tex_color = texture(TEXTURE, UV);
                 vec3 emission = glow_color.rgb * glow_intensity;
                 COLOR = tex_color + vec4(emission, tex_color.a);
             }",
        );
        assert!(wesl.contains("params.glow_color"), "{wesl}");
    }

    #[test]
    fn a_bool_uniform_an_if_without_braces_and_a_const_array_carry() {
        compiles(
            "shader_type canvas_item;
             uniform bool use_mask = true;
             uniform int samples : hint_range(4, 8) = 8;
             const vec2[4] DIRS = vec2[4](vec2(1.0, 0.0), vec2(0.0, 1.0), vec2(-1.0, 0.0), vec2(0.0, -1.0));
             void fragment() {
                 vec4 tex = texture(TEXTURE, UV);
                 float strength = 0.0;
                 for (int i = 0; i < samples; i++)
                     for (float d = 1.0; d <= 2.0; d++) {
                         if (texture(TEXTURE, UV + TEXTURE_PIXEL_SIZE * DIRS[i] * d).a > 0.1) {
                             strength += exp(-d);
                             break;
                         }
                     }
                 if (use_mask) COLOR = vec4(1.0, 1.0, 1.0, 1.0 - tex.a); else COLOR.rgb = mix(tex.rgb, vec3(strength), 0.5);
                 if (tex.a <= 0.0) discard;
             }",
        );
    }

    #[test]
    fn a_vertex_stage_and_a_varying_ride_the_shaders_own_output() {
        compiles(
            "shader_type canvas_item;
             uniform float speed = 1.0;
             varying float world_y;
             float wind(vec2 uv, float t) { return sin(t) * max(0.0, 1.0 - uv.y); }
             void vertex() {
                 world_y = (MODEL_MATRIX * vec4(VERTEX, 0.0, 1.0)).y;
                 VERTEX.x += wind(UV, TIME * speed);
             }
             void fragment() {
                 COLOR.a *= smoothstep(0.0, 48.0, world_y);
             }",
        );
    }

    #[test]
    fn an_image_uniform_takes_a_slot_and_the_screen_is_the_frame_so_far() {
        let translated = translate(
            "shader_type canvas_item;
             uniform sampler2D noise_tex;
             uniform sampler2D SCREEN_TEXTURE : hint_screen_texture, filter_linear_mipmap;
             void fragment() {
                 float n = texture(noise_tex, UV).r;
                 COLOR = textureLod(SCREEN_TEXTURE, SCREEN_UV, 0.0) * n;
             }",
        )
        .unwrap();
        assert!(translated.screen);
        assert_eq!(translated.uniforms[0].slot, Some(1));
        compiles(
            "shader_type canvas_item;
             uniform sampler2D noise_tex;
             uniform sampler2D SCREEN_TEXTURE : hint_screen_texture, filter_linear_mipmap;
             void fragment() {
                 float n = texture(noise_tex, UV).r;
                 COLOR = textureLod(SCREEN_TEXTURE, SCREEN_UV, 0.0) * n;
             }",
        );
    }

    #[test]
    fn a_light_pass_or_a_spatial_shader_says_why_it_did_not_carry() {
        let why = translate("shader_type spatial; void fragment() {}")
            .err()
            .unwrap();
        assert!(format!("{why}").contains("spatial"));
        let why = translate("shader_type canvas_item; void fragment() { NORMAL = vec3(0.0); }")
            .err()
            .unwrap();
        assert!(format!("{why}").contains("NORMAL"));
    }
}

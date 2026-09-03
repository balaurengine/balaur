//! Drawing the value at one line instead of the shader's own colour.
//!
//! The caret names a declaration. Its value is written to a private global at
//! that point and returned in place of the fragment's colour, so a value
//! computed inside a helper or a branch is still what comes out — and a pixel
//! that never reached the line keeps the picture it had, which is itself the
//! answer to "which pixels get here".
//!
//! The rewrite is spliced at byte offsets the syntax tree gives, never by
//! matching text: the AST is what says which statement a line holds and where
//! a `return` ends.

use anyhow::{anyhow, bail, Result};
use wesl::syntax::{
    Attribute, CompoundStatement, Expression, GlobalDeclaration, Statement, StatementNode,
    TranslationUnit,
};

/// The channel the rewritten shader draws through. Zero-initialised, so a
/// pixel that never reached the line reports `false`.
const CHANNEL: &str = "var<private> balaur_preview: vec4<f32>;
var<private> balaur_preview_hit: bool;
";

/// A shader rewritten to draw one of its own values.
#[derive(Debug)]
pub struct Preview {
    /// The source to link, with the channel spliced in.
    pub source: String,
    /// The name being drawn.
    pub name: String,
    /// Its type, which is what decided the encoding.
    pub ty: String,
}

/// The byte range of a 1-based line.
fn line_span(source: &str, line: usize) -> Result<(usize, usize)> {
    let mut offset = 0;
    for (number, text) in source.lines().enumerate() {
        if number + 1 == line {
            return Ok((offset, offset + text.len()));
        }
        offset += text.len() + 1;
    }
    Err(anyhow!("the shader has no line {line}"))
}

/// Every statement in `body`, outer before inner.
fn walk<'a>(body: &'a CompoundStatement, out: &mut Vec<&'a StatementNode>) {
    for statement in &body.statements {
        out.push(statement);
        match statement.node() {
            Statement::Compound(inner) => walk(inner, out),
            Statement::If(s) => {
                walk(&s.if_clause.body, out);
                for clause in &s.else_if_clauses {
                    walk(&clause.body, out);
                }
                if let Some(clause) = &s.else_clause {
                    walk(&clause.body, out);
                }
            }
            Statement::Loop(s) => walk(&s.body, out),
            Statement::For(s) => walk(&s.body, out),
            Statement::While(s) => walk(&s.body, out),
            _ => {}
        }
    }
}

/// How a value of `ty` becomes the four channels the preview draws.
fn encode(ty: &str, name: &str) -> Result<String> {
    Ok(match ty {
        "f32" => format!("vec4<f32>({name}, {name}, {name}, 1.0)"),
        "vec2" | "vec2f" => format!("vec4<f32>({name}, 0.0, 1.0)"),
        "vec3" | "vec3f" => format!("vec4<f32>({name}, 1.0)"),
        "vec4" | "vec4f" => name.to_string(),
        other => {
            bail!("`{other}` is not a value the preview can draw: f32, vec2, vec3 and vec4 are")
        }
    })
}

/// The declared type of `declaration`, from its annotation or its
/// initialiser's constructor.
///
/// Nothing else is inferred: WESL's own type checker is not exposed, and a
/// wrong guess would draw a shader that does not compile.
fn declared_type(declaration: &wesl::syntax::Declaration) -> Result<String> {
    if let Some(ty) = &declaration.ty {
        return Ok(ty.ident.name().to_string());
    }
    if let Some(initializer) = &declaration.initializer {
        if let Expression::FunctionCall(call) = initializer.node() {
            return Ok(call.ty.ident.name().to_string());
        }
    }
    Err(anyhow!(
        "`{}` has no written type, so there is nothing to say how to draw it; \
         annotate it (`let x: vec3<f32> = ...`) to preview this line",
        declaration.ident.name()
    ))
}

/// Rewrite `source` so its fragment stage draws the value declared on `line`.
pub fn preview(source: &str, line: usize) -> Result<Preview> {
    let unit: TranslationUnit = source
        .parse()
        .map_err(|e| anyhow!("parsing the shader: {e}"))?;
    let (from, to) = line_span(source, line)?;

    let mut statements = Vec::new();
    let mut fragment = Vec::new();
    for global in &unit.global_declarations {
        let GlobalDeclaration::Function(function) = global.node() else {
            continue;
        };
        walk(&function.body, &mut statements);
        if function
            .attributes
            .iter()
            .any(|a| matches!(a.node(), Attribute::Fragment))
        {
            walk(&function.body, &mut fragment);
        }
    }
    if fragment.is_empty() {
        bail!("the shader has no fragment stage, so it draws nothing to replace");
    }

    // Innermost first: `walk` pushes a block before what it contains.
    let found = statements
        .iter()
        .rev()
        .find(|s| {
            matches!(s.node(), Statement::Declaration(_))
                && s.span().start < to
                && s.span().end > from
        })
        .ok_or_else(|| anyhow!("line {line} declares nothing to preview"))?;
    let Statement::Declaration(declaration) = found.node() else {
        unreachable!("the search matched on this");
    };
    let name = declaration.ident.name().to_string();
    let ty = declared_type(declaration)?;

    // Highest offset first, so earlier edits do not move later ones.
    let mut edits: Vec<(usize, usize, String)> = vec![(
        found.span().end,
        found.span().end,
        format!(
            " balaur_preview = {}; balaur_preview_hit = true;",
            encode(&ty, &name)?
        ),
    )];
    for statement in &fragment {
        let Statement::Return(returning) = statement.node() else {
            continue;
        };
        let Some(expression) = &returning.expression else {
            continue;
        };
        let span = expression.span();
        edits.push((
            span.start,
            span.end,
            format!(
                "select({}, balaur_preview, balaur_preview_hit)",
                &source[span.start..span.end]
            ),
        ));
    }
    edits.sort_by_key(|(start, ..)| std::cmp::Reverse(*start));

    let mut out = source.to_string();
    for (start, end, text) in edits {
        out.replace_range(start..end, &text);
    }
    Ok(Preview {
        source: format!("{CHANNEL}{out}"),
        name,
        ty,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Line 5 declares `tinted`; line 10 declares `shade`, inside a helper.
    const SHADER: &str = r"
@fragment fn fs_main() -> @location(0) vec4<f32> {
    let base = vec3<f32>(0.25, 0.5, 0.75);
    let tinted: vec3<f32> = base * lift(0.5);
    if base.x > 0.0 {
        return vec4<f32>(tinted, 1.0);
    }
    return vec4<f32>(base, 1.0);
}

fn lift(x: f32) -> f32 {
    let shade: f32 = x * 2.0;
    return shade;
}
";

    fn linked(source: &str) -> String {
        crate::shaders::link(&[("package::p", source)], "package::p", &[])
            .map(|unit| crate::shaders::wgsl(&unit))
            .expect("a preview must be a shader that links")
    }

    #[test]
    fn the_value_at_the_line_replaces_the_colour() {
        let preview = preview(SHADER, 4).unwrap();
        assert_eq!(preview.name, "tinted");
        assert_eq!(preview.ty, "vec3");
        let wgsl = linked(&preview.source);
        assert!(wgsl.contains("balaur_preview_hit = true"), "{wgsl}");
        // Both returns are diverted, not only the last.
        assert_eq!(wgsl.matches("select(").count(), 2, "{wgsl}");
    }

    #[test]
    fn a_value_inside_a_helper_is_previewable() {
        // The whole point of writing to a channel rather than returning
        // early: `lift` has nowhere to return the preview to.
        let preview = preview(SHADER, 12).unwrap();
        assert_eq!(preview.name, "shade");
        assert_eq!(preview.ty, "f32");
        let wgsl = linked(&preview.source);
        assert!(wgsl.contains("fn lift"), "{wgsl}");
        assert!(wgsl.contains("balaur_preview_hit = true"), "{wgsl}");
    }

    #[test]
    fn a_float_is_drawn_as_grey() {
        assert_eq!(encode("f32", "x").unwrap(), "vec4<f32>(x, x, x, 1.0)");
    }

    #[test]
    fn a_vec2_fills_red_and_green() {
        assert_eq!(encode("vec2", "v").unwrap(), "vec4<f32>(v, 0.0, 1.0)");
    }

    #[test]
    fn a_vec4_is_drawn_as_it_is() {
        assert_eq!(encode("vec4", "v").unwrap(), "v");
    }

    #[test]
    fn a_matrix_says_it_cannot_be_drawn() {
        let err = encode("mat3x3", "m").unwrap_err();
        assert!(format!("{err}").contains("mat3x3"), "{err}");
    }

    #[test]
    fn a_line_that_declares_nothing_says_so() {
        let err = preview(SHADER, 6).unwrap_err();
        assert!(format!("{err}").contains("declares nothing"), "{err}");
    }

    #[test]
    fn an_untyped_declaration_asks_for_an_annotation() {
        let source = "@fragment fn fs_main() -> @location(0) vec4<f32> {
            let a = 1.0;
            let b = a * 2.0;
            return vec4<f32>(b);
        }";
        let err = preview(source, 3).unwrap_err();
        assert!(format!("{err}").contains("annotate it"), "{err}");
    }

    #[test]
    fn a_shader_with_no_fragment_stage_says_so() {
        let source = "@vertex fn vs_main() -> @builtin(position) vec4<f32> {
            let p = vec4<f32>(0.0);
            return p;
        }";
        let err = preview(source, 2).unwrap_err();
        assert!(format!("{err}").contains("no fragment stage"), "{err}");
    }
}

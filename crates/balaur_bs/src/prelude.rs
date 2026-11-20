//! What every program has without a `use`: the engine's vectors and colours,
//! the methods of `str` and `any`, and the modules a script calls by name,
//! `encoding`, `quat` and parts of `audio`, `random` and `geometry2d`,
//! written in Balaur Script. A method is its module's `pub` function, named as `prefix` names a linked module's
//! (`str::find`, a helper `str::#part`), and a program compiles one in only
//! once a call reaches it, as it does a release (`codegen::Gen::callee`).
//! Only these functions may call the built-ins that read a string's bytes or
//! a cell's parts (`codegen::call`'s `intrinsic`).

use crate::Error;
use crate::ast::{Expr, FnDecl, Program, Ty};
use crate::vocabulary::words;
use std::rc::Rc;

/// The structs every program declares.
pub(crate) const CORE: &str = include_str!("prelude/core.bs");

/// Each built-in type's methods: its name and its module's sources.
const METHODS: [(&str, &[&str]); 2] = [
    (
        words::keywords::STR,
        &[
            include_str!("prelude/str.bs"),
            include_str!(concat!(env!("OUT_DIR"), "/case.bs")),
            include_str!(concat!(env!("OUT_DIR"), "/marks.bs")),
        ],
    ),
    (words::keywords::ANY, &[include_str!("prelude/any.bs")]),
];

/// The modules a script calls by name, `encoding::base64(..)`: each one's
/// name and source. One may share its name with an engine module, as
/// `random` does, and call that module's functions.
const CALLED: [(&str, &str); 5] = [
    (words::library::AUDIO, include_str!("prelude/audio.bs")),
    (
        words::library::ENCODING,
        include_str!("prelude/encoding.bs"),
    ),
    (
        words::library::GEOMETRY_2D,
        include_str!("prelude/geometry2d.bs"),
    ),
    (words::library::QUAT, include_str!("prelude/quat.bs")),
    (words::library::RANDOM, include_str!("prelude/random.bs")),
];

/// The prelude's functions and constants, named as a program calls them.
pub(crate) struct Library {
    pub fns: Vec<FnDecl>,
    pub consts: Vec<(String, Expr)>,
    /// What each public function of a called module does: its `///` lines.
    pub docs: Vec<(String, String)>,
    /// Each called module's name, and its `//!` lines.
    pub modules: Vec<(String, String)>,
}

thread_local! {
    static LIBRARY: std::cell::OnceCell<Rc<Library>> = const { std::cell::OnceCell::new() };
}

/// Read once a thread; a compile clones what it adds to its program.
///
/// # Panics
/// A prelude that does not parse, which a test catches before a build ships.
pub(crate) fn library() -> Rc<Library> {
    LIBRARY.with(|cell| {
        cell.get_or_init(|| {
            let mut out = Library {
                fns: vec![],
                consts: vec![],
                docs: vec![],
                modules: vec![],
            };
            let called = CALLED.map(|(name, source)| (name, vec![source]));
            for (name, sources) in METHODS
                .map(|(name, sources)| (name, sources.to_vec()))
                .into_iter()
                .chain(called)
            {
                let source = sources.join("\n");
                let mut parsed = crate::parse::parse(&source)
                    .unwrap_or_else(|error| panic!("the prelude's `{name}`: {error}"));
                if !is_method(&format!("{name}::")) {
                    let lines: Vec<&str> = source.lines().collect();
                    for func in parsed.fns.iter().filter(|func| func.public) {
                        let doc = crate::parse::doc_above(&lines, func.pos.line);
                        out.docs.push((format!("{name}::{}", func.name), doc));
                    }
                    let doc: Vec<&str> = lines
                        .iter()
                        .map_while(|line| line.strip_prefix("//!"))
                        .map(str::trim)
                        .collect();
                    out.modules.push((name.to_string(), doc.join(" ")));
                }
                crate::modules::prefix(&mut parsed, name, &format!("prelude/{name}.bs"));
                out.fns.append(&mut parsed.fns);
                out.consts.append(&mut parsed.consts);
            }
            Rc::new(out)
        })
        .clone()
    })
}

/// The program with the prelude's structs first and its functions to call.
pub(crate) fn add(prog: &mut Program) -> Result<(), Error> {
    let core = crate::parse::parse(CORE).map_err(|error| Error {
        pos: error.pos,
        msg: format!("prelude: {}", error.msg),
    })?;
    let mut structs = core.structs;
    structs.append(&mut prog.structs);
    prog.structs = structs;
    let lib = library();
    prog.consts.extend(lib.consts.iter().cloned());
    prog.lazy.clone_from(&lib.fns);
    Ok(())
}

// Whether `key` is `module::..` for one of `modules`.
fn in_module<'a>(key: &str, mut modules: impl Iterator<Item = &'a str>) -> bool {
    modules.any(|name| {
        key.strip_prefix(name)
            .is_some_and(|rest| rest.starts_with("::"))
    })
}

/// Whether `key` names a prelude function, which may call the built-ins
/// that read a string's bytes or a cell's parts.
pub(crate) fn owns(key: &str) -> bool {
    is_method(key) || is_called(key)
}

/// Whether `key` names a built-in type's method, which only another prelude
/// function may call by name: a script reaches it as a method.
pub(crate) fn is_method(key: &str) -> bool {
    in_module(key, METHODS.iter().map(|(name, _)| *name))
}

/// Whether `key` names a function of a module the prelude writes for a
/// script to call by name.
pub(crate) fn is_called(key: &str) -> bool {
    in_module(key, CALLED.iter().map(|(name, _)| *name))
}

/// The public functions of the modules a script calls by name, bodies
/// left out: what an editor completes and checks a call against.
pub(crate) fn called() -> Vec<FnDecl> {
    library()
        .fns
        .iter()
        .filter(|func| func.public && func.owner.is_none() && is_called(&func.name))
        .map(|func| FnDecl {
            body: crate::ast::Block::default(),
            ..func.clone()
        })
        .collect()
}

/// The names of the modules the prelude writes for a script to call.
pub(crate) fn called_modules() -> impl Iterator<Item = &'static str> {
    CALLED.iter().map(|(name, _)| *name)
}

/// A built-in type's methods, as an editor lists them: name, what each takes
/// after the value it is called on, and what it gives.
pub(crate) fn methods(ty: &str) -> Vec<(String, Vec<Ty>, Ty)> {
    let prefix = format!("{ty}::");
    library()
        .fns
        .iter()
        .filter(|func| func.public)
        .filter_map(|func| {
            let name = func.name.strip_prefix(&prefix)?;
            let params = func
                .params
                .iter()
                .skip(1)
                .map(|param| param.ty.clone())
                .collect();
            Some((name.to_string(), params, func.ret.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::ast::Ty;
    use std::fmt::Write;

    // A value of `ty` to call a method with.
    fn any_of(ty: &Ty) -> &'static str {
        match ty {
            Ty::Int => "0",
            Ty::Float => "0.0",
            Ty::Bool => "false",
            Ty::Str => "\"\"",
            Ty::Array(_) => "[]",
            Ty::Struct(name) if name == "Vec2" => "vec2(0.0, 0.0)",
            Ty::Struct(name) if name == "Vec3" => "vec3(0.0, 0.0, 0.0)",
            Ty::Struct(name) if name == "quat::Quat" => "quat::identity()",
            _ => "nil",
        }
    }

    // A prelude function compiles only once a program calls it, so one that
    // nothing in the suite calls would wait for a game to find it broken.
    #[test]
    fn every_method_compiles() {
        let mut src = String::from("pub fn all(text: str, value: any) {\n");
        for (ty, receiver) in [("str", "text"), ("any", "value")] {
            for (name, params, _) in super::methods(ty) {
                let args: Vec<&str> = params.iter().map(any_of).collect();
                let _ = writeln!(src, "    {receiver}.{name}({});", args.join(", "));
            }
        }
        src += "}\n";
        if let Err(error) = crate::compile(&src, &crate::Options::default()) {
            panic!("{error}\n{src}");
        }
    }

    #[test]
    fn every_called_function_compiles_with_the_engine_calls_it_makes() {
        let engine = |key: &str, params: Vec<Ty>, ret: Ty| crate::HostSignature {
            key: key.into(),
            params,
            ret,
            is_async: false,
            editor: false,
        };
        let optional = |ty: Ty| Ty::Optional(Box::new(ty));
        let vec3 = || Ty::Struct("Vec3".into());
        let played = [
            ("bus", optional(Ty::Str)),
            ("volume_linear", optional(Ty::Float)),
            ("pitch_scale", optional(Ty::Float)),
            ("loop", optional(Ty::Bool)),
            ("position", optional(vec3())),
            ("min_distance", optional(Ty::Float)),
            ("max_distance", optional(Ty::Float)),
            ("doppler_level", optional(Ty::Float)),
        ];
        let options = crate::Options {
            host: crate::HostApi {
                functions: vec![
                    engine("random::int", vec![Ty::Int, Ty::Int], Ty::Int),
                    engine("scene::root", vec![], Ty::Node),
                    engine("node.add_child", vec![Ty::Node, Ty::Str], Ty::Node),
                    engine("node.set_process", vec![Ty::Node, Ty::Str], Ty::Void),
                    engine(
                        "node.set_component",
                        vec![Ty::Node, Ty::Str, Ty::Any],
                        Ty::Void,
                    ),
                    engine("node.sound.play", vec![Ty::Node], Ty::Int),
                ],
                components: vec![("transform".into(), vec![("position".into(), vec3())])],
                constants: vec![
                    (
                        "audio::EVENT_SOUND_FINISHED".into(),
                        crate::Literal::Str("sound_finished".into()),
                    ),
                    (
                        "math::PI".into(),
                        crate::Literal::Float(std::f64::consts::PI),
                    ),
                ],
                structs: vec![
                    (
                        "audio::PlayFileOptions".into(),
                        played
                            .into_iter()
                            .map(|(name, ty)| (name.to_string(), ty))
                            .collect(),
                    ),
                    (
                        "quat::Quat".into(),
                        ["x", "y", "z", "w"]
                            .into_iter()
                            .map(|name| (name.to_string(), Ty::Float))
                            .collect(),
                    ),
                ],
            },
            ..crate::Options::default()
        };
        let mut src = String::from("pub fn all() {\n");
        for func in super::called() {
            let args: Vec<&str> = func.params.iter().map(|param| any_of(&param.ty)).collect();
            let _ = writeln!(src, "    {}({});", func.name, args.join(", "));
        }
        src += "}\n";
        let compiled =
            crate::compile(&src, &options).unwrap_or_else(|error| panic!("{error}\n{src}"));
        let imports: Vec<&str> = compiled
            .meta
            .host_calls
            .iter()
            .map(|call| call.key.as_str())
            .collect();
        assert_eq!(
            imports,
            [
                "node.add_child",
                "scene::root",
                "node.set_process",
                "transform.position.get",
                "transform.position.set",
                "node.set_component",
                "node.sound.play",
                "random::int"
            ],
            "{src}"
        );
    }
}

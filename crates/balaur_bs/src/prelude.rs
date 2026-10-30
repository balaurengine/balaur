// What every program has without a `use`: the engine's vectors and colours,
// and the methods of `str` and `any`, written in Balaur Script. A method is
// its module's `pub` function, named as `prefix` names a linked module's
// (`str::find`, a helper `str::#part`), and a program compiles one in only
// once a call reaches it, as it does a release (`codegen::Gen::callee`).
// Only these functions may call the built-ins that read a string's bytes or
// a cell's parts (`codegen::call`'s `intrinsic`).

use crate::Error;
use crate::ast::{Expr, FnDecl, Program, Ty};
use std::rc::Rc;

/// The structs every program declares.
pub(crate) const CORE: &str = include_str!("prelude/core.bs");

/// Each built-in type's methods: its name and its module's sources.
const MODULES: [(&str, &[&str]); 2] = [
    (
        "str",
        &[
            include_str!("prelude/str.bs"),
            include_str!(concat!(env!("OUT_DIR"), "/case.bs")),
        ],
    ),
    ("any", &[include_str!("prelude/any.bs")]),
];

/// The prelude's functions and constants, named as a program calls them.
pub(crate) struct Library {
    pub fns: Vec<FnDecl>,
    pub consts: Vec<(String, Expr)>,
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
            };
            for (name, sources) in MODULES {
                let source = sources.join("\n");
                let mut parsed = crate::parse::parse(&source)
                    .unwrap_or_else(|error| panic!("the prelude's `{name}`: {error}"));
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

/// Whether `key` names a prelude function, which only another may call by
/// name: a script reaches it as a method.
pub(crate) fn owns(key: &str) -> bool {
    MODULES.iter().any(|(name, _)| {
        key.strip_prefix(name)
            .is_some_and(|rest| rest.starts_with("::"))
    })
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
}

//! Who may name what. An item a file does not mark is its own; `internal`
//! opens it to the file's package, and `pub` to every file. A package is the
//! `addons/<name>` or `deps/<alias>` folder a file sits in, or else the project.

use crate::Error;
use crate::ast::{EnumDecl, Pos, Program};
use crate::modules::{ROOT, Rename, block, expr};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub(crate) struct Reach {
    /// Each item, method `T.m` and field `T.f` that is not `pub`, and whether
    /// it is `internal`.
    kept: BTreeMap<String, bool>,
    /// Each module's file, by the name it is used as; the root's under "".
    files: BTreeMap<String, String>,
}

/// The module `name` is in: "" for the root file's.
pub(crate) fn module_of(name: &str) -> &str {
    name.split_once("::").map_or("", |(module, _)| module)
}

// The folder of the package the file at `key` is in; "" for the project's.
// The innermost one: a dependency's addon is its own.
fn package(key: &str) -> &str {
    let folders = key.rsplit_once('/').map_or("", |(folders, _)| folders);
    let mut end = 0;
    let mut at = 0;
    let mut names = folders.split('/').peekable();
    while let Some(name) = names.next() {
        at += name.len() + 1;
        if matches!(name, "addons" | "deps")
            && let Some(package) = names.peek()
        {
            end = at + package.len();
        }
    }
    &folders[..end]
}

impl Reach {
    pub(crate) fn of(prog: &Program) -> Self {
        let mut kept: BTreeMap<String, bool> = prog.kept.iter().cloned().collect();
        let enums = |name: &str| prog.enums.iter().any(|EnumDecl { name: n, .. }| n == name);
        for (owner, fields) in prog.structs.iter().filter(|(owner, _)| !enums(owner)) {
            for f in fields.iter().filter(|f| !f.public) {
                kept.insert(format!("{owner}.{}", f.name), f.internal);
            }
        }
        for f in prog.fns.iter().filter(|f| f.owner.is_some() && !f.public) {
            kept.insert(f.key(), f.internal);
        }
        Reach {
            kept,
            files: prog.files.iter().cloned().collect(),
        }
    }

    fn package_of(&self, module: &str) -> &str {
        self.files.get(module).map_or("", |key| package(key))
    }

    /// Why code in `from` cannot name `item`; `None` when it can.
    pub(crate) fn refused(&self, from: &str, item: &str) -> Option<String> {
        let root = format!("{ROOT}::");
        let item = item.strip_prefix(&root).unwrap_or(item);
        let internal = *self.kept.get(item)?;
        let module = module_of(item);
        if module == from || (internal && self.package_of(module) == self.package_of(from)) {
            return None;
        }
        let name = item.split_once("::").map_or(item, |(_, name)| name);
        let owner = if module.is_empty() {
            self.files.get("").map_or("", String::as_str)
        } else {
            module
        };
        Some(if internal {
            format!(
                "`{name}` is internal to `{}`: mark it `pub` there",
                self.package_of(module)
            )
        } else {
            format!("`{name}` is private to `{owner}`: mark it `pub` there")
        })
    }
}

/// Works out who may name what, and refuses the first name in a linked file
/// that its file may not. Fields and methods wait for their receiver's type.
pub(crate) fn seal(prog: &mut Program) -> Result<(), Error> {
    if prog.linked.is_empty() {
        return Ok(());
    }
    let reach = Reach::of(prog);
    let checked = names(prog, &reach);
    prog.reach = reach;
    checked
}

fn names(prog: &mut Program, reach: &Reach) -> Result<(), Error> {
    for e in &mut prog.enums {
        let names = Names::new(reach, module_of(&e.name));
        for v in &mut e.variants {
            names.at(v.pos);
            for (_, t) in &mut v.fields {
                names.ty(t);
            }
        }
        names.done()?;
    }
    for (name, fields) in &mut prog.structs {
        let names = Names::new(reach, module_of(name));
        for f in fields {
            names.at(f.pos);
            names.ty(&mut f.ty);
            if let Some(d) = &mut f.default {
                expr(&names, &mut vec![], d);
            }
        }
        names.done()?;
    }
    for (name, e) in &mut prog.consts {
        let names = Names::new(reach, module_of(name));
        expr(&names, &mut vec![], e);
        names.done()?;
    }
    for f in &mut prog.fns {
        let names = Names::new(reach, module_of(f.owner.as_deref().unwrap_or(&f.name)));
        names.at(f.pos);
        let mut scope: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
        for p in &mut f.params {
            names.ty(&mut p.ty);
        }
        names.ty(&mut f.ret);
        block(&names, &mut scope, &mut f.body);
        names.done()?;
    }
    Ok(())
}

// Reads every name in one file's item, renaming none.
struct Names<'a> {
    reach: &'a Reach,
    from: &'a str,
    at: Cell<Pos>,
    refused: RefCell<Option<Error>>,
}

impl<'a> Names<'a> {
    fn new(reach: &'a Reach, from: &'a str) -> Self {
        Names {
            reach,
            from,
            at: Cell::new(Pos::default()),
            refused: RefCell::new(None),
        }
    }

    fn name(&self, item: &str) {
        if self.refused.borrow().is_some() {
            return;
        }
        if let Some(msg) = self.reach.refused(self.from, item) {
            let msg = match self
                .reach
                .files
                .get(self.from)
                .filter(|_| !self.from.is_empty())
            {
                Some(file) => format!("in {file}: {msg}"),
                None => msg,
            };
            *self.refused.borrow_mut() = Some(Error {
                pos: self.at.get(),
                msg,
            });
        }
    }

    fn done(self) -> Result<(), Error> {
        self.refused.into_inner().map_or(Ok(()), Err)
    }
}

impl Rename for Names<'_> {
    fn at(&self, pos: Pos) {
        self.at.set(pos);
    }
    fn func(&self, n: &str) -> Option<String> {
        self.name(n);
        None
    }
    fn value(&self, n: &str) -> Option<String> {
        self.func(n)
    }
    fn strukt(&self, n: &str) -> Option<String> {
        self.func(n)
    }
}

//! What a file declares, calls and spells by name, read without building it:
//! for a checker that holds every script of a project against the events
//! something announces.

use crate::ast::{Block, EventDecl, Expr, ExprKind, Part, Pos, Program, StmtKind};
use crate::{Error, HostApi, Literal, Options, parse};

/// How far a constant is followed through the constants it is made of.
const DEPTH: usize = 8;

/// An event a file declares its node sends: `event scored;`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub name: String,
    /// The `///` lines above it, as one line; empty when it has none.
    pub doc: String,
    /// Where its name is written.
    pub pos: Pos,
}

impl From<&EventDecl> for Event {
    fn from(declared: &EventDecl) -> Self {
        Event {
            name: declared.name.clone(),
            doc: declared.doc.clone(),
            pos: declared.pos,
        }
    }
}

/// A call one of whose arguments a literal or a constant spells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamedCall {
    /// `events::listen` for a function, `emit` for `value.emit(..)`.
    pub callee: String,
    /// Called on a value, with a dot.
    pub method: bool,
    /// Each such argument: its place among the arguments, the text it
    /// spells, and where it is written.
    pub args: Vec<(usize, String, Pos)>,
}

/// One file's names.
#[derive(Clone, Debug, Default)]
pub struct Names {
    /// The file's own `event` items, in order.
    pub events: Vec<Event>,
    /// Each method of the class the file is named for, and where it is
    /// declared: what the engine may call on a node running the file.
    pub methods: Vec<(String, Pos)>,
    /// Every function and method the file calls, or hands on as a value, by
    /// the name written.
    pub called: Vec<String>,
    /// Every string the file spells whole: a literal, a constant's value.
    pub strings: Vec<String>,
    /// Each string with `{..}` in it, as the text around its values.
    pub patterns: Vec<Vec<String>>,
    /// Each call that names something with a literal or a constant.
    pub calls: Vec<NamedCall>,
    /// The modules the file uses, by the name each goes by.
    pub modules: Vec<String>,
    /// Every method name the file and its modules declare on a type of
    /// their own.
    pub declared_methods: Vec<String>,
}

/// The events `src` declares, in order.
///
/// # Errors
/// A file that does not parse.
pub fn events(src: &str) -> Result<Vec<Event>, Error> {
    Ok(parse::parse(src)?.events.iter().map(Event::from).collect())
}

/// What `src`, the file at `options.path`, declares, calls and spells. A
/// constant from a module it uses is read through `options.modules`.
///
/// # Errors
/// A file that does not parse, or a `use` that finds no file.
pub fn names(src: &str, options: &Options) -> Result<Names, Error> {
    let mut prog = parse::parse(src)?;
    let class = crate::script_class(&options.path);
    let mut found = Names {
        events: prog.events.iter().map(Event::from).collect(),
        methods: prog
            .fns
            .iter()
            .filter(|f| f.owner.as_deref() == Some(class.as_str()))
            .map(|f| (f.name.clone(), f.pos))
            .collect(),
        ..Names::default()
    };
    if !prog.uses.is_empty() {
        crate::modules::link(
            &mut prog,
            &options.path,
            options.modules.as_ref(),
            false,
            options.parsed.as_ref(),
        )?;
    }
    found.modules.clone_from(&prog.linked);
    found.declared_methods = prog
        .fns
        .iter()
        .filter(|f| f.owner.is_some())
        .map(|f| f.name.clone())
        .collect();
    let mut walk = Walk {
        prog: &prog,
        host: &options.host,
        found: &mut found,
    };
    for (_, value) in &prog.consts {
        walk.expr(value);
    }
    for (_, fields) in &prog.structs {
        for default in fields.iter().filter_map(|field| field.default.as_ref()) {
            walk.expr(default);
        }
    }
    // A module's functions are linked without their bodies: these are the file's.
    for function in &prog.fns {
        walk.block(&function.body);
    }
    found.called.sort_unstable();
    found.called.dedup();
    found.strings.sort_unstable();
    found.strings.dedup();
    Ok(found)
}

struct Walk<'a> {
    prog: &'a Program,
    host: &'a HostApi,
    found: &'a mut Names,
}

impl Walk<'_> {
    // The text `e` spells where the compiler can read it: a literal, or a
    // constant of the file, of a module it uses or of the engine.
    fn spelled(&self, e: &Expr, depth: usize) -> Option<String> {
        match &e.kind {
            ExprKind::Str(text) => Some(text.clone()),
            ExprKind::Var(name) if depth > 0 => {
                if let Some((_, value)) = self.prog.consts.iter().find(|(c, _)| c == name) {
                    return self.spelled(value, depth - 1);
                }
                match self.host.constant(name) {
                    Some(Literal::Str(text)) => Some(text.clone()),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn call(&mut self, callee: &str, method: bool, args: &[Expr]) {
        self.found.called.push(callee.to_string());
        if let Some((_, last)) = callee.rsplit_once("::") {
            self.found.called.push(last.to_string());
        }
        let named: Vec<(usize, String, Pos)> = args
            .iter()
            .enumerate()
            .filter_map(|(at, arg)| Some((at, self.spelled(arg, DEPTH)?, arg.pos)))
            .collect();
        if !named.is_empty() {
            self.found.calls.push(NamedCall {
                callee: callee.to_string(),
                method,
                args: named,
            });
        }
    }

    fn block(&mut self, block: &Block) {
        for stmt in &block.stmts {
            match &stmt.kind {
                StmtKind::Let { init, .. } => self.expr(init),
                StmtKind::Assign { target, value, .. } => {
                    self.expr(target);
                    self.expr(value);
                }
                StmtKind::Expr(e) | StmtKind::Return(Some(e)) => self.expr(e),
                StmtKind::While { cond, body } => {
                    self.expr(cond);
                    self.block(body);
                }
                StmtKind::Loop(body) => self.block(body),
                StmtKind::ForRange { from, to, body, .. } => {
                    self.expr(from);
                    self.expr(to);
                    self.block(body);
                }
                StmtKind::ForEach { items, body, .. } => {
                    self.expr(items);
                    self.block(body);
                }
                StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
            }
        }
        if let Some(tail) = &block.tail {
            self.expr(tail);
        }
    }

    fn each(&mut self, items: &[Expr]) {
        for item in items {
            self.expr(item);
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Str(text) => self.found.strings.push(text.clone()),
            ExprKind::Interp(parts) => {
                let mut text = vec![String::new()];
                for part in parts {
                    match part {
                        Part::Text(piece) => {
                            if let Some(last) = text.last_mut() {
                                last.push_str(piece);
                            }
                        }
                        Part::Value(value) => {
                            self.expr(value);
                            text.push(String::new());
                        }
                    }
                }
                self.found.patterns.push(text);
            }
            // A function handed on as a value is called by whoever takes it.
            ExprKind::Var(name) => self.found.called.push(name.clone()),
            ExprKind::Bin(_, a, b) | ExprKind::Index(a, b) | ExprKind::ArrayRepeat(a, b) => {
                self.expr(a);
                self.expr(b);
            }
            ExprKind::Unary(_, a)
            | ExprKind::Field(a, _)
            | ExprKind::Cast(a, _)
            | ExprKind::Await(a) => self.expr(a),
            ExprKind::Call(callee, args) => {
                self.call(callee, false, args);
                self.each(args);
            }
            ExprKind::Method(on, callee, args) => {
                self.call(callee, true, args);
                self.expr(on);
                self.each(args);
            }
            ExprKind::StructLit(_, fields) => {
                for (_, value) in fields {
                    self.expr(value);
                }
            }
            ExprKind::ArrayLit(items) => self.each(items),
            ExprKind::MapLit(pairs) => {
                for (key, value) in pairs {
                    self.expr(key);
                    self.expr(value);
                }
            }
            ExprKind::If(cond, then, otherwise) => {
                self.expr(cond);
                self.block(then);
                if let Some(otherwise) = otherwise {
                    self.block(otherwise);
                }
            }
            ExprKind::Match(subject, arms) => {
                self.expr(subject);
                for arm in arms {
                    self.expr(&arm.body);
                }
            }
            ExprKind::Block(block) => self.block(block),
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Bool(_)
            | ExprKind::Nil
            | ExprKind::SelfRef
            | ExprKind::Resumed(..) => {}
        }
    }
}

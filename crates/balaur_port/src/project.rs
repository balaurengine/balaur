// The files being ported, what each declares, and the rounds that infer
// types across all of them before any is written.

use crate::api::Api;
use crate::cx::Cx;
use crate::source::{self, Comment, Lines, pascal};
use crate::ty::{Shape, Shapes, Ty};
use crate::{Flag, Input, Ported, Report};
use rune::ast;
use rune::ast::Spanned;
use std::collections::{BTreeSet, HashMap, HashSet};

/// Learning rounds before writing; the editor settles in five or six.
const ROUNDS: usize = 12;

pub(crate) struct Param {
    pub name: String,
    pub ty: Ty,
    /// The function writes through it: a struct passed here is a copy in
    /// `.bs`, so the caller does not see the write.
    pub mutated: bool,
    /// Some call site passes an `any`: untyped otherwise, it is an `any`.
    pub saw_any: bool,
}

// Flags of a function, not a state machine.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Sig {
    pub module: usize,
    /// The Rune name, and what `.bs` calls it.
    pub name: String,
    pub out: String,
    pub params: Vec<Param>,
    pub ret: Ty,
    /// A method of the file's struct: Rune's first parameter was `this`.
    pub method: bool,
    pub public: bool,
    pub is_async: bool,
    pub locals: HashMap<String, Ty>,
    /// Lifted out of a closure; its leading `captured` params were the
    /// closure's captures.
    pub lifted: bool,
    pub captured: usize,
    /// A callback given the editor's state: lifted from a closure that read
    /// only `S`, which is its first parameter, `&mut`, passed by each caller.
    pub state: bool,
    /// The parameters whose values are all it returns (`max`, `clamp`): a
    /// call's result is typed from its own arguments.
    pub poly: Option<Vec<usize>>,
}

pub(crate) struct Const {
    pub ty: Ty,
    /// `.bs` takes only a literal for a `const`; anything else is a function.
    pub literal: bool,
    pub public: bool,
}

pub(crate) struct Module {
    pub path: String,
    pub key: String,
    pub src: String,
    pub lines: Lines,
    pub comments: Vec<Comment>,
    pub ccur: usize,
    pub error: Option<(u32, String)>,
    pub fns: HashMap<String, usize>,
    pub consts: HashMap<String, Const>,
    pub script: Option<usize>,
    /// `use crate::a::b;` as `b` → `a::b`.
    pub uses: HashMap<String, String>,
    /// Modules this one calls, for its `use` lines.
    pub needs: BTreeSet<String>,
    pub flags: Vec<Flag>,
    pub notes: Vec<Flag>,
}

impl Module {
    pub(crate) fn flag(&mut self, line: u32, kind: &'static str, msg: String) {
        let f = Flag { line, kind, msg };
        if !self.flags.contains(&f) {
            self.flags.push(f);
        }
    }
    pub(crate) fn note(&mut self, line: u32, kind: &'static str, msg: String) {
        let f = Flag { line, kind, msg };
        if !self.notes.contains(&f) {
            self.notes.push(f);
        }
    }

    /// The last segment of the key: what another file calls this one by.
    pub(crate) fn alias(&self) -> &str {
        self.key.rsplit("::").next().unwrap_or(&self.key)
    }
}

pub(crate) struct Project {
    pub api: Api,
    pub modules: Vec<Module>,
    pub sigs: Vec<Sig>,
    pub shapes: Shapes,
    /// An object or tuple literal's shape, by module and offset.
    pub lit_shapes: HashMap<(usize, usize), usize>,
    /// A closure's lifted function, by module and offset.
    pub lifted: HashMap<(usize, usize), usize>,
    pub changed: bool,
    pub peeking: u32,
    /// Final struct names, by root shape.
    pub names: HashMap<usize, String>,
    pub keys: HashMap<String, usize>,
    /// Shapes some final type mentions.
    pub reach: HashSet<usize>,
    /// Literal sites the first pass found free-form, and why: the second
    /// pass starts them so, as the types it keeps assume.
    pub free_sites: HashMap<(usize, usize), String>,
}

/// A module's key from its path: after the last `scripts/` or `addons/`,
/// without the extension, `::`-joined.
pub fn module_key(path: &str) -> String {
    let p = path.replace('\\', "/");
    let rest = ["scripts/", "addons/"]
        .iter()
        .filter_map(|m| p.rfind(m).map(|i| &p[i + m.len()..]))
        .min_by_key(|r| r.len())
        .unwrap_or_else(|| p.rsplit('/').next().unwrap_or(&p));
    rest.trim_end_matches(".rn").replace('/', "::")
}

/// A struct name a variable or field offers, and how much to trust it.
pub(crate) fn name_hint(name: &str) -> (u8, String) {
    let name = name.trim_start_matches('_');
    match name {
        "S" => (4, "State".into()),
        "" | "out" | "v" | "x" | "it" | "o" | "r" | "res" | "result" | "value" | "obj" | "data"
        | "t" | "e" | "a" | "b" | "m" | "tmp" | "ret" | "item" | "this" | "params" | "opts"
        | "args" => (0, pascal(name)),
        n if n.len() <= 2 => (0, pascal(n)),
        n => (3, pascal(n)),
    }
}

/// Whether a struct of this name is written as a `class`.
pub(crate) fn class_name(name: &str) -> bool {
    name.ends_with("State")
}

/// A value of type `t` to fill a field a literal left out.
pub(crate) fn default_of(t: &Ty) -> String {
    match t {
        Ty::Int => "0".into(),
        Ty::Float => "0.0".into(),
        Ty::Bool => "false".into(),
        Ty::Str => "\"\"".into(),
        Ty::Array(_) => "[]".into(),
        _ => "nil".into(),
    }
}

const RESERVED: &[&str] = &["Node", "Vec3", "Vec2", "Color", "State"];

impl Project {
    pub(crate) fn new(api: Api) -> Self {
        Self {
            api,
            modules: vec![],
            sigs: vec![],
            shapes: Shapes::default(),
            lit_shapes: HashMap::new(),
            lifted: HashMap::new(),
            changed: false,
            peeking: 0,
            names: HashMap::new(),
            keys: HashMap::new(),
            reach: HashSet::new(),
            free_sites: HashMap::new(),
        }
    }

    /// A type with its shapes resolved: a free-form one is `any`.
    pub(crate) fn norm(&self, t: &Ty) -> Ty {
        match t {
            Ty::Shape(id) => {
                if self.shapes.is_free(*id) {
                    Ty::Any
                } else {
                    Ty::Shape(self.shapes.find(*id))
                }
            }
            Ty::Array(t) => Ty::Array(Box::new(self.norm(t))),
            Ty::Opt(t) => Ty::opt(self.norm(t)),
            t => t.clone(),
        }
    }

    /// A state is shared by everything that holds it, as Rune's objects
    /// were: the editor's `State` and each module's `*State` are classes.
    pub(crate) fn is_class(&self, t: &Ty) -> bool {
        matches!(self.norm(t), Ty::Shape(id) if class_name(&self.shape_name(id)))
    }

    pub(crate) fn shape_name(&self, id: usize) -> String {
        let r = self.shapes.find(id);
        self.names
            .get(&r)
            .cloned()
            .unwrap_or_else(|| format!("Shape{r}"))
    }

    pub(crate) fn ty_str(&self, t: &Ty, from: usize) -> String {
        match self.norm(t) {
            Ty::Unknown | Ty::Nil | Ty::Any | Ty::Void => "any".into(),
            Ty::Int => "int".into(),
            Ty::Float => "float".into(),
            Ty::Bool => "bool".into(),
            Ty::Str => "str".into(),
            Ty::Node => "Node".into(),
            Ty::Named(n) => n.into(),
            Ty::Array(t) => format!("[{}]", self.ty_str(&t, from)),
            Ty::Opt(t) => format!("{}?", self.ty_str(&t, from)),
            Ty::Shape(id) => {
                let name = self.shape_name(id);
                match self.shapes.get(id).home {
                    Some((m, _)) if m != from => format!("{}::{name}", self.modules[m].alias()),
                    _ => name,
                }
            }
            Ty::Fn(sig) => {
                let s = &self.sigs[sig];
                let params: Vec<String> = (s.captured..s.params.len())
                    .map(|i| {
                        let amp = if self.by_ref(s, i) { "&mut " } else { "" };
                        format!("{amp}{}", self.ty_str(&s.params[i].ty, from))
                    })
                    .collect();
                let ret = self.norm(&s.ret);
                if matches!(ret, Ty::Void | Ty::Unknown) {
                    format!("fn({})", params.join(", "))
                } else {
                    format!("fn({}) -> {}", params.join(", "), self.ty_str(&ret, from))
                }
            }
        }
    }

    /// Whether `s`'s parameter `i` is `&mut`: a struct or list it writes, or
    /// hands on to be written, or the state a callback is given. Rune shared
    /// them; an `async fn` cannot take one.
    pub(crate) fn by_ref(&self, s: &Sig, i: usize) -> bool {
        let p = &s.params[i];
        (p.mutated || (s.state && i == 0))
            && !s.is_async
            && matches!(self.norm(&p.ty), Ty::Shape(_) | Ty::Array(_))
    }

    /// A project function by a path's segments, from module `m`.
    pub(crate) fn resolve_fn(&self, m: usize, segs: &[String]) -> Option<(usize, usize)> {
        let (last, mods) = segs.split_last()?;
        let target = self.resolve_module(m, mods)?;
        self.modules[target].fns.get(last).map(|s| (target, *s))
    }

    pub(crate) fn resolve_const(&self, m: usize, segs: &[String]) -> Option<usize> {
        let (last, mods) = segs.split_last()?;
        let target = self.resolve_module(m, mods)?;
        self.modules[target]
            .consts
            .contains_key(last)
            .then_some(target)
    }

    /// The module a path's leading segments name, from module `m`.
    pub(crate) fn resolve_module(&self, m: usize, mods: &[String]) -> Option<usize> {
        if mods.is_empty() {
            return Some(m);
        }
        let mut segs: Vec<String> = mods.to_vec();
        let explicit =
            segs[0] == "crate" || segs[0] == "self" || self.modules[m].uses.contains_key(&segs[0]);
        // Rune looks a bare `settings::get` up from the current module, then
        // in the engine's: a sibling file is reached by `crate::` or `use`.
        // `gamend::core::x`, two segments deep, is an addon's own path.
        if !explicit && segs.len() < 2 {
            return None;
        }
        if segs[0] == "crate" {
            segs.remove(0);
        } else if segs[0] == "self" {
            segs.remove(0);
            if segs.is_empty() {
                return Some(m);
            }
        } else if let Some(full) = self.modules[m].uses.get(&segs[0]) {
            let mut v: Vec<String> = full.split("::").map(str::to_string).collect();
            v.extend(segs.drain(1..));
            segs = v;
        }
        let key = segs.join("::");
        if let Some(i) = self.keys.get(&key) {
            return Some(*i);
        }
        // `crate::util` from `panels::code` is a sibling of the crate root.
        let here = &self.modules[m].key;
        if let Some((dir, _)) = here.rsplit_once("::")
            && let Some(i) = self.keys.get(&format!("{dir}::{key}"))
        {
            return Some(*i);
        }
        None
    }

    /// Settles names for every struct: the strongest hint, unique.
    /// Shapes some final type mentions: the rest were guesses a call site
    /// overruled, and are not written.
    fn reachable(&self) -> HashSet<usize> {
        let mut seen = HashSet::new();
        let mut todo: Vec<Ty> = vec![];
        for s in &self.sigs {
            todo.extend(s.params.iter().map(|p| p.ty.clone()));
            todo.push(s.ret.clone());
            todo.extend(s.locals.values().cloned());
        }
        for m in &self.modules {
            todo.extend(m.consts.values().map(|c| c.ty.clone()));
            if let Some(s) = m.script {
                todo.push(Ty::Shape(s));
            }
        }
        todo.extend(self.lit_shapes.values().map(|s| Ty::Shape(*s)));
        while let Some(t) = todo.pop() {
            match self.norm(&t) {
                Ty::Shape(id) => {
                    if seen.insert(id) {
                        todo.extend(self.shapes.data[id].fields.values().cloned());
                    }
                }
                Ty::Array(t) | Ty::Opt(t) => todo.push(*t),
                _ => {}
            }
        }
        seen
    }

    /// Every editor state is one: `S` is the same object in every file.
    fn unify_states(&mut self) {
        let states: Vec<usize> = (0..self.shapes.data.len())
            .filter(|i| self.shapes.find(*i) == *i)
            .filter(|i| {
                self.shapes.data[*i]
                    .hints
                    .iter()
                    .any(|(s, n)| *s >= 4 && n == "State")
            })
            .collect();
        if let Some((first, rest)) = states.split_first() {
            for r in rest {
                self.shapes.union(*first, *r);
            }
        }
    }

    fn name_shapes(&mut self) {
        self.reach = self.reachable();
        let mut taken: HashMap<String, usize> = HashMap::new();
        let mut roots: Vec<usize> = (0..self.shapes.data.len())
            .filter(|i| self.shapes.find(*i) == *i)
            .collect();
        // Script structs first, then the most-used, so they keep the plain name.
        roots.sort_by_key(|r| {
            let s = &self.shapes.data[*r];
            (!s.script, std::cmp::Reverse(s.literals), *r)
        });
        for r in roots {
            if self.shapes.is_free(r) || !self.reach.contains(&r) {
                continue;
            }
            let s = &self.shapes.data[r];
            let mut hints = s.hints.clone();
            hints.sort_by_key(|h| std::cmp::Reverse(h.0));
            let base = hints.iter().find(|h| !h.1.is_empty()).map_or_else(
                || {
                    if s.tuple {
                        "Pair".to_string()
                    } else {
                        "Obj".to_string()
                    }
                },
                |h| h.1.clone(),
            );
            let module = s.home.map(|h| h.0);
            let mut name = base.clone();
            if RESERVED.contains(&name.as_str()) && (name != "State" || s.tuple) {
                name = format!("{name}Data");
            }
            let mut n = 1;
            while taken.contains_key(&name) && (!s.script || taken.get(&name) != module.as_ref()) {
                // Another file's struct of the same name: qualified anyway,
                // but a reader is better served by two names.
                n += 1;
                name = format!("{base}{n}");
            }
            taken.insert(name.clone(), module.unwrap_or(usize::MAX));
            self.names.insert(r, name);
        }
    }

    /// A struct is declared in the file that builds it most.
    fn settle_homes(&mut self) {
        for i in 0..self.shapes.data.len() {
            if self.shapes.find(i) != i || self.shapes.data[i].script {
                continue;
            }
            let best = self.shapes.data[i]
                .sites
                .iter()
                .max_by_key(|(m, (n, _, keys))| (*keys, *n, std::cmp::Reverse(**m)))
                .map(|(m, (_, at, _))| (*m, *at));
            if let Some(h) = best {
                self.shapes.data[i].home = Some(h);
            }
        }
    }

    fn cleanup_shapes(&mut self) {
        // A struct nothing put a field in is free-form data (`#{}` filled by key).
        for i in 0..self.shapes.data.len() {
            if self.shapes.find(i) != i {
                continue;
            }
            let s = &self.shapes.data[i];
            if s.fields.is_empty() && !s.script && s.free.is_none() {
                self.shapes.data[i].free = Some("no field is ever named".into());
            }
        }
        // A wide struct whose literals each name a few of its fields is a
        // bag of properties (a widget's): a map reads better than a literal
        // spelling forty defaults.
        for i in 0..self.shapes.data.len() {
            if self.shapes.find(i) != i {
                continue;
            }
            let s = &self.shapes.data[i];
            let n = s.fields.len();
            let sites = s.site_keys.len();
            if s.script || s.free.is_some() || n < 12 || sites < 2 {
                continue;
            }
            let avg = s.site_keys.values().sum::<usize>() as f64 / sites as f64;
            // One literal naming most of it (`fresh_state`) declares a struct.
            let fullest = s.site_keys.values().max().copied().unwrap_or(0);
            if avg < 0.4 * n as f64 && (fullest as f64) < 0.6 * n as f64 {
                let why = format!(
                    "a property bag: {n} fields, and a literal names {avg:.0} of them on average"
                );
                self.shapes.data[i].free = Some(why);
            }
        }
        // A struct the engine's `any` data fills more often than literals
        // build it is that data: a document node, a reply.
        for i in 0..self.shapes.data.len() {
            if self.shapes.find(i) != i {
                continue;
            }
            let s = &self.shapes.data[i];
            let state = s.hints.iter().any(|(st, n)| *st >= 4 && n == "State");
            let fed = s.any_fed.len();
            if !s.script && !state && s.free.is_none() && fed >= 3 && fed > s.literals {
                let why = format!(
                    "the engine's `any` fills it in {fed} places, literals in {}",
                    s.literals
                );
                self.shapes.data[i].free = Some(why);
                continue;
            }
            let out = s.to_any.len();
            if !s.script && !state && s.free.is_none() && out > 0 && out >= s.literals {
                let why = format!("it is handed on as an `any` in {out} places");
                self.shapes.data[i].free = Some(why);
            }
        }
        // What a free-form value holds is free-form too: `any` boxes no struct.
        loop {
            let mut more = vec![];
            for i in 0..self.shapes.data.len() {
                if self.shapes.find(i) != i || !self.shapes.is_free(i) {
                    continue;
                }
                let mut todo: Vec<Ty> = self.shapes.data[i].fields.values().cloned().collect();
                while let Some(t) = todo.pop() {
                    match t {
                        Ty::Shape(s) => {
                            let s = self.shapes.find(s);
                            if !self.shapes.is_free(s) && !self.shapes.data[s].script {
                                more.push(s);
                            }
                        }
                        Ty::Array(t) | Ty::Opt(t) => todo.push(*t),
                        _ => {}
                    }
                }
            }
            if more.is_empty() {
                break;
            }
            for s in more {
                self.shapes.data[s].free = Some("it sits inside free-form data".into());
            }
        }
        // A struct holding itself cannot be a value.
        for i in 0..self.shapes.data.len() {
            if self.shapes.find(i) != i || self.shapes.is_free(i) {
                continue;
            }
            let recursive = self.shapes.data[i].fields.values().any(|t| match t {
                Ty::Shape(_) | Ty::Opt(_) => self.shapes.contains(t, i, 0),
                _ => false,
            });
            if recursive && !self.shapes.data[i].script {
                self.shapes.data[i].free =
                    Some("it holds itself, which a value struct cannot".into());
            }
        }
    }
}

/// Reads every file, infers across all of them, writes each.
pub(crate) fn port(inputs: &[Input], api: Api) -> Vec<Ported> {
    let mut p = Project::new(api);
    let mut asts: Vec<Option<ast::File>> = vec![];
    for input in inputs {
        let lines = Lines::new(&input.source);
        let comments = source::comments(&input.source, &lines);
        let parsed = std::panic::catch_unwind(|| {
            rune::parse::parse_all::<ast::File>(&input.source, rune::SourceId::empty(), true)
        });
        let (file, error) = match parsed {
            Ok(Ok(f)) => (Some(f), None),
            Ok(Err(e)) => {
                let at = e.span().range().start;
                (None, Some((lines.line(at), format!("{e}"))))
            }
            Err(_) => (None, Some((1, "the Rune parser panicked".to_string()))),
        };
        let key = input.key.clone().unwrap_or_else(|| module_key(&input.path));
        p.keys.insert(key.clone(), p.modules.len());
        p.modules.push(Module {
            path: input.path.clone(),
            key,
            src: input.source.clone(),
            lines,
            comments,
            ccur: 0,
            error,
            fns: HashMap::new(),
            consts: HashMap::new(),
            script: None,
            uses: HashMap::new(),
            needs: BTreeSet::new(),
            flags: vec![],
            notes: vec![],
        });
        asts.push(file);
    }
    for (m, file) in asts.iter().enumerate() {
        if let Some(file) = file {
            index(&mut p, m, file);
        }
    }
    for pass in 0..2 {
        if pass == 1 {
            p.restart();
        }
        for _ in 0..ROUNDS {
            p.changed = false;
            p.shapes.changed = false;
            for (m, file) in asts.iter().enumerate() {
                if let Some(file) = file {
                    crate::item::module(&mut p, m, file, true);
                }
            }
            p.unify_states();
            if !p.changed && !p.shapes.changed {
                break;
            }
        }
    }
    p.settle_homes();
    p.cleanup_shapes();
    p.name_shapes();
    let mut out = vec![];
    for (m, file) in asts.iter().enumerate() {
        let bs = if let Some(file) = file {
            crate::item::module(&mut p, m, file, false)
        } else {
            let (line, msg) = p.modules[m].error.clone().unwrap_or_default();
            p.modules[m].flag(line, "parse", format!("Rune does not parse: {msg}"));
            let mut s = format!(
                "// {}: {} did not parse as Rune ({msg}), so nothing was translated.\n",
                crate::MARK,
                p.modules[m].path
            );
            for l in p.modules[m].src.lines() {
                s.push_str("// ");
                s.push_str(l);
                s.push('\n');
            }
            s
        };
        let module = &p.modules[m];
        let code = source::code_lines(&module.src);
        let all_bad = module.error.is_some();
        let flagged: BTreeSet<u32> = if all_bad {
            code.iter().copied().collect()
        } else {
            module.flags.iter().map(|f| f.line).collect()
        };
        let mut flags = module.flags.clone();
        flags.sort_by_key(|f| f.line);
        let mut notes = module.notes.clone();
        notes.sort_by_key(|f| f.line);
        out.push(Ported {
            path: module.path.clone(),
            key: module.key.clone(),
            bs,
            report: Report {
                code_lines: code.len(),
                flagged_lines: flagged.into_iter().filter(|l| code.contains(l)).count(),
                flags,
                notes,
            },
        });
    }
    out
}

/// What each file declares, before any body is read.
fn index(p: &mut Project, m: usize, file: &ast::File) {
    let src = p.modules[m].src.clone();
    let text = |s: ast::Span| src.get(s.range()).unwrap_or("").to_string();
    let mut script = false;
    for (item, _) in &file.items {
        match item {
            ast::Item::Fn(f) => {
                let name = text(f.name.span);
                let mut params = vec![];
                let mut method = false;
                for (i, (arg, _)) in f.args.iter().enumerate() {
                    let pname = match arg {
                        ast::FnArg::SelfValue(_) => "self".to_string(),
                        ast::FnArg::Pat(pat) => match pat {
                            ast::Pat::Path(pp) => text(pp.path.span()),
                            ast::Pat::Ignore(_) => format!("_{i}"),
                            other => {
                                let t = text(other.span());
                                format!("_arg{i}_{}", t.len())
                            }
                        },
                        _ => format!("_arg{i}"),
                    };
                    if i == 0 && pname == "this" {
                        method = true;
                        continue;
                    }
                    params.push(Param {
                        name: pname,
                        ty: Ty::Unknown,
                        mutated: false,
                        saw_any: false,
                    });
                }
                script |= method;
                let sig = p.sigs.len();
                p.sigs.push(Sig {
                    module: m,
                    out: source::safe_ident(&name),
                    name: name.clone(),
                    params,
                    ret: Ty::Unknown,
                    method,
                    public: matches!(f.visibility, ast::Visibility::Public(_)),
                    is_async: f.async_token.is_some(),
                    locals: HashMap::new(),
                    lifted: false,
                    captured: 0,
                    state: false,
                    poly: None,
                });
                let names: Vec<String> =
                    p.sigs[sig].params.iter().map(|p| p.name.clone()).collect();
                p.sigs[sig].poly = poly_leaves(&f.body, &names, &src);
                // A hook's arguments are what the engine passes; another
                // `on_*` handler is an event's, an `any`.
                if p.sigs[sig].method {
                    let given = crate::api::hook_params(&name).or_else(|| {
                        name.starts_with("on_")
                            .then(|| vec![Ty::Any; p.sigs[sig].params.len()])
                    });
                    if let Some(tys) = given {
                        for (param, t) in p.sigs[sig].params.iter_mut().zip(tys) {
                            if t == Ty::Any {
                                param.saw_any = true;
                            } else {
                                param.ty = t;
                            }
                        }
                    }
                }
                p.modules[m].fns.insert(name, sig);
            }
            ast::Item::Const(c) => {
                let name = text(c.name.span);
                let literal = is_literal(&c.expr);
                p.modules[m].consts.insert(
                    name,
                    Const {
                        ty: Ty::Unknown,
                        literal,
                        public: matches!(c.visibility, ast::Visibility::Public(_)),
                    },
                );
            }
            ast::Item::Mod(md) => {
                let name = text(md.name.span);
                p.modules[m].uses.insert(name.clone(), name);
            }
            ast::Item::Use(u) => {
                let mut segs = vec![];
                let mut ok = true;
                for seg in
                    std::iter::once(&u.path.first).chain(u.path.segments.iter().map(|s| &s.1))
                {
                    match seg {
                        ast::ItemUseSegment::PathSegment(ps) => segs.push(text(ps.span())),
                        _ => ok = false,
                    }
                }
                if ok && !segs.is_empty() {
                    let alias = u.path.alias.as_ref().map_or_else(
                        || segs.last().cloned().unwrap_or_default(),
                        |a| text(a.1.span),
                    );
                    let full: Vec<String> = segs.into_iter().filter(|s| s != "crate").collect();
                    p.modules[m].uses.insert(alias, full.join("::"));
                }
            }
            _ => {}
        }
    }
    if script {
        script_shape(p, m);
    }
}

/// A type with no struct or function in it.
fn plain(t: &Ty) -> bool {
    match t {
        Ty::Shape(_) | Ty::Fn(_) => false,
        Ty::Array(t) | Ty::Opt(t) => plain(t),
        _ => true,
    }
}

/// The file's own struct, holding what `this` held.
fn script_shape(p: &mut Project, m: usize) {
    let stem = p.modules[m]
        .key
        .rsplit("::")
        .next()
        .unwrap_or("Script")
        .to_string();
    let id = p.shapes.add(Shape {
        script: true,
        home: Some((m, 0)),
        hints: vec![(9, pascal(&stem))],
        ..Default::default()
    });
    p.shapes.set_field(id, "node", Ty::Node);
    p.modules[m].script = Some(id);
}

impl Project {
    /// Forgets every type and struct, keeping what is known for sure: which
    /// parameters are only ever handed an `any`, and which are written
    /// through. A second pass from there merges nothing an early guess did.
    fn restart(&mut self) {
        self.settle_homes();
        self.cleanup_shapes();
        self.free_sites = self
            .lit_shapes
            .iter()
            .filter(|(_, id)| self.shapes.is_free(**id))
            .map(|(k, id)| (*k, self.shapes.free_reason(*id)))
            .collect();
        self.shapes = Shapes::default();
        self.lit_shapes.clear();
        // Types with no struct in them are kept: they seed the second pass,
        // so `util::obj` is an `any` before anything calls it.
        let keep = |t: &mut Ty| {
            if !plain(t) {
                *t = Ty::Unknown;
            }
        };
        for s in &mut self.sigs {
            for p in &mut s.params {
                keep(&mut p.ty);
            }
            keep(&mut s.ret);
            for t in s.locals.values_mut() {
                keep(t);
            }
        }
        for m in 0..self.modules.len() {
            for c in self.modules[m].consts.values_mut() {
                keep(&mut c.ty);
            }
            if self.modules[m].script.is_some() {
                script_shape(self, m);
            }
        }
    }
}

/// The parameters a body returns, when it returns nothing else: a function
/// whose only tail is `if a > b { a } else { b }` and the like.
fn poly_leaves(body: &ast::Block, params: &[String], src: &str) -> Option<Vec<usize>> {
    fn tail(b: &ast::Block) -> Option<&ast::Expr> {
        match b.statements.as_slice() {
            [ast::Stmt::Expr(e)] => Some(e),
            _ => None,
        }
    }
    fn leaves(e: &ast::Expr, params: &[String], src: &str, out: &mut Vec<usize>) -> bool {
        match e {
            ast::Expr::Path(p) if p.rest.is_empty() => {
                let name = src.get(p.span().range()).unwrap_or("").trim();
                match params.iter().position(|q| q == name) {
                    Some(i) => {
                        if !out.contains(&i) {
                            out.push(i);
                        }
                        true
                    }
                    None => false,
                }
            }
            ast::Expr::Group(g) => leaves(&g.expr, params, src, out),
            ast::Expr::Unary(u) if matches!(u.op, ast::UnOp::Neg(_)) => {
                leaves(&u.expr, params, src, out)
            }
            ast::Expr::If(i) => {
                let Some(els) = &i.expr_else else {
                    return false;
                };
                let mut blocks = vec![&*i.block];
                blocks.extend(i.expr_else_ifs.iter().map(|e| &*e.block));
                blocks.push(&els.block);
                blocks
                    .iter()
                    .all(|b| tail(b).is_some_and(|t| leaves(t, params, src, out)))
            }
            _ => false,
        }
    }
    let mut out = vec![];
    let t = tail(body)?;
    (leaves(t, params, src, &mut out) && !out.is_empty()).then_some(out)
}

/// A comment as `.bs` lines: it has no `/* */`.
fn line_comments(text: &str) -> Vec<String> {
    if let Some(body) = text.strip_prefix("/*") {
        let body = body.strip_suffix("*/").unwrap_or(body);
        return body
            .lines()
            .map(|l| {
                let l = l.trim().trim_start_matches('*').trim_start();
                if l.is_empty() {
                    "//".to_string()
                } else {
                    format!("// {l}")
                }
            })
            .collect();
    }
    text.lines().map(|l| l.trim_start().to_string()).collect()
}

fn is_literal(e: &ast::Expr) -> bool {
    match e {
        ast::Expr::Lit(l) => matches!(
            l.lit,
            ast::Lit::Bool(_) | ast::Lit::Number(_) | ast::Lit::Str(_) | ast::Lit::Char(_)
        ),
        ast::Expr::Unary(u) => matches!(u.op, ast::UnOp::Neg(_)) && is_literal(&u.expr),
        _ => false,
    }
}

impl Cx<'_> {
    /// The file's opening comment (a run from its first lines with no gap)
    /// and the comments after it, before `at`.
    pub(crate) fn header_comments(&mut self, at: usize) -> (Vec<String>, Vec<String>) {
        if self.learn || self.p.peeking > 0 {
            return (vec![], vec![]);
        }
        let module = &mut self.p.modules[self.m];
        let mut head = vec![];
        let mut next_line = 1;
        while module.ccur < module.comments.len() && module.comments[module.ccur].at < at {
            let c = &module.comments[module.ccur];
            if c.line > next_line || c.trailing {
                break;
            }
            head.extend(line_comments(&c.text));
            next_line = c.line + c.text.lines().count() as u32;
            module.ccur += 1;
        }
        (head, self.comments_before(at))
    }

    /// Comments before `at` not yet written, as lines.
    pub(crate) fn comments_before(&mut self, at: usize) -> Vec<String> {
        if self.learn || self.p.peeking > 0 {
            return vec![];
        }
        let module = &mut self.p.modules[self.m];
        let mut out = vec![];
        while module.ccur < module.comments.len() && module.comments[module.ccur].at < at {
            let c = &module.comments[module.ccur];
            out.extend(line_comments(&c.text));
            module.ccur += 1;
        }
        out
    }
}

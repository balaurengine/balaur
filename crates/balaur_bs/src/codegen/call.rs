use super::Kind;
use super::func::{FnGen, R, RefPlace, err, show};
use super::rt::{load, mem, store};
use crate::Error;
use crate::ast::{Block, Expr, ExprKind, Pos, Stmt, StmtKind, Ty};
use wasm_encoder::{BlockType, Instruction as I, ValType};

impl FnGen<'_> {
    /// A function's name as a value: its slot in the module's table.
    pub(crate) fn fn_value(&mut self, name: &str, pos: Pos) -> R<Ty> {
        let sig = self.g.sigs[name].clone();
        if sig.kind != Kind::Script || sig.owner.is_some() {
            return err(
                pos,
                format!(
                    "only a script's own plain function can be a `fn` value; `{name}` is not one"
                ),
            );
        }
        let slot = self.g.fn_ref(sig.idx);
        self.e(I::I32Const(slot as i32));
        Ok(Ty::Fn(sig.params, Box::new(sig.ret), sig.refs))
    }

    // `f(..)` where `f` holds a `fn`: a call through the table.
    fn call_value(&mut self, f: &super::func::LVal, args: &[Expr], pos: Pos, name: &str) -> R<Ty> {
        let Ty::Fn(params, ret, refs) = &f.ty else {
            return err(
                pos,
                format!("`{name}` is a {}, not a function", show(&f.ty)),
            );
        };
        // Called as a script function is, `&mut` arguments stored back.
        let sig = super::Sig {
            idx: 0,
            params: params.clone(),
            refs: refs.clone(),
            ret: (**ret).clone(),
            owner: None,
            kind: Kind::Script,
        };
        let places = self.script_args(&sig, args, pos, name, None)?;
        self.get_raw(f);
        self.spill();
        let (p, r) = self.g.wasm_sig(&sig);
        self.e(I::CallIndirect {
            type_index: self.g.type_idx(p, r),
            table_index: 0,
        });
        if let Some(r) = self.store_back(places, &sig.ret) {
            self.get_raw(&r);
        }
        Ok(sig.ret)
    }

    pub(crate) fn call(&mut self, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        if let Some(f) = self.lookup(name) {
            return self.call_value(&f, args, pos, name);
        }
        let native = match name {
            "sqrt" => Some((I::F64Sqrt, 1)),
            "abs" => Some((I::F64Abs, 1)),
            "floor" => Some((I::F64Floor, 1)),
            "ceil" => Some((I::F64Ceil, 1)),
            "min" => Some((I::F64Min, 2)),
            "max" => Some((I::F64Max, 2)),
            _ => self
                .g
                .imp
                .math
                .get(name)
                .map(|(idx, n)| (I::Call(*idx), *n)),
        };
        if let Some((i, n)) = native {
            self.args(&vec![Ty::Float; n], args, pos, name)?;
            self.e(i);
            return Ok(Ty::Float);
        }
        if name == "nil" {
            self.args(&[], args, pos, name)?;
            self.e(I::I32Const(0));
            return Ok(Ty::Any);
        }
        // An empty map: a cell whose key and value arrays are not made yet.
        if name == "map" {
            self.args(&[], args, pos, name)?;
            self.e(I::I64Const(0));
            self.cell(crate::anys::MAP);
            return Ok(Ty::Any);
        }
        if let Some(fields) = match name {
            "vec2" => Some(2),
            "vec3" => Some(3),
            "color" => Some(4),
            _ => None,
        } {
            self.args(&vec![Ty::Float; fields], args, pos, name)?;
            return Ok(Ty::Struct(
                ["Vec2", "Vec3", "Color"][fields - 2].to_string(),
            ));
        }
        let sig = self.g.sigs.get(name).cloned().ok_or_else(|| Error {
            pos,
            msg: match name.split_once("::") {
                Some((m, f)) if self.g.sigs.contains_key(&format!("{m}::#{f}")) => {
                    format!("`{f}` is private to `{m}`: mark it `pub` there")
                }
                Some(_) => format!("the engine has no `{name}`"),
                None => format!("unknown function `{name}`"),
            },
        })?;
        match sig.kind {
            Kind::ExternAsync | Kind::HostAsync | Kind::Async => {
                return err(
                    pos,
                    format!("`{name}` is async: `await {name}(..)` inside an `async fn`"),
                );
            }
            Kind::Extern | Kind::Host => {
                let borrowed = self.host_args(&sig.params, args, pos, name)?;
                self.e(I::Call(sig.idx));
                for b in borrowed {
                    self.release(&b);
                }
            }
            Kind::Script if sig.owner.is_some() => {
                return err(
                    pos,
                    format!("`{name}` is a method: call it as `self.{name}(..)`"),
                );
            }
            Kind::Script => {
                let places = self.script_args(&sig, args, pos, name, None)?;
                self.spill();
                self.e(I::Call(sig.idx));
                if let Some(r) = self.store_back(places, &sig.ret) {
                    self.get_raw(&r);
                }
            }
        }
        Ok(sig.ret)
    }

    pub(crate) fn method(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        // `node.transform.translate(..)`: an operation a component drives is
        // called on that component, as Rune calls it.
        if let Some(field) = self.component_get(base, name) {
            return self.method(&field, name, args, pos);
        }
        if let ExprKind::Field(node, comp) = &base.kind
            && let Some(sig) = self.g.sigs.get(&format!("node.{comp}.{name}")).cloned()
            && sig.kind == Kind::Host
        {
            // An `any` holding the node is read as one.
            if self.type_of(node)? == Ty::Any {
                self.expr(node)?;
                self.unbox_any(&Ty::Node);
                let lv = self.alloc(&Ty::Node);
                self.set_raw(&lv);
                let hidden = format!("#node{}", lv.slots[0]);
                self.declare(&hidden, lv);
                let var = Expr {
                    kind: ExprKind::Var(hidden),
                    pos,
                };
                return self.node_call(&var, &sig, args, pos, name);
            }
            if self.type_of(node)? != Ty::Node {
                return err(
                    pos,
                    format!("`{comp}.{name}` is called on a node's `{comp}`"),
                );
            }
            return self.node_call(node, &sig, args, pos, name);
        }
        let place = self.place(base)?;
        let bty = match &place {
            Some(lv) => lv.ty.clone(),
            None => self.type_of(base)?,
        };
        if let Some(t) = self.method_sugar(base, &bty, name, args, pos)? {
            return Ok(t);
        }
        match &bty {
            Ty::Array(_) => self.array_method(base, name, args, pos),
            Ty::Str if name == "len" => {
                self.args(&[], args, pos, "len")?;
                self.expr(base)?;
                let t = self.temp(&Ty::Str);
                self.set_raw(&t);
                self.emit([
                    I::LocalGet(t.slots[0]),
                    I::I32Load(mem(4, 2)),
                    I::I64ExtendI32U,
                ]);
                self.release(&t);
                Ok(Ty::Int)
            }
            Ty::Str => self.str_method(base, name, args, pos),
            Ty::Any => self.any_method(base, name, args, pos),
            Ty::Optional(inner) => self.optional_method(base, inner, name, args, pos),
            Ty::Node => self.node_method(base, name, args, pos),
            Ty::Struct(s) => self.script_method(base, place, s, name, args, pos),
            _ => err(pos, format!("{} has no methods", show(&bty))),
        }
    }

    // `v.m(..)` on a script struct or class: `self` lent from where `v` is
    // and handed back there.
    fn script_method(
        &mut self,
        base: &Expr,
        place: Option<super::func::LVal>,
        s: &str,
        name: &str,
        args: &[Expr],
        pos: Pos,
    ) -> R<Ty> {
        let bty = Ty::Struct(s.to_string());
        let key = format!("{s}.{name}");
        let sig = self.g.sigs.get(&key).cloned().ok_or(Error {
            pos,
            msg: format!("no method `{name}` on {s}"),
        })?;
        if sig.kind == Kind::Async {
            return err(
                pos,
                format!("`{key}` is async: `await self.{name}(..)` inside an `async fn`"),
            );
        }
        // A class is shared, so any expression holding one will do;
        // a value inside an object is lent and handed back there.
        let mut held = None;
        let mut lent = None;
        let lv = match place {
            Some(lv) => lv,
            None if self.g.is_class(&bty).is_some() => {
                self.expr(base)?;
                let t = self.temp(&bty);
                self.set_raw(&t);
                held = Some(t.clone());
                t
            }
            None => match self.mem_place(base)? {
                Some(mp) if mp.stable => {
                    let t = self.alloc(&bty);
                    self.mem_get_raw(&mp);
                    self.set_raw(&t);
                    lent = Some(mp);
                    t
                }
                _ => {
                    return err(
                        pos,
                        "a method changes its value: call it on a variable or a field",
                    );
                }
            },
        };
        self.get_raw(&lv);
        let places = self.script_args(&sig, args, pos, &key, Some(&lv))?;
        self.spill();
        self.e(I::Call(sig.idx));
        let ret = if let Some(r) = self.store_back(places, &sig.ret) {
            r
        } else {
            let r = self.alloc(&sig.ret);
            self.set_raw(&r);
            r
        };
        self.set_raw(&lv);
        if let Some(mp) = lent {
            self.get_raw(&lv);
            self.mem_set_raw(&mp);
            if let Some(r) = &mp.release {
                self.release(r);
            }
        }
        if let Some(t) = held {
            self.release(&t);
        }
        self.get_raw(&ret);
        Ok(sig.ret)
    }

    /// A script call's arguments. A `&mut` one is a variable or a field,
    /// passed as it is, and no other place the call changes may overlap it:
    /// each comes back on its own and the last stored would win.
    pub(crate) fn script_args(
        &mut self,
        sig: &super::Sig,
        args: &[Expr],
        pos: Pos,
        name: &str,
        receiver: Option<&super::func::LVal>,
    ) -> R<Vec<RefPlace>> {
        if !sig.has_refs() {
            self.args(&sig.params, args, pos, name)?;
            return Ok(vec![]);
        }
        if sig.params.len() != args.len() {
            return err(
                pos,
                format!(
                    "`{name}` takes {} argument(s), {} given",
                    sig.params.len(),
                    args.len()
                ),
            );
        }
        let mut places: Vec<RefPlace> = vec![];
        for ((want, by_ref), a) in sig.params.iter().zip(&sig.refs).zip(args) {
            if !by_ref {
                self.expr_as(a, want)?;
                continue;
            }
            let Some(lv) = self.place(a)? else {
                // A field of a class: lent from its object, and stored back.
                if let Some(mp) = self.mem_place(a)?
                    && mp.stable
                {
                    if mp.ty != *want {
                        return err(
                            a.pos,
                            format!("expected {}, got {}", show(want), show(&mp.ty)),
                        );
                    }
                    self.mem_get_raw(&mp);
                    places.push(RefPlace::Mem(mp));
                    continue;
                }
                return err(
                    a.pos,
                    format!("`{name}` changes this argument: pass a variable or a field"),
                );
            };
            if lv.ty != *want {
                return err(
                    a.pos,
                    format!("expected {}, got {}", show(want), show(&lv.ty)),
                );
            }
            let overlaps = |o: &super::func::LVal| o.slots.iter().any(|s| lv.slots.contains(s));
            let locals = places.iter().filter_map(|p| match p {
                RefPlace::Local(l) => Some(l),
                RefPlace::Mem(_) => None,
            });
            if locals.chain(receiver).any(overlaps) {
                return err(
                    a.pos,
                    format!(
                        "`{name}` would change this twice: it is part of another `&mut` argument or of what the method is called on"
                    ),
                );
            }
            self.get_raw(&lv);
            places.push(RefPlace::Local(lv));
        }
        Ok(places)
    }

    /// After a call with `&mut` arguments: the result set aside, each
    /// argument stored back where it came from. The result, if set aside.
    pub(crate) fn store_back(
        &mut self,
        places: Vec<RefPlace>,
        ret: &Ty,
    ) -> Option<super::func::LVal> {
        if places.is_empty() {
            return None;
        }
        let r = self.alloc(ret);
        self.set_raw(&r);
        for p in places.into_iter().rev() {
            match p {
                RefPlace::Local(lv) => self.set_raw(&lv),
                RefPlace::Mem(mp) => {
                    self.mem_set_raw(&mp);
                    if let Some(t) = &mp.release {
                        self.release(t);
                    }
                }
            }
        }
        Some(r)
    }

    // `n.get("transform")` before a method the component drives is
    // `n.transform`: the key-reading spelling of the same thing.
    fn component_get(&self, base: &Expr, name: &str) -> Option<Expr> {
        let ExprKind::Method(n, get, key) = &base.kind else {
            return None;
        };
        let [arg] = key.as_slice() else { return None };
        let ExprKind::Str(comp) = &arg.kind else {
            return None;
        };
        let drives = get == "get" && self.g.sigs.contains_key(&format!("node.{comp}.{name}"));
        drives.then(|| Expr {
            kind: ExprKind::Field(n.clone(), comp.clone()),
            pos: base.pos,
        })
    }

    // What a value answers before its own kind's methods: a vector's field
    // by `at`/`get`, `as_` conversions, and the `T?` methods on a value that
    // is always there.
    fn method_sugar(
        &mut self,
        base: &Expr,
        bty: &Ty,
        name: &str,
        args: &[Expr],
        pos: Pos,
    ) -> R<Option<Ty>> {
        let bty = bty.clone();
        // `v.at(0)` or `v.get("y")` on a vector is its field.
        if let Ty::Struct(s) = &bty
            && matches!(s.as_str(), "Vec2" | "Vec3" | "Color")
            && let [arg] = args
        {
            let fields: &[&str] = match s.as_str() {
                "Vec2" => &["x", "y"],
                "Vec3" => &["x", "y", "z"],
                _ => &["r", "g", "b", "a"],
            };
            let field = match (name, &arg.kind) {
                ("at", ExprKind::Int(i)) => usize::try_from(*i).ok().and_then(|i| fields.get(i)),
                ("get", ExprKind::Str(k)) => fields.iter().find(|f| **f == k.as_str()),
                _ => None,
            };
            if let Some(f) = field {
                let read = Expr {
                    kind: ExprKind::Field(Box::new(base.clone()), (*f).to_string()),
                    pos,
                };
                return self.expr(&read).map(Some);
            }
        }
        let plain = matches!(bty, Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Node);
        if plain && args.is_empty() && name.starts_with("as_") {
            return self.plain_as(base, name, pos).map(Some);
        }
        Ok(None)
    }

    // `node.f(a)` is the host's `f(node, a)`.
    fn node_method(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        let takes_node = |s: &super::Sig| s.params.first() == Some(&Ty::Node);
        let sig = self
            .g
            .sigs
            .get(name)
            .cloned()
            .filter(|s| s.kind == Kind::Extern && takes_node(s));
        let sig = sig.or_else(|| {
            self.g
                .sigs
                .get(&format!("node.{name}"))
                .cloned()
                .filter(|s| s.kind == Kind::Host && takes_node(s))
        });
        let sig = sig.ok_or(Error {
            pos,
            msg: format!("no host function `{name}(node: Node, ..)`"),
        })?;
        self.node_call(base, &sig, args, pos, name)
    }

    fn node_call(
        &mut self,
        base: &Expr,
        sig: &super::Sig,
        args: &[Expr],
        pos: Pos,
        name: &str,
    ) -> R<Ty> {
        self.expr(base)?;
        let borrowed = self.host_args(&sig.params[1..], args, pos, name)?;
        self.e(I::Call(sig.idx));
        for b in borrowed {
            self.release(&b);
        }
        Ok(sig.ret.clone())
    }

    // `a.get("k")` is the host's `get(a, "k")`; `a["k"]` and `a[0]` are the
    // same calls.
    pub(crate) fn any_method(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        let Some(m) = crate::anys::method(name) else {
            return self.held_method(base, name, args, pos);
        };
        if args.len() != m.params.len() {
            return err(
                pos,
                format!(
                    "`{name}` takes {} argument(s), {} given",
                    m.params.len(),
                    args.len()
                ),
            );
        }
        let all: Vec<Expr> = std::iter::once(base.clone())
            .chain(args.iter().cloned())
            .collect();
        let borrowed = self.host_args(&crate::anys::params(m), &all, pos, name)?;
        self.e(I::Call(self.g.imp.anys[name]));
        for b in borrowed {
            self.release(&b);
        }
        Ok(crate::anys::ty(m.ret))
    }

    // A method an `any` lacks but what it holds has: a `T?`'s, read as nil
    // or not; a string's or a node's, called on it read as one.
    fn held_method(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        match (name, args.len()) {
            ("is_some" | "is_none", 0) => {
                self.any_method(base, "is_nil", &[], pos)?;
                if name == "is_some" {
                    self.e(I::I32Eqz);
                }
                return Ok(Ty::Bool);
            }
            ("unwrap", 0) => return self.expr(base),
            ("or" | "unwrap_or", 1) => {
                self.expr(base)?;
                let v = self.temp(&Ty::Any);
                self.set_raw(&v);
                self.get_raw(&v);
                self.e(I::Call(self.g.imp.anys["is_nil"]));
                self.depth += 1;
                self.e(I::If(BlockType::Result(ValType::I32)));
                self.release(&v);
                self.expr_as(&args[0], &Ty::Any)?;
                self.e(I::Else);
                self.take_out(&v);
                self.e(I::End);
                self.depth -= 1;
                return Ok(Ty::Any);
            }
            _ => {}
        }
        let held = if crate::strings::method(name).is_some() {
            Ty::Str
        } else if self.g.sigs.contains_key(&format!("node.{name}")) {
            Ty::Node
        } else {
            let names: Vec<&str> = crate::anys::METHODS.iter().map(|m| m.name).collect();
            return err(
                pos,
                format!("an `any` has `{}`, not `{name}`", names.join("`, `")),
            );
        };
        self.expr(base)?;
        self.unbox_any(&held);
        let lv = self.alloc(&held);
        self.set_raw(&lv);
        let hidden = format!("#held{}", lv.slots[0]);
        self.declare(&hidden, lv.clone());
        let var = Expr {
            kind: ExprKind::Var(hidden),
            pos,
        };
        let out = self.method(&var, name, args, pos)?;
        self.release(&lv);
        Ok(out)
    }

    // `x.as_float()` on a value that is one already, or converts plainly.
    fn plain_as(&mut self, base: &Expr, name: &str, pos: Pos) -> R<Ty> {
        let to = match name {
            "as_int" => Ty::Int,
            "as_float" => Ty::Float,
            "as_bool" => Ty::Bool,
            "as_str" => Ty::Str,
            "as_node" => Ty::Node,
            _ => return err(pos, format!("no method `{name}` on this value")),
        };
        let from = self.expr(base)?;
        match (&from, &to) {
            (a, b) if a == b => {}
            (Ty::Int, Ty::Float) => self.e(I::F64ConvertI64S),
            (Ty::Float, Ty::Int) => self.e(I::I64TruncSatF64S),
            (Ty::Int | Ty::Float | Ty::Bool | Ty::Node, Ty::Str) => self.stringify(&from, pos)?,
            _ => {
                return err(
                    pos,
                    format!("a {} cannot be read as {}", show(&from), show(&to)),
                );
            }
        }
        Ok(to)
    }

    // `s.contains(t)` is the host's `contains(s, t)`.
    fn str_method(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        let Some(m) = crate::strings::method(name) else {
            let names: Vec<&str> = crate::strings::METHODS.iter().map(|m| m.name).collect();
            return err(
                pos,
                format!("strings have `len`, `{}`, not `{name}`", names.join("`, `")),
            );
        };
        if args.len() != m.params.len() {
            return err(
                pos,
                format!(
                    "`{name}` takes {} argument(s), {} given",
                    m.params.len(),
                    args.len()
                ),
            );
        }
        let all: Vec<Expr> = std::iter::once(base.clone())
            .chain(args.iter().cloned())
            .collect();
        let borrowed = self.host_args(&crate::strings::params(m), &all, pos, name)?;
        self.e(I::Call(self.g.imp.strs[name]));
        for b in borrowed {
            self.release(&b);
        }
        Ok(crate::strings::ty(m.ret))
    }

    // `at`, `extend` and `sort`, written with what a list has already.
    fn list_sugar(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Option<Ty>> {
        match (name, args) {
            // `xs.at(i)` is `xs[i]`.
            ("at", [i]) => {
                let read = Expr {
                    kind: ExprKind::Index(Box::new(base.clone()), Box::new(i.clone())),
                    pos,
                };
                return self.expr(&read).map(Some);
            }
            // `xs.extend(ys)` pushes each of `ys`.
            ("extend", [other]) => {
                let var = format!("#each{}", self.locals.len());
                let push = Expr {
                    kind: ExprKind::Method(
                        Box::new(base.clone()),
                        "push".into(),
                        vec![Expr {
                            kind: ExprKind::Var(var.clone()),
                            pos,
                        }],
                    ),
                    pos,
                };
                let body = Block {
                    stmts: vec![Stmt {
                        kind: StmtKind::Expr(push),
                        pos,
                    }],
                    tail: None,
                };
                let walk = Stmt {
                    kind: StmtKind::ForEach {
                        var,
                        items: other.clone(),
                        body,
                    },
                    pos,
                };
                self.stmt(&walk)?;
                return Ok(Some(Ty::Void));
            }
            // `xs.sort()` puts the list in order where it is.
            ("sort", []) => {
                let Some(lv) = self.place(base)? else {
                    return err(
                        pos,
                        "`sort` changes its list: call it on a variable or a field",
                    );
                };
                let t = lv.ty.clone();
                self.expr(base)?;
                self.coerce(&t, &Ty::Any, pos)?;
                let v = self.temp(&Ty::Any);
                self.set_raw(&v);
                self.get_raw(&v);
                self.e(I::Call(self.g.imp.anys["sorted"]));
                self.release(&v);
                self.unbox_any(&t);
                self.set_owned(&lv);
                return Ok(Some(Ty::Void));
            }
            _ => {}
        }
        Ok(None)
    }

    fn array_method(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        if let Some(t) = self.list_sugar(base, name, args, pos)? {
            return Ok(t);
        }
        let (a, et, release) = self.array_ref(base, pos)?;
        let esz = self.g.elem_size(&et) as i32;
        let out = match name {
            "len" => {
                self.args(&[], args, pos, "len")?;
                self.emit([I::LocalGet(a), I::I32Load(mem(4, 2)), I::I64ExtendI32U]);
                Ty::Int
            }
            "push" => {
                if args.len() != 1 {
                    return err(pos, "`push` takes one value");
                }
                self.expr_as(&args[0], &et)?;
                let v = self.alloc(&et);
                self.set_raw(&v);
                self.emit([
                    I::LocalGet(a),
                    I::I32Const(esz),
                    I::Call(self.g.h.arr_reserve),
                ]);
                let addr = self.end_addr(a, esz, 0);
                for (j, (vt, _)) in self.g.slots(&et).into_iter().enumerate() {
                    self.emit([
                        I::LocalGet(addr),
                        I::LocalGet(v.slots[j]),
                        store(vt, 8 * j as u64),
                    ]);
                }
                self.emit([
                    I::LocalGet(a),
                    I::LocalGet(a),
                    I::I32Load(mem(4, 2)),
                    I::I32Const(1),
                    I::I32Add,
                    I::I32Store(mem(4, 2)),
                ]);
                Ty::Void
            }
            "pop" => {
                self.args(&[], args, pos, "pop")?;
                self.emit([
                    I::LocalGet(a),
                    I::I32Load(mem(4, 2)),
                    I::I32Eqz,
                    I::If(BlockType::Empty),
                    I::Unreachable,
                    I::End,
                ]);
                self.emit([
                    I::LocalGet(a),
                    I::LocalGet(a),
                    I::I32Load(mem(4, 2)),
                    I::I32Const(1),
                    I::I32Sub,
                    I::I32Store(mem(4, 2)),
                ]);
                let addr = self.end_addr(a, esz, 0);
                for (j, (vt, _)) in self.g.slots(&et).into_iter().enumerate() {
                    self.emit([I::LocalGet(addr), load(vt, 8 * j as u64)]);
                }
                et.clone()
            }
            "insert" => self.array_insert(a, &et, esz, args, pos)?,
            "remove" => self.array_remove(a, &et, esz, args, pos)?,
            _ => {
                return err(
                    pos,
                    format!(
                        "arrays have `len`, `push`, `pop`, `insert` and `remove`, not `{name}`"
                    ),
                );
            }
        };
        if let Some(r) = release {
            self.release(&r);
        }
        Ok(out)
    }

    // The address just past the last element, plus `extra` elements.
    fn end_addr(&mut self, a: u32, esz: i32, extra: i32) -> u32 {
        let addr = self.local(ValType::I32);
        self.emit([
            I::LocalGet(a),
            I::I32Load(mem(12, 2)),
            I::LocalGet(a),
            I::I32Load(mem(4, 2)),
        ]);
        self.emit([
            I::I32Const(extra),
            I::I32Add,
            I::I32Const(esz),
            I::I32Mul,
            I::I32Add,
            I::LocalSet(addr),
        ]);
        addr
    }

    // Traps unless `i` is below the length, or at it when `room` allows the end.
    fn checked_index(&mut self, a: u32, idx: &Expr, room: bool) -> R<u32> {
        let i = self.index_local(idx)?;
        self.emit([
            I::LocalGet(i),
            I::LocalGet(a),
            I::I32Load(mem(4, 2)),
            I::I64ExtendI32U,
            if room { I::I64GtU } else { I::I64GeU },
            I::If(BlockType::Empty),
            I::Unreachable,
            I::End,
        ]);
        Ok(i)
    }

    // The address of element `i`, kept in a local.
    fn element_at(&mut self, a: u32, i: u32, esz: i32) -> u32 {
        let addr = self.local(ValType::I32);
        self.emit([
            I::LocalGet(a),
            I::I32Load(mem(12, 2)),
            I::LocalGet(i),
            I::I32WrapI64,
            I::I32Const(esz),
            I::I32Mul,
            I::I32Add,
            I::LocalSet(addr),
        ]);
        addr
    }

    // `(len - i - skip) * esz` bytes: what lies after the gap being opened or closed.
    fn tail_bytes(&mut self, a: u32, i: u32, esz: i32, skip: i32) {
        self.emit([
            I::LocalGet(a),
            I::I32Load(mem(4, 2)),
            I::LocalGet(i),
            I::I32WrapI64,
            I::I32Sub,
            I::I32Const(skip),
            I::I32Sub,
            I::I32Const(esz),
            I::I32Mul,
        ]);
    }

    fn bump_len(&mut self, a: u32, by: i32) {
        self.emit([
            I::LocalGet(a),
            I::LocalGet(a),
            I::I32Load(mem(4, 2)),
            I::I32Const(by),
            I::I32Add,
            I::I32Store(mem(4, 2)),
        ]);
    }

    // `xs.insert(i, v)`: what was at `i` and after moves up one.
    fn array_insert(&mut self, a: u32, et: &Ty, esz: i32, args: &[Expr], pos: Pos) -> R<Ty> {
        if args.len() != 2 {
            return err(pos, "`insert` takes an index and a value");
        }
        let i = self.checked_index(a, &args[0], true)?;
        self.expr_as(&args[1], et)?;
        let v = self.alloc(et);
        self.set_raw(&v);
        self.emit([
            I::LocalGet(a),
            I::I32Const(esz),
            I::Call(self.g.h.arr_reserve),
        ]);
        let at = self.element_at(a, i, esz);
        self.emit([
            I::LocalGet(at),
            I::I32Const(esz),
            I::I32Add,
            I::LocalGet(at),
        ]);
        self.tail_bytes(a, i, esz, 0);
        self.e(I::MemoryCopy {
            src_mem: 0,
            dst_mem: 0,
        });
        for (j, (vt, _)) in self.g.slots(et).into_iter().enumerate() {
            self.emit([
                I::LocalGet(at),
                I::LocalGet(v.slots[j]),
                store(vt, 8 * j as u64),
            ]);
        }
        self.bump_len(a, 1);
        Ok(Ty::Void)
    }

    // `xs.remove(i)`: the element leaves the array to the caller, and what was
    // after it moves down one.
    fn array_remove(&mut self, a: u32, et: &Ty, esz: i32, args: &[Expr], pos: Pos) -> R<Ty> {
        if args.len() != 1 {
            return err(pos, "`remove` takes an index");
        }
        let i = self.checked_index(a, &args[0], false)?;
        let at = self.element_at(a, i, esz);
        for (j, (vt, _)) in self.g.slots(et).into_iter().enumerate() {
            self.emit([I::LocalGet(at), load(vt, 8 * j as u64)]);
        }
        let v = self.alloc(et);
        self.set_raw(&v);
        self.emit([
            I::LocalGet(at),
            I::LocalGet(at),
            I::I32Const(esz),
            I::I32Add,
        ]);
        self.tail_bytes(a, i, esz, 1);
        self.e(I::MemoryCopy {
            src_mem: 0,
            dst_mem: 0,
        });
        self.bump_len(a, -1);
        self.get_raw(&v);
        Ok(et.clone())
    }
}

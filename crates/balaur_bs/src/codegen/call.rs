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
        if sig.kind != Kind::Script || sig.owner.is_some() || self.g.lends_to(name) {
            return err(
                pos,
                format!(
                    "only a script's own plain function can be a `fn` value; `{name}` is not one"
                ),
            );
        }
        let slot = self.g.fn_ref(sig.idx);
        self.e(I::I32Const(slot as i32));
        // A unit's slots start at its table base.
        if self.g.unit {
            self.emit([I::GlobalGet(super::rt::G_TABLE_BASE), I::I32Add]);
        }
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
        // Called as a script function is, `&mut` arguments stored back; its
        // type says how many it takes, so none is left out.
        let sig = super::Sig {
            idx: 0,
            params: params.clone(),
            refs: refs.clone(),
            ret: (**ret).clone(),
            owner: None,
            kind: Kind::Script,
        };
        let places = self.script_args(&sig, sig.params.len(), args, pos, name)?;
        self.get_raw(f);
        self.spill();
        let (p, r) = self.g.wasm_sig(&sig);
        self.g.calls_indirect.set(true);
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
        if self.prelude
            && name.starts_with("__")
            && let Some(ty) = self.intrinsic(name, args, pos)?
        {
            return Ok(ty);
        }
        // A script reaches the prelude's functions as methods only.
        if !self.prelude && crate::prelude::owns(name) {
            return err(pos, format!("unknown function `{name}`"));
        }
        // `math::sqrt` is wasm's own instruction, and the rest of `math`'s
        // functions of a float fixed imports, unless a module of the project's
        // goes by that name.
        let scripted = self
            .g
            .sigs
            .get(name)
            .is_some_and(|sig| !matches!(sig.kind, Kind::Extern | Kind::Host));
        let native = match name.strip_prefix("math::").filter(|_| !scripted) {
            Some("sqrt") => Some((I::F64Sqrt, 1)),
            Some("abs") => Some((I::F64Abs, 1)),
            Some("floor") => Some((I::F64Floor, 1)),
            Some("ceil") => Some((I::F64Ceil, 1)),
            Some("min") => Some((I::F64Min, 2)),
            Some("max") => Some((I::F64Max, 2)),
            Some(function) => self
                .g
                .imp
                .math
                .get(if function == "log" { "ln" } else { function })
                .map(|(idx, n)| (I::Call(*idx), *n)),
            None => None,
        };
        if let Some((i, n)) = native {
            self.args(&vec![Ty::Float; n], args, pos, name)?;
            self.e(i);
            return Ok(Ty::Float);
        }
        if name == "map" {
            return err(
                pos,
                "there is no `map()`: an `any` map is a literal where an `any` goes, `let m: any = {};`",
            );
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
            msg: self.no_function(name),
        })?;
        if matches!(sig.kind, Kind::Extern | Kind::Host) {
            self.no_node_for_a_reply(name, args, pos)?;
        }
        match sig.kind {
            Kind::ExternAsync | Kind::HostAsync | Kind::Async => {
                return err(
                    pos,
                    format!("`{name}` is async: `await {name}(..)` inside an `async fn`"),
                );
            }
            Kind::Extern | Kind::Host => {
                let borrowed =
                    self.host_args(&sig.params, &args.iter().collect::<Vec<_>>(), pos, name)?;
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
            Kind::Script if self.g.lends_to(name) => {
                let held =
                    self.host_args(&sig.params, &args.iter().collect::<Vec<_>>(), pos, name)?;
                self.spill();
                self.e(I::Call(self.g.callee(name, &sig)));
                self.release_all(held);
            }
            Kind::Script => {
                let places = self.script_args(&sig, sig.least(), args, pos, name)?;
                self.spill();
                self.e(I::Call(self.g.callee(name, &sig)));
                if let Some(r) = self.store_back(places, &sig.ret) {
                    self.get_raw(&r);
                }
            }
        }
        Ok(sig.ret)
    }

    // Why `name` calls nothing.
    fn no_function(&self, name: &str) -> String {
        match name.split_once("::") {
            Some((_, f)) => super::dropped::function(name)
                .or_else(|| self.on_a_component(name, f))
                .unwrap_or_else(|| format!("the engine has no `{name}`")),
            None => format!("unknown function `{name}`"),
        }
    }

    // A module's function that drives a component is called on it.
    fn on_a_component(&self, name: &str, function: &str) -> Option<String> {
        let mut on: Vec<String> = self
            .g
            .host
            .functions
            .iter()
            .filter_map(|host| {
                let (component, op) = host.key.strip_prefix("node.")?.split_once('.')?;
                (op == function).then(|| format!("`node.{component}.{function}(..)`"))
            })
            .collect();
        if on.is_empty() {
            return None;
        }
        on.sort();
        Some(format!(
            "`{name}` is called on its component: {}",
            on.join(" or ")
        ))
    }

    /// A call whose one reply is what `task::wait` answers names no node to tell.
    fn no_node_for_a_reply(&mut self, name: &str, args: &[Expr], pos: Pos) -> R<()> {
        let Some(first) = args
            .first()
            .filter(|_| super::dropped::answers_by_await(name))
        else {
            return Ok(());
        };
        // A map literal has no type until it lands, and that is no node either.
        if matches!(&first.kind, ExprKind::Nil) || self.type_of(first).ok() == Some(Ty::Node) {
            return err(
                pos,
                format!(
                    "`{name}` takes no node: await what it answers, `await task::wait({name}(..))`"
                ),
            );
        }
        Ok(())
    }

    /// A spelling a value has no more, refused by name with the one it keeps.
    /// A prelude function may call what its own code needs by any name.
    pub(crate) fn dropped_form(&mut self, base: &Expr, name: &str, pos: Pos) -> R<()> {
        let nil_test = matches!(name, "is_nil" | "is_none" | "is_some" | "unwrap_or");
        let read = matches!(
            name,
            "as_int" | "as_float" | "as_bool" | "as_str" | "as_node"
        );
        if self.prelude || !(nil_test || read || matches!(name, "get" | "at" | "set" | "put")) {
            return Ok(());
        }
        let msg = match (&self.type_of(base)?, name) {
            (Ty::Any | Ty::Optional(_), "is_nil" | "is_none") => {
                format!("write `x == nil`, not `x.{name}()`")
            }
            (Ty::Any | Ty::Optional(_), "is_some") => {
                "write `x != nil`, not `x.is_some()`".to_string()
            }
            (Ty::Any | Ty::Optional(_), "unwrap_or") => {
                "write `x.or(v)`, not `x.unwrap_or(v)`".to_string()
            }
            (Ty::Any, "get") => {
                "an `any` reads a key as `a[\"k\"]`, not `a.get(\"k\")`".to_string()
            }
            (Ty::Any, "at") => {
                "an `any` list reads an element as `a[i]`, not `a.at(i)`".to_string()
            }
            (Ty::Any, "set") => {
                "an `any` writes a key as `a[\"k\"] = v`, not `a.set(\"k\", v)`".to_string()
            }
            (Ty::Any, "put") => {
                "an `any` list writes an element as `a[i] = v`, not `a.put(i, v)`".to_string()
            }
            (Ty::Array(_), "at") => {
                "a list reads an element as `xs[i]`, not `xs.at(i)`".to_string()
            }
            (Ty::Struct(s), "at" | "get") if matches!(s.as_str(), "Vec2" | "Vec3" | "Color") => {
                format!(
                    "a {s} reads a field by its name, as `v.{}`",
                    if s == "Color" { "r" } else { "x" }
                )
            }
            (Ty::Struct(s), "get") if crate::maps::is_map(s) => {
                "a map reads a key as `m[k]`, not `m.get(k)`".to_string()
            }
            (Ty::Struct(s), "set") if crate::maps::is_map(s) => {
                "a map writes a key as `m[k] = v`, not `m.set(k, v)`".to_string()
            }
            (Ty::Int, "as_float") => {
                "an int becomes a float as `x as float`, not `x.as_float()`".to_string()
            }
            (Ty::Float, "as_int") => {
                "a float becomes an int as `x as int`, not `x.as_int()`".to_string()
            }
            (Ty::Int | Ty::Float | Ty::Bool | Ty::Node, "as_str") => {
                "a value becomes text as `\"{x}\"`, not `x.as_str()`".to_string()
            }
            (from @ (Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Node), _) if read => {
                let article = if *from == Ty::Int { "an" } else { "a" };
                format!(
                    "`{name}` reads an `any`, and this is {article} {}",
                    show(from)
                )
            }
            _ => return Ok(()),
        };
        err(pos, msg)
    }

    pub(crate) fn method(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        // `node.transform.translate(..)`: an operation a component drives is
        // called on that component.
        if let ExprKind::Field(node, comp) = &base.kind
            && let Some(sig) = self.g.sigs.get(&format!("node.{comp}.{name}")).cloned()
            && sig.kind == Kind::Host
        {
            if self.type_of(node)? == Ty::Any {
                return err(
                    pos,
                    format!("`{comp}.{name}` on an `any`: read the node with `.as_node()` first"),
                );
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

    // `v.m(..)` on a class: its pointer, which the call lends and never moves.
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
        self.may_name(&key, pos)?;
        if sig.kind == Kind::Async {
            return err(
                pos,
                format!("`{key}` is async: `await self.{name}(..)` inside an `async fn`"),
            );
        }
        // A class is shared, so any expression holding one will do.
        let (lv, held) = if let Some(lv) = place {
            (lv, None)
        } else {
            self.expr(base)?;
            let t = self.temp(&bty);
            self.set_raw(&t);
            (t.clone(), Some(t))
        };
        self.get_raw(&lv);
        if self.g.lends_to(&key) {
            let lent = self.host_args(&sig.params, &args.iter().collect::<Vec<_>>(), pos, &key)?;
            self.spill();
            self.e(I::Call(sig.idx));
            self.release_all(lent);
            if let Some(t) = held {
                self.release(&t);
            }
            return Ok(sig.ret);
        }
        let places = self.script_args(&sig, sig.least(), args, pos, &key)?;
        self.spill();
        self.e(I::Call(sig.idx));
        let ret = self.store_back(places, &sig.ret);
        if let Some(t) = held {
            self.release(&t);
        }
        if let Some(r) = ret {
            self.get_raw(&r);
        }
        Ok(sig.ret)
    }

    /// A script call's arguments, `least` of them at the fewest and the rest
    /// nil. A `&mut` one is a variable or a field, passed as it is, and no
    /// other place the call changes may overlap it: each comes back on its
    /// own and the last stored would win.
    pub(crate) fn script_args(
        &mut self,
        sig: &super::Sig,
        least: usize,
        args: &[Expr],
        pos: Pos,
        name: &str,
    ) -> R<Vec<RefPlace>> {
        self.arg_count(least, sig.params.len(), args.len(), args.last(), pos, name)?;
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
            if locals.into_iter().any(overlaps) {
                return err(
                    a.pos,
                    format!(
                        "`{name}` would change this twice: it is part of another `&mut` argument"
                    ),
                );
            }
            self.get_raw(&lv);
            places.push(RefPlace::Local(lv));
        }
        for want in &sig.params[args.len()..] {
            self.nil_as(want, pos)?;
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
        let borrowed = self.host_args(
            &sig.params[1..],
            &args.iter().collect::<Vec<_>>(),
            pos,
            name,
        )?;
        self.e(I::Call(sig.idx));
        for b in borrowed {
            self.release(&b);
        }
        Ok(sig.ret.clone())
    }

    pub(crate) fn any_method(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        if name == "with" {
            return err(
                pos,
                "an `any` has no `with`: write the map as a literal, `{ \"k\": v }`, or write the key, `a[\"k\"] = v`",
            );
        }
        if name == "equals" && !self.prelude {
            return err(pos, "an `any` compares as `a == b`, not `a.equals(b)`");
        }
        if !self.g.sigs.contains_key(&format!("any::{name}")) {
            return self.held_method(base, name, args, pos);
        }
        self.prelude_method("any", base, name, args, pos)
    }

    /// `value.method(..)` on a `str` or an `any`: the prelude's
    /// `str::method(value, ..)`. The receiver is compiled where it is, not
    /// cloned into a call: a clone is a tree `type_of` has not seen, so each
    /// link of a chain would type the whole chain under it again.
    pub(crate) fn prelude_method(
        &mut self,
        ty: &str,
        base: &Expr,
        name: &str,
        args: &[Expr],
        pos: Pos,
    ) -> R<Ty> {
        let key = format!("{ty}::{name}");
        let sig = self.g.sigs[&key].clone();
        if args.len() + 1 != sig.params.len() {
            return err(
                pos,
                format!(
                    "`{name}` takes {} argument(s), {} given",
                    sig.params.len() - 1,
                    args.len()
                ),
            );
        }
        let all: Vec<&Expr> = std::iter::once(base).chain(args).collect();
        let held = self.lent(&sig.params, &all, false)?;
        self.spill();
        self.e(I::Call(self.g.callee(&key, &sig)));
        self.release_all(held);
        Ok(sig.ret)
    }

    /// A call to prelude function `key` with its arguments on the stack,
    /// each owned: held for the call, which borrows them, and released after.
    pub(crate) fn call_prelude(&mut self, key: &str) -> Ty {
        let sig = self.g.sigs[key].clone();
        let held: Vec<_> = sig.params.iter().map(|ty| self.alloc(ty)).collect();
        for temp in held.iter().rev() {
            self.set_raw(temp);
        }
        for temp in &held {
            self.get_raw(temp);
        }
        self.spill();
        self.e(I::Call(self.g.callee(key, &sig)));
        let heap = held.into_iter().filter(|temp| temp.ty.is_heap()).collect();
        self.release_all(heap);
        sig.ret
    }

    // A method an `any` lacks: a `T?`'s, read as nil or not. A string's or a
    // node's is named, with the read it wants first.
    fn held_method(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        match (name, args.len()) {
            ("unwrap", 0) => return self.expr(base),
            ("or", 1) => {
                self.expr(base)?;
                let cell = self.temp(&Ty::Any);
                self.set_raw(&cell);
                self.get_raw(&cell);
                self.e(I::I32Eqz);
                self.depth += 1;
                self.e(I::If(BlockType::Result(ValType::I32)));
                self.release(&cell);
                self.expr_as(&args[0], &Ty::Any)?;
                self.e(I::Else);
                self.take_out(&cell);
                self.e(I::End);
                self.depth -= 1;
                return Ok(Ty::Any);
            }
            _ => {}
        }
        if self.g.sigs.contains_key(&format!("str::{name}")) {
            return err(
                pos,
                format!("an `any` has no `{name}`: read it with `.as_str()` first"),
            );
        }
        let takes_node = |sig: &super::Sig| sig.params.first() == Some(&Ty::Node);
        if self.g.sigs.contains_key(&format!("node.{name}"))
            || self.g.sigs.get(name).is_some_and(takes_node)
        {
            return err(
                pos,
                format!("an `any` has no `{name}`: read it with `.as_node()` first"),
            );
        }
        let names: Vec<String> = crate::prelude::methods("any")
            .into_iter()
            .map(|method| method.0)
            .collect();
        err(
            pos,
            format!("an `any` has `{}`, not `{name}`", names.join("`, `")),
        )
    }

    // `text.contains(needle)` is the prelude's `str::contains(text, needle)`.
    fn str_method(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Ty> {
        if !self.g.sigs.contains_key(&format!("str::{name}")) {
            let names: Vec<String> = crate::prelude::methods("str")
                .into_iter()
                .map(|method| method.0)
                .collect();
            return err(
                pos,
                format!("strings have `len`, `{}`, not `{name}`", names.join("`, `")),
            );
        }
        self.prelude_method("str", base, name, args, pos)
    }

    // `extend` and `sort`, written with what a list has already.
    fn list_sugar(&mut self, base: &Expr, name: &str, args: &[Expr], pos: Pos) -> R<Option<Ty>> {
        match (name, args) {
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
                self.call_prelude("any::sorted");
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

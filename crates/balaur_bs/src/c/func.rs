// One function body: wasm's value stack becomes C variables, one per height
// and kind, and its blocks become labels a branch jumps to. A memory's base
// and length live in locals, reloaded after anything that can grow it.

use super::{Module, c_type, code, each, err, from_bits, to_bits};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use wasmparser::{BlockType, FunctionBody, MemArg, Operator, ValType};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Func,
    Block,
    Loop,
    If,
}

struct Frame {
    kind: Kind,
    label: usize,
    /// The stack height its results start at.
    height: usize,
    results: Vec<ValType>,
    /// Whether a branch jumps to its end, which then needs a label.
    used: bool,
}

struct Gen<'a, 'm> {
    m: &'a Module<'m>,
    p: &'a str,
    out: String,
    stack: Vec<ValType>,
    vars: BTreeSet<(usize, char)>,
    frames: Vec<Frame>,
    labels: usize,
    /// After a branch, a return or a trap, until the block it is in ends.
    dead: bool,
    dead_nest: usize,
    locals: Vec<ValType>,
    ty: u32,
}

pub(super) fn emit(
    m: &Module<'_>,
    p: &str,
    index: u32,
    body: &FunctionBody<'_>,
    out: &mut String,
) -> Result<(), String> {
    let ty = m.funcs[index as usize];
    let sig = &m.types[ty as usize];
    let mut locals: Vec<ValType> = sig.params().to_vec();
    let mut reader = body.get_locals_reader().map_err(err)?;
    for _ in 0..reader.get_count() {
        let (n, t) = reader.read().map_err(err)?;
        locals.extend(std::iter::repeat_n(t, n as usize));
    }
    let mut g = Gen {
        m,
        p,
        out: String::new(),
        stack: vec![],
        vars: BTreeSet::new(),
        frames: vec![Frame {
            kind: Kind::Func,
            label: 0,
            height: 0,
            results: sig.results().to_vec(),
            used: false,
        }],
        labels: 1,
        dead: false,
        dead_nest: 0,
        locals: locals.clone(),
        ty,
    };
    let mut ops = body.get_operators_reader().map_err(err)?;
    while !ops.eof() {
        let op = ops.read().map_err(err)?;
        g.op(&op)?;
    }
    let _ = writeln!(out, "BS_FN {} {{", m.signature(p, index));
    out.push_str("  uint8_t *M = I->mem;\n  uint64_t L = I->mem_len;\n");
    for (j, t) in locals.iter().enumerate().skip(sig.params().len()) {
        let _ = writeln!(out, "  {} l{j} = 0;", c_type(*t));
    }
    for (h, c) in &g.vars {
        let t = match c {
            'i' => "uint32_t",
            'l' => "uint64_t",
            _ => "double",
        };
        let _ = writeln!(out, "  {t} s{h}{c} = 0;");
    }
    out.push_str("  (void)M;\n  (void)L;\n  BS_ENTER(I);\n");
    out.push_str(&g.out);
    out.push_str("}\n");
    Ok(())
}

fn var(h: usize, t: ValType) -> String {
    format!("s{h}{}", code(t))
}

fn addr(a: &str, m: &MemArg) -> String {
    format!("(uint64_t){a} + {}ull", m.offset)
}

impl Gen<'_, '_> {
    fn line(&mut self, s: &str) {
        self.out.push_str("  ");
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn push(&mut self, t: ValType) -> String {
        let h = self.stack.len();
        self.stack.push(t);
        self.vars.insert((h, code(t)));
        var(h, t)
    }

    fn pop(&mut self) -> Result<(String, ValType), String> {
        let t = self.stack.pop().ok_or("the stack ran dry")?;
        Ok((var(self.stack.len(), t), t))
    }

    fn top(&self) -> Result<String, String> {
        let t = self.stack.last().ok_or("the stack is empty")?;
        Ok(var(self.stack.len() - 1, *t))
    }

    fn block_results(&self, bt: BlockType) -> Result<Vec<ValType>, String> {
        Ok(match bt {
            BlockType::Empty => vec![],
            BlockType::Type(t) => vec![t],
            BlockType::FuncType(i) => {
                let ty = &self.m.types[i as usize];
                if !ty.params().is_empty() {
                    return Err("a block with parameters".into());
                }
                ty.results().to_vec()
            }
        })
    }

    fn open(&mut self, kind: Kind, results: Vec<ValType>) -> usize {
        let label = self.labels;
        self.labels += 1;
        self.frames.push(Frame {
            kind,
            label,
            height: self.stack.len(),
            results,
            used: false,
        });
        label
    }

    fn ret(&self) -> Result<String, String> {
        let results = self.m.types[self.ty as usize].results();
        let n = results.len();
        let base = self
            .stack
            .len()
            .checked_sub(n)
            .ok_or("a return short of results")?;
        Ok(match n {
            0 => "return;".into(),
            1 => format!("return {};", var(base, results[0])),
            _ => {
                let mut s = format!("{{ {} v;", self.m.ret_type(self.p, self.ty));
                for (j, t) in results.iter().enumerate() {
                    let _ = write!(s, " v.r{j} = {};", var(base + j, *t));
                }
                s.push_str(" return v; }");
                s
            }
        })
    }

    /// What jumping to the frame `depth` out does: results moved to where it
    /// expects them, then a jump.
    fn branch(&mut self, depth: u32) -> Result<String, String> {
        let at = self
            .frames
            .len()
            .checked_sub(1 + depth as usize)
            .ok_or("a branch out of the function")?;
        let (kind, label, height, results) = {
            let f = &self.frames[at];
            (f.kind, f.label, f.height, f.results.clone())
        };
        match kind {
            Kind::Func => self.ret(),
            Kind::Loop => Ok(format!("goto L{label}_top;")),
            Kind::Block | Kind::If => {
                self.frames[at].used = true;
                let n = results.len();
                let base = self
                    .stack
                    .len()
                    .checked_sub(n)
                    .ok_or("a branch short of values")?;
                let mut s = String::new();
                for (j, t) in results.iter().enumerate() {
                    if base + j != height + j {
                        let _ = write!(s, "{} = {}; ", var(height + j, *t), var(base + j, *t));
                    }
                }
                let _ = write!(s, "goto L{label};");
                Ok(s)
            }
        }
    }

    fn un(&mut self, out: ValType, f: impl Fn(&str) -> String) -> Result<(), String> {
        let (a, _) = self.pop()?;
        let r = self.push(out);
        self.line(&format!("{r} = {};", f(&a)));
        Ok(())
    }

    fn bin(&mut self, out: ValType, f: impl Fn(&str, &str) -> String) -> Result<(), String> {
        let (b, _) = self.pop()?;
        let (a, _) = self.pop()?;
        let r = self.push(out);
        self.line(&format!("{r} = {};", f(&a, &b)));
        Ok(())
    }

    fn load(&mut self, m: &MemArg, out: ValType, read: &str) -> Result<(), String> {
        let (a, _) = self.pop()?;
        let r = self.push(out);
        let at = addr(&a, m);
        self.line(&format!(
            "{r} = {};",
            read.replace('@', &format!("I, M, L, {at}"))
        ));
        Ok(())
    }

    fn store(&mut self, m: &MemArg, write: &str, cast: &str) -> Result<(), String> {
        let (v, _) = self.pop()?;
        let (a, _) = self.pop()?;
        let at = addr(&a, m);
        self.line(&format!("{write}(I, M, L, {at}, {cast}{v});"));
        Ok(())
    }

    fn call(&mut self, f: u32) -> Result<(), String> {
        let ty = self.m.func_type(f).clone();
        let args = self.call_args(ty.params().len())?;
        let call = format!("{}_f{f}(I{args})", self.p);
        self.results(&ty, self.m.funcs[f as usize], &call);
        Ok(())
    }

    // `call_indirect`: the slot is on top, the arguments under it.
    fn call_indirect(&mut self, t: u32) -> Result<(), String> {
        let ty = self.m.types[t as usize].clone();
        let (slot, _) = self.pop()?;
        let args = self.call_args(ty.params().len())?;
        let call = format!("{}_ci{t}(I, {slot}{args})", self.p);
        self.results(&ty, t, &call);
        Ok(())
    }

    fn call_args(&mut self, n: usize) -> Result<String, String> {
        let mut args = vec![];
        for _ in 0..n {
            args.push(self.pop()?.0);
        }
        args.reverse();
        Ok(each(&args, |s, a| {
            let _ = write!(s, ", {a}");
        }))
    }

    // What a call leaves: nothing, one value, or a struct's fields, and the
    // memory's base and length read again, since the callee may have grown it.
    fn results(&mut self, ty: &wasmparser::FuncType, type_index: u32, call: &str) {
        match ty.results().len() {
            0 => self.line(&format!("{call};")),
            1 => {
                let r = self.push(ty.results()[0]);
                self.line(&format!("{r} = {call};"));
            }
            _ => {
                let mut s = format!("{{ {} v = {call};", self.m.ret_type(self.p, type_index));
                for (j, t) in ty.results().iter().enumerate() {
                    let r = self.push(*t);
                    let _ = write!(s, " {r} = v.r{j};");
                }
                s.push_str(" }");
                self.line(&s);
            }
        }
        self.line("BS_RELOAD();");
    }

    // Past a branch nothing runs until the block ends, so only the nesting
    // is followed to find where that is.
    fn skipped(&mut self, op: &Operator<'_>) -> bool {
        if !self.dead {
            return false;
        }
        match op {
            Operator::Block { .. } | Operator::Loop { .. } | Operator::If { .. } => {
                self.dead_nest += 1;
                true
            }
            Operator::End if self.dead_nest > 0 => {
                self.dead_nest -= 1;
                true
            }
            Operator::Else if self.dead_nest > 0 => true,
            Operator::End | Operator::Else => false,
            _ => true,
        }
    }

    fn op(&mut self, op: &Operator<'_>) -> Result<(), String> {
        if self.skipped(op) {
            return Ok(());
        }
        if self.control(op)?
            || self.values(op)?
            || self.memory(op)?
            || self.int(op)?
            || self.float(op)?
        {
            return Ok(());
        }
        Err(format!("{op:?} is not written"))
    }

    fn control(&mut self, op: &Operator<'_>) -> Result<bool, String> {
        use Operator as O;
        match op {
            O::Nop => {}
            O::Unreachable => {
                self.line("bs_trap(I, BS_UNREACHABLE);");
                self.dead = true;
            }
            O::Block { blockty } => {
                let results = self.block_results(*blockty)?;
                self.open(Kind::Block, results);
            }
            O::Loop { blockty } => {
                let results = self.block_results(*blockty)?;
                let label = self.open(Kind::Loop, results);
                let _ = writeln!(self.out, "L{label}_top:;");
            }
            O::If { blockty } => {
                let results = self.block_results(*blockty)?;
                let (c, _) = self.pop()?;
                self.line(&format!("if ({c}) {{"));
                self.open(Kind::If, results);
            }
            O::Else => {
                let height = self.frames.last().ok_or("an else outside a block")?.height;
                self.stack.truncate(height);
                self.dead = false;
                self.line("} else {");
            }
            O::End => self.end()?,
            O::Br { relative_depth } => {
                let s = self.branch(*relative_depth)?;
                self.line(&s);
                self.dead = true;
            }
            O::BrIf { relative_depth } => {
                let (c, _) = self.pop()?;
                let s = self.branch(*relative_depth)?;
                self.line(&format!("if ({c}) {{ {s} }}"));
            }
            O::BrTable { targets } => {
                let (i, _) = self.pop()?;
                let mut s = format!("switch ({i}) {{");
                for (n, t) in targets.targets().enumerate() {
                    let b = self.branch(t.map_err(err)?)?;
                    let _ = write!(s, " case {n}: {{ {b} }}");
                }
                let b = self.branch(targets.default())?;
                let _ = write!(s, " default: {{ {b} }} }}");
                self.line(&s);
                self.dead = true;
            }
            O::Return => {
                let s = self.ret()?;
                self.line(&s);
                self.dead = true;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    // Calls, constants, and moving values between the stack, locals and globals.
    fn values(&mut self, op: &Operator<'_>) -> Result<bool, String> {
        use Operator as O;
        match op {
            O::Call { function_index } => self.call(*function_index)?,
            O::CallIndirect { type_index, .. } => self.call_indirect(*type_index)?,
            O::Drop => {
                self.pop()?;
            }
            O::Select => {
                let (c, _) = self.pop()?;
                let (b, _) = self.pop()?;
                let (a, t) = self.pop()?;
                let r = self.push(t);
                self.line(&format!("{r} = {c} ? {a} : {b};"));
            }
            O::LocalGet { local_index } => {
                let t = self.locals[*local_index as usize];
                let r = self.push(t);
                self.line(&format!("{r} = l{local_index};"));
            }
            O::LocalSet { local_index } => {
                let (v, _) = self.pop()?;
                self.line(&format!("l{local_index} = {v};"));
            }
            O::LocalTee { local_index } => {
                let v = self.top()?;
                self.line(&format!("l{local_index} = {v};"));
            }
            O::GlobalGet { global_index } => {
                let t = self.m.globals[*global_index as usize].0;
                let r = self.push(t);
                let v = from_bits(t, &format!("I->g[{global_index}]"));
                self.line(&format!("{r} = {v};"));
            }
            O::GlobalSet { global_index } => {
                let t = self.m.globals[*global_index as usize].0;
                let (v, _) = self.pop()?;
                let bits = to_bits(t, &v);
                self.line(&format!("I->g[{global_index}] = {bits};"));
            }
            O::I32Const { value } => {
                let r = self.push(ValType::I32);
                self.line(&format!("{r} = {}u;", value.cast_unsigned()));
            }
            O::I64Const { value } => {
                let r = self.push(ValType::I64);
                self.line(&format!("{r} = {}ull;", value.cast_unsigned()));
            }
            O::F64Const { value } => {
                let r = self.push(ValType::F64);
                self.line(&format!("{r} = bs_f64({}ull);", value.bits()));
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn end(&mut self) -> Result<(), String> {
        let f = self.frames.pop().ok_or("an end outside a block")?;
        if f.kind == Kind::Func {
            if self.dead {
                self.line("__builtin_unreachable();");
            } else {
                let s = self.ret()?;
                self.line(&s);
            }
            return Ok(());
        }
        if f.kind == Kind::If {
            self.line("}");
        }
        if f.used {
            let _ = writeln!(self.out, "L{}:;", f.label);
        }
        self.stack.truncate(f.height);
        for t in &f.results {
            self.push(*t);
        }
        self.dead = false;
        Ok(())
    }

    fn memory(&mut self, op: &Operator<'_>) -> Result<bool, String> {
        use Operator as O;
        use ValType::{F64, I32, I64};
        match op {
            O::I32Load { memarg } => self.load(memarg, I32, "bs_ld32(@)")?,
            O::I32Load8U { memarg } => self.load(memarg, I32, "(uint32_t)bs_ld8(@)")?,
            O::I32Load8S { memarg } => {
                self.load(memarg, I32, "(uint32_t)(int32_t)(int8_t)bs_ld8(@)")?;
            }
            O::I32Load16U { memarg } => self.load(memarg, I32, "(uint32_t)bs_ld16(@)")?,
            O::I32Load16S { memarg } => {
                self.load(memarg, I32, "(uint32_t)(int32_t)(int16_t)bs_ld16(@)")?;
            }
            O::I64Load { memarg } => self.load(memarg, I64, "bs_ld64(@)")?,
            O::I64Load8U { memarg } => self.load(memarg, I64, "(uint64_t)bs_ld8(@)")?,
            O::I64Load16U { memarg } => self.load(memarg, I64, "(uint64_t)bs_ld16(@)")?,
            O::I64Load32U { memarg } => self.load(memarg, I64, "(uint64_t)bs_ld32(@)")?,
            O::I64Load32S { memarg } => {
                self.load(memarg, I64, "(uint64_t)(int64_t)(int32_t)bs_ld32(@)")?;
            }
            O::F64Load { memarg } => self.load(memarg, F64, "bs_ldf64(@)")?,
            O::I32Store { memarg } => self.store(memarg, "bs_st32", "")?,
            O::I32Store8 { memarg } | O::I64Store8 { memarg } => {
                self.store(memarg, "bs_st8", "(uint8_t)")?;
            }
            O::I32Store16 { memarg } | O::I64Store16 { memarg } => {
                self.store(memarg, "bs_st16", "(uint16_t)")?;
            }
            O::I64Store { memarg } => self.store(memarg, "bs_st64", "")?,
            O::I64Store32 { memarg } => self.store(memarg, "bs_st32", "(uint32_t)")?,
            O::F64Store { memarg } => self.store(memarg, "bs_stf64", "")?,
            O::MemorySize { .. } => {
                let r = self.push(I32);
                self.line(&format!("{r} = (uint32_t)(L >> 16);"));
            }
            O::MemoryGrow { .. } => {
                let (a, _) = self.pop()?;
                let r = self.push(I32);
                self.line(&format!("{r} = bs_grow(I, {a});"));
                self.line("BS_RELOAD();");
            }
            O::MemoryCopy { .. } => {
                let (n, _) = self.pop()?;
                let (s, _) = self.pop()?;
                let (d, _) = self.pop()?;
                self.line(&format!("bs_copy(I, M, L, {d}, {s}, {n});"));
            }
            O::MemoryFill { .. } => {
                let (n, _) = self.pop()?;
                let (v, _) = self.pop()?;
                let (d, _) = self.pop()?;
                self.line(&format!("bs_fill(I, M, L, {d}, {v}, {n});"));
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn int(&mut self, op: &Operator<'_>) -> Result<bool, String> {
        use Operator as O;
        use ValType::{I32, I64};
        let s32 = |a: &str| format!("(int32_t){a}");
        let s64 = |a: &str| format!("(int64_t){a}");
        match op {
            O::I32Eqz => self.un(I32, |a| format!("{a} == 0"))?,
            O::I64Eqz => self.un(I32, |a| format!("{a} == 0"))?,
            O::I32Eq | O::I64Eq => self.bin(I32, |a, b| format!("{a} == {b}"))?,
            O::I32Ne | O::I64Ne => self.bin(I32, |a, b| format!("{a} != {b}"))?,
            O::I32LtU | O::I64LtU => self.bin(I32, |a, b| format!("{a} < {b}"))?,
            O::I32GtU | O::I64GtU => self.bin(I32, |a, b| format!("{a} > {b}"))?,
            O::I32LeU | O::I64LeU => self.bin(I32, |a, b| format!("{a} <= {b}"))?,
            O::I32GeU | O::I64GeU => self.bin(I32, |a, b| format!("{a} >= {b}"))?,
            O::I32LtS => self.bin(I32, |a, b| format!("{} < {}", s32(a), s32(b)))?,
            O::I32GtS => self.bin(I32, |a, b| format!("{} > {}", s32(a), s32(b)))?,
            O::I32LeS => self.bin(I32, |a, b| format!("{} <= {}", s32(a), s32(b)))?,
            O::I32GeS => self.bin(I32, |a, b| format!("{} >= {}", s32(a), s32(b)))?,
            O::I64LtS => self.bin(I32, |a, b| format!("{} < {}", s64(a), s64(b)))?,
            O::I64GtS => self.bin(I32, |a, b| format!("{} > {}", s64(a), s64(b)))?,
            O::I64LeS => self.bin(I32, |a, b| format!("{} <= {}", s64(a), s64(b)))?,
            O::I64GeS => self.bin(I32, |a, b| format!("{} >= {}", s64(a), s64(b)))?,
            O::I32Clz => self.un(I32, |a| format!("{a} ? (uint32_t)__builtin_clz({a}) : 32u"))?,
            O::I32Ctz => self.un(I32, |a| format!("{a} ? (uint32_t)__builtin_ctz({a}) : 32u"))?,
            O::I32Popcnt => self.un(I32, |a| format!("(uint32_t)__builtin_popcount({a})"))?,
            O::I64Clz => self.un(I64, |a| {
                format!("{a} ? (uint64_t)__builtin_clzll({a}) : 64u")
            })?,
            O::I64Ctz => self.un(I64, |a| {
                format!("{a} ? (uint64_t)__builtin_ctzll({a}) : 64u")
            })?,
            O::I64Popcnt => self.un(I64, |a| format!("(uint64_t)__builtin_popcountll({a})"))?,
            O::I32Add => self.bin(I32, |a, b| format!("{a} + {b}"))?,
            O::I32Sub => self.bin(I32, |a, b| format!("{a} - {b}"))?,
            O::I32Mul => self.bin(I32, |a, b| format!("{a} * {b}"))?,
            wasmparser::Operator::I32DivS => {
                self.bin(I32, |a, b| format!("bs_div_s32(I, {a}, {b})"))?;
            }
            wasmparser::Operator::I32DivU => {
                self.bin(I32, |a, b| format!("bs_div_u32(I, {a}, {b})"))?;
            }
            O::I32RemS => self.bin(I32, |a, b| format!("bs_rem_s32(I, {a}, {b})"))?,
            O::I32RemU => self.bin(I32, |a, b| format!("bs_rem_u32(I, {a}, {b})"))?,
            O::I32And | O::I64And => self.same(|a, b| format!("{a} & {b}"))?,
            O::I32Or | O::I64Or => self.same(|a, b| format!("{a} | {b}"))?,
            O::I32Xor | O::I64Xor => self.same(|a, b| format!("{a} ^ {b}"))?,
            O::I32Shl => self.bin(I32, |a, b| format!("{a} << ({b} & 31)"))?,
            O::I32ShrU => self.bin(I32, |a, b| format!("{a} >> ({b} & 31)"))?,
            O::I32ShrS => self.bin(I32, |a, b| {
                format!("(uint32_t)((int32_t){a} >> ({b} & 31))")
            })?,
            O::I32Rotl => {
                self.bin(I32, |a, b| {
                    format!("({a} << ({b} & 31)) | ({a} >> ((32 - ({b} & 31)) & 31))")
                })?;
            }
            O::I32Rotr => {
                self.bin(I32, |a, b| {
                    format!("({a} >> ({b} & 31)) | ({a} << ((32 - ({b} & 31)) & 31))")
                })?;
            }
            O::I64Add => self.bin(I64, |a, b| format!("{a} + {b}"))?,
            O::I64Sub => self.bin(I64, |a, b| format!("{a} - {b}"))?,
            O::I64Mul => self.bin(I64, |a, b| format!("{a} * {b}"))?,
            O::I64DivS => self.bin(I64, |a, b| format!("bs_div_s64(I, {a}, {b})"))?,
            O::I64DivU => self.bin(I64, |a, b| format!("bs_div_u64(I, {a}, {b})"))?,
            O::I64RemS => self.bin(I64, |a, b| format!("bs_rem_s64(I, {a}, {b})"))?,
            O::I64RemU => self.bin(I64, |a, b| format!("bs_rem_u64(I, {a}, {b})"))?,
            O::I64Shl => self.bin(I64, |a, b| format!("{a} << ({b} & 63)"))?,
            O::I64ShrU => self.bin(I64, |a, b| format!("{a} >> ({b} & 63)"))?,
            O::I64ShrS => self.bin(I64, |a, b| {
                format!("(uint64_t)((int64_t){a} >> ({b} & 63))")
            })?,
            O::I64Rotl => {
                self.bin(I64, |a, b| {
                    format!("({a} << ({b} & 63)) | ({a} >> ((64 - ({b} & 63)) & 63))")
                })?;
            }
            O::I64Rotr => {
                self.bin(I64, |a, b| {
                    format!("({a} >> ({b} & 63)) | ({a} << ((64 - ({b} & 63)) & 63))")
                })?;
            }
            O::I32WrapI64 => self.un(I32, |a| format!("(uint32_t){a}"))?,
            O::I64ExtendI32U => self.un(I64, |a| format!("(uint64_t){a}"))?,
            O::I64ExtendI32S => self.un(I64, |a| format!("(uint64_t)(int64_t)(int32_t){a}"))?,
            O::I32Extend8S => self.un(I32, |a| format!("(uint32_t)(int32_t)(int8_t){a}"))?,
            O::I32Extend16S => self.un(I32, |a| format!("(uint32_t)(int32_t)(int16_t){a}"))?,
            O::I64Extend8S => self.un(I64, |a| format!("(uint64_t)(int64_t)(int8_t){a}"))?,
            O::I64Extend16S => self.un(I64, |a| format!("(uint64_t)(int64_t)(int16_t){a}"))?,
            O::I64Extend32S => self.un(I64, |a| format!("(uint64_t)(int64_t)(int32_t){a}"))?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    // A bitwise operator, whose result is the kind its operands are.
    fn same(&mut self, f: impl Fn(&str, &str) -> String) -> Result<(), String> {
        let t = *self.stack.last().ok_or("the stack is empty")?;
        self.bin(t, f)
    }

    fn float(&mut self, op: &Operator<'_>) -> Result<bool, String> {
        use Operator as O;
        use ValType::{F64, I32, I64};
        match op {
            O::F64Eq => self.bin(I32, |a, b| format!("{a} == {b}"))?,
            O::F64Ne => self.bin(I32, |a, b| format!("{a} != {b}"))?,
            O::F64Lt => self.bin(I32, |a, b| format!("{a} < {b}"))?,
            O::F64Gt => self.bin(I32, |a, b| format!("{a} > {b}"))?,
            O::F64Le => self.bin(I32, |a, b| format!("{a} <= {b}"))?,
            O::F64Ge => self.bin(I32, |a, b| format!("{a} >= {b}"))?,
            O::F64Abs => self.un(F64, |a| format!("__builtin_fabs({a})"))?,
            O::F64Neg => self.un(F64, |a| format!("-{a}"))?,
            O::F64Ceil => self.un(F64, |a| format!("bs_ceil({a})"))?,
            O::F64Floor => self.un(F64, |a| format!("bs_floor({a})"))?,
            O::F64Trunc => self.un(F64, |a| format!("bs_trunc({a})"))?,
            O::F64Nearest => self.un(F64, |a| format!("bs_nearest({a})"))?,
            O::F64Sqrt => self.un(F64, |a| format!("__builtin_sqrt({a})"))?,
            O::F64Add => self.bin(F64, |a, b| format!("{a} + {b}"))?,
            O::F64Sub => self.bin(F64, |a, b| format!("{a} - {b}"))?,
            O::F64Mul => self.bin(F64, |a, b| format!("{a} * {b}"))?,
            O::F64Div => self.bin(F64, |a, b| format!("{a} / {b}"))?,
            O::F64Min => self.bin(F64, |a, b| format!("bs_min({a}, {b})"))?,
            O::F64Max => self.bin(F64, |a, b| format!("bs_max({a}, {b})"))?,
            O::F64Copysign => self.bin(F64, |a, b| format!("__builtin_copysign({a}, {b})"))?,
            O::F64ConvertI32S => self.un(F64, |a| format!("(double)(int32_t){a}"))?,
            O::F64ConvertI32U => self.un(F64, |a| format!("(double){a}"))?,
            O::F64ConvertI64S => self.un(F64, |a| format!("(double)(int64_t){a}"))?,
            O::F64ConvertI64U => self.un(F64, |a| format!("(double){a}"))?,
            O::I64ReinterpretF64 => self.un(I64, |a| format!("bs_bits({a})"))?,
            O::F64ReinterpretI64 => self.un(F64, |a| format!("bs_f64({a})"))?,
            O::I64TruncSatF64S => self.un(I64, |a| format!("bs_trunc_sat_s64({a})"))?,
            O::I32TruncSatF64S => self.un(I32, |a| format!("bs_trunc_sat_s32({a})"))?,
            O::I64TruncF64S => self.un(I64, |a| format!("bs_trunc_s64(I, {a})"))?,
            O::I32TruncF64S => self.un(I32, |a| format!("bs_trunc_s32(I, {a})"))?,
            _ => return Ok(false),
        }
        Ok(true)
    }
}

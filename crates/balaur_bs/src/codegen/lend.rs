// An argument lent, as a host call, a built-in, a prelude function and a string
// compare take one: a local goes as it is, with no count up and down; anything
// else is held for the call and released after it.

use super::func::{FnGen, LVal, MemPlace, R};
use crate::ast::{BinOp, Expr, ExprKind, Part, Ty};
use crate::vocabulary::words;
use wasm_encoder::Instruction as I;

/// The expressions directly inside `e`; `None` for one holding statements,
/// which may assign a local or leave the function.
fn parts(e: &Expr) -> Option<Vec<&Expr>> {
    Some(match &e.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Nil
        | ExprKind::Var(_)
        | ExprKind::SelfRef
        | ExprKind::Resumed(..) => vec![],
        ExprKind::Interp(pieces) => pieces
            .iter()
            .filter_map(|piece| match piece {
                Part::Value(value) => Some(value),
                Part::Text(_) => None,
            })
            .collect(),
        ExprKind::Bin(_, a, b) | ExprKind::Index(a, b) | ExprKind::ArrayRepeat(a, b) => {
            vec![&**a, &**b]
        }
        ExprKind::Unary(_, a) | ExprKind::Field(a, _) | ExprKind::Cast(a, _) => vec![&**a],
        ExprKind::Call(_, args) | ExprKind::ArrayLit(args) => args.iter().collect(),
        ExprKind::Method(base, _, args) => std::iter::once(&**base).chain(args).collect(),
        ExprKind::StructLit(_, fields) => fields.iter().map(|(_, value)| value).collect(),
        ExprKind::MapLit(entries) => entries
            .iter()
            .flat_map(|(key, value)| [key, value])
            .collect(),
        ExprKind::If(..) | ExprKind::Block(_) | ExprKind::Match(..) | ExprKind::Await(_) => {
            return None;
        }
    })
}

fn mentions(e: &Expr, name: &str) -> bool {
    matches!(&e.kind, ExprKind::Var(n) if n == name)
        || parts(e).is_none_or(|inside| inside.iter().any(|part| mentions(part, name)))
}

fn holds_statements(e: &Expr) -> bool {
    parts(e).is_none_or(|inside| inside.iter().any(|part| holds_statements(part)))
}

/// Whether `e` names no call, so nothing it runs lets go of what a class or a
/// list holds.
fn calls_nothing(e: &Expr) -> bool {
    !matches!(e.kind, ExprKind::Call(..) | ExprKind::Method(..))
        && parts(e).is_some_and(|inside| inside.iter().all(|part| calls_nothing(part)))
}

fn root_variable(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::Var(name) => Some(name),
        ExprKind::Field(base, _) => root_variable(base),
        _ => None,
    }
}

/// Who the arguments of a call are lent to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Taker {
    /// The engine or a built-in, which keeps nothing: a `str?`, a struct and a
    /// class are lent as a string is.
    Engine,
    /// A prelude function or a map's method: it owns what is no string, list
    /// or `any`.
    Prelude,
    /// A built-in that runs no code, so a string may be read where it is held.
    Leaf,
}

impl FnGen<'_> {
    /// Whether a value of `ty` holds anything counted: a string in a struct or
    /// under a `T?` is one, though `ty` itself is no heap type.
    pub(crate) fn counted(&self, ty: &Ty) -> bool {
        self.g.slots(ty).iter().any(|(_, heap)| heap.is_some())
    }

    /// `args` on the stack as `want` says, each one `taker` borrows lent, and
    /// what to release once the call has returned.
    pub(crate) fn lent(&mut self, want: &[Ty], args: &[&Expr], taker: Taker) -> R<Vec<LVal>> {
        let mut held = vec![];
        for (at, (ty, arg)) in want.iter().zip(args).enumerate() {
            let rest = &args[at + 1..];
            let borrowed = ty.is_heap() || (taker == Taker::Engine && self.counted(ty));
            if borrowed && let Some(local) = self.local_to_lend(arg, ty, rest)? {
                self.get_raw(&local);
                continue;
            }
            if taker == Taker::Leaf
                && rest.iter().all(|later| calls_nothing(later))
                && let Some(place) = self.place_to_lend(arg)?
            {
                self.mem_get_raw(&place);
                held.extend(place.release);
                continue;
            }
            self.expr_as(arg, ty)?;
            // A literal string lives in the data segment; boxed into an
            // `any`, it is a fresh cell like any other.
            if borrowed && !(matches!(arg.kind, ExprKind::Str(_)) && *ty == Ty::Str) {
                // Only a later argument leaving the function needs the frame
                // to release it, and the frame's exit pays for every temp.
                let temp = if rest.iter().any(|later| holds_statements(later)) {
                    self.temp(ty)
                } else {
                    self.alloc(ty)
                };
                self.set_raw(&temp);
                self.get_raw(&temp);
                held.push(temp);
            }
        }
        Ok(held)
    }

    /// The owned `ty` on top of the stack, held for a call that borrows it:
    /// what to release once the call has returned.
    pub(crate) fn held_for_call(&mut self, ty: &Ty) -> Vec<LVal> {
        if !self.counted(ty) {
            return vec![];
        }
        let temp = self.alloc(ty);
        self.set_raw(&temp);
        self.get_raw(&temp);
        vec![temp]
    }

    pub(crate) fn release_all(&mut self, held: Vec<LVal>) {
        for temp in held {
            self.release(&temp);
        }
    }

    // The local `arg` reads, when it is a `ty` nothing before the call changes.
    fn local_to_lend(&mut self, arg: &Expr, ty: &Ty, rest: &[&Expr]) -> R<Option<LVal>> {
        let Some(root) = root_variable(arg).filter(|root| self.lookup(root).is_some()) else {
            return Ok(None);
        };
        let Some(local) = self.place(arg)?.filter(|local| local.ty == *ty) else {
            return Ok(None);
        };
        Ok((!rest.iter().any(|later| self.may_change(later, root))).then_some(local))
    }

    // An array's element or a class's field, where working out where it is
    // calls nothing.
    fn place_to_lend(&mut self, arg: &Expr) -> R<Option<MemPlace>> {
        let in_memory = match &arg.kind {
            ExprKind::Index(array, index) => {
                calls_nothing(array)
                    && calls_nothing(index)
                    && matches!(self.type_of(array), Ok(Ty::Array(_)))
            }
            ExprKind::Field(base, _) => {
                self.place(base)?
                    .is_some_and(|local| self.g.is_class(&local.ty).is_some())
                    && self.component_access(arg)?.is_none()
            }
            _ => false,
        };
        if in_memory {
            self.mem_place(arg)
        } else {
            Ok(None)
        }
    }

    // Whether working `e` out could assign local `name`: an assignment in a
    // block it holds, or a call that takes the local by `&mut`.
    fn may_change(&self, e: &Expr, name: &str) -> bool {
        let Some(inside) = parts(e) else {
            return true;
        };
        let handed = match &e.kind {
            ExprKind::Call(callee, args) => {
                !self.takes_no_ref(callee) && args.iter().any(|arg| mentions(arg, name))
            }
            ExprKind::Method(base, method, args) => {
                method != words::methods::LEN
                    && (mentions(base, name) || args.iter().any(|arg| mentions(arg, name)))
            }
            _ => false,
        };
        handed || inside.iter().any(|part| self.may_change(part, name))
    }

    fn takes_no_ref(&self, callee: &str) -> bool {
        self.lookup(callee).is_none()
            && (callee.starts_with("__")
                || self.g.sigs.get(callee).is_some_and(|sig| !sig.has_refs()))
    }

    /// `a == b` on two strings: lent, and compared by the helper that takes
    /// them as they are.
    pub(crate) fn str_compare(&mut self, op: BinOp, a: &Expr, b: &Expr) -> R<Ty> {
        let held = self.lent(&[Ty::Str, Ty::Str], &[a, b], Taker::Leaf)?;
        Ok(self.compare_lent(op, held))
    }

    /// The two strings on the stack, `a` under `b`, owned: held for the compare.
    pub(crate) fn str_compare_owned(&mut self, op: BinOp) -> Ty {
        let (a, b) = (self.alloc(&Ty::Str), self.alloc(&Ty::Str));
        self.set_raw(&b);
        self.set_raw(&a);
        self.get_raw(&a);
        self.get_raw(&b);
        self.compare_lent(op, vec![a, b])
    }

    fn compare_lent(&mut self, op: BinOp, held: Vec<LVal>) -> Ty {
        self.e(I::Call(self.g.h.str_same));
        self.release_all(held);
        if op == BinOp::Ne {
            self.e(I::I32Eqz);
        }
        Ty::Bool
    }
}

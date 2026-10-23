//! What the emitter knows of a value's type, which is little on purpose: an
//! int, a float, a bool, a string or a node where a hint or a literal says
//! so, and an `any` everywhere else. A vector, an array and a dictionary are
//! `any` too, so Godot's arithmetic on them goes through the shim as one.

use super::Emitter;
use crate::godot::gdscript::ast::Expr;

/// A value's type as the converted script declares it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bt {
    Int,
    Float,
    Bool,
    Str,
    Node,
    Any,
}

impl Bt {
    /// The type a GDScript hint names, `any` for one this does not keep.
    pub(crate) fn of_hint(hint: &str) -> Bt {
        match hint {
            "int" => Bt::Int,
            "float" => Bt::Float,
            "bool" => Bt::Bool,
            "String" | "StringName" | "NodePath" => Bt::Str,
            // A node-typed variable may hold null, which a `Node` cannot.
            _ => Bt::Any,
        }
    }

    /// The type a literal has where it stands.
    pub(crate) fn of_literal(value: &Expr) -> Bt {
        match value {
            Expr::Int(_) => Bt::Int,
            Expr::Float(_) => Bt::Float,
            Expr::Bool(_) => Bt::Bool,
            Expr::Str(_) => Bt::Str,
            _ => Bt::Any,
        }
    }

    /// How a declaration spells it.
    pub(crate) fn text(self) -> &'static str {
        match self {
            Bt::Int => "int",
            Bt::Float => "float",
            Bt::Bool => "bool",
            Bt::Str => "str",
            Bt::Node => "Node",
            Bt::Any => "any",
        }
    }

    pub(crate) fn numeric(self) -> bool {
        matches!(self, Bt::Int | Bt::Float)
    }
}

impl Emitter<'_> {
    /// What an expression's type is known to be, without emitting it.
    pub(crate) fn type_of(&self, value: &Expr) -> Bt {
        match value {
            Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::Str(_) => Bt::of_literal(value),
            Expr::Name(name) if self.is_local(name) => {
                self.local_types.get(name).copied().unwrap_or(Bt::Any)
            }
            Expr::Name(name) => self.member_type(name),
            Expr::Field(object, name) if matches!(**object, Expr::SelfRef) => {
                self.member_type(name)
            }
            Expr::SelfRef if !self.context.object_class => Bt::Node,
            Expr::NodePath(_) | Expr::Unique(_) => Bt::Node,
            Expr::Unary("!", _) | Expr::Is(..) => Bt::Bool,
            Expr::Unary("-", inner) => {
                let t = self.type_of(inner);
                if t.numeric() { t } else { Bt::Any }
            }
            Expr::Binary(op, left, right) => {
                let (l, r) = (self.type_of(left), self.type_of(right));
                match *op {
                    "==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||" | "in" => Bt::Bool,
                    "+" | "-" | "*" if l.numeric() && r.numeric() => {
                        if l == Bt::Int && r == Bt::Int { Bt::Int } else { Bt::Float }
                    }
                    "/" if l.numeric() && r.numeric() => {
                        if l == Bt::Int && r == Bt::Int { Bt::Int } else { Bt::Float }
                    }
                    "+" if l == Bt::Str && r == Bt::Str => Bt::Str,
                    _ => Bt::Any,
                }
            }
            Expr::Ternary { then, other, .. } => {
                let (a, b) = (self.type_of(then), self.type_of(other));
                if a == b { a } else { Bt::Any }
            }
            Expr::Call(callee, _) => self.call_type(callee),
            Expr::Cast(_, name) => match Bt::of_hint(name) {
                Bt::Node => Bt::Node,
                _ => Bt::Any,
            },
            _ => Bt::Any,
        }
    }

    fn member_type(&self, name: &str) -> Bt {
        self.context
            .member_types
            .get(name)
            .copied()
            .unwrap_or(Bt::Any)
    }

    // A call's result: an own method's declared one, a few globals', any
    // other's unknown.
    fn call_type(&self, callee: &Expr) -> Bt {
        let own = match callee {
            Expr::Name(name) if !self.is_local(name) => Some(name.as_str()),
            Expr::Field(object, name) if matches!(**object, Expr::SelfRef) => Some(name.as_str()),
            _ => None,
        };
        if let Some(name) = own {
            if let Some(t) = self.context.returns.get(name) {
                return *t;
            }
            return match name {
                "str" | "char" => Bt::Str,
                "int" | "len" => Bt::Int,
                "float" | "sqrt" | "sin" | "cos" | "atan2" | "deg_to_rad" | "rad_to_deg" => {
                    Bt::Float
                }
                _ => Bt::Any,
            };
        }
        Bt::Any
    }
}

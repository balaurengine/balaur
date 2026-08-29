//! The neutral value type crossing the engine/script boundary.

use balaur_core::hecs::Entity;

/// A value a script can pass or receive.
///
/// Deliberately small and closed: every backend must represent all of it, and
/// a closed set means a type confusion is a caught error rather than silent
/// corruption. Vectors and colours are variants rather than lists because
/// they are the hot types and boxing them is what makes scripting slow.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Color([f32; 4]),
    Node(Entity),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
}

impl Value {
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Nil => "nil",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Num(_) => "number",
            Self::Str(_) => "string",
            Self::Vec2(_) => "vec2",
            Self::Vec3(_) => "vec3",
            Self::Color(_) => "color",
            Self::Node(_) => "node",
            Self::List(_) => "list",
            Self::Map(_) => "map",
        }
    }
}

/// Convert script arguments into a Rust type.
pub trait FromArgs: Sized {
    fn from_args(args: &[Value]) -> anyhow::Result<Self>;
}

/// Convert a Rust value into something a script can hold.
pub trait IntoValue {
    fn into_value(self) -> Value;
}

macro_rules! from_args_scalar {
    ($t:ty, $variant:ident, $conv:expr) => {
        impl FromArgs for $t {
            fn from_args(args: &[Value]) -> anyhow::Result<Self> {
                match args.first() {
                    Some(Value::$variant(v)) => Ok($conv(v)),
                    Some(other) => {
                        anyhow::bail!(
                            "expected {}, got {}",
                            stringify!($variant),
                            other.type_name()
                        )
                    }
                    None => anyhow::bail!("expected {}, got no argument", stringify!($variant)),
                }
            }
        }
    };
}

from_args_scalar!(bool, Bool, |v: &bool| *v);
from_args_scalar!(i64, Int, |v: &i64| *v);
from_args_scalar!(f64, Num, |v: &f64| *v);
from_args_scalar!(String, Str, std::clone::Clone::clone);

impl FromArgs for () {
    fn from_args(_: &[Value]) -> anyhow::Result<Self> {
        Ok(())
    }
}

impl IntoValue for () {
    fn into_value(self) -> Value {
        Value::Nil
    }
}
impl IntoValue for bool {
    fn into_value(self) -> Value {
        Value::Bool(self)
    }
}
impl IntoValue for i64 {
    fn into_value(self) -> Value {
        Value::Int(self)
    }
}
impl IntoValue for f64 {
    fn into_value(self) -> Value {
        Value::Num(self)
    }
}
impl IntoValue for f32 {
    fn into_value(self) -> Value {
        Value::Num(f64::from(self))
    }
}
impl IntoValue for String {
    fn into_value(self) -> Value {
        Value::Str(self)
    }
}
impl IntoValue for Value {
    fn into_value(self) -> Value {
        self
    }
}

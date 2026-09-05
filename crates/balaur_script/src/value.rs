//! The neutral value type crossing the engine/script boundary.

use anyhow::{Result, bail};

/// A value a script can pass or receive.
///
/// Deliberately small and closed: every backend must represent all of it, and
/// a closed set means a type confusion is a caught error rather than silent
/// corruption. Vectors and colours are variants rather than lists because they
/// are the hot types and boxing them is what makes scripting slow.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
    /// A binary payload: a websocket frame, and later a datagram. Separate
    /// from `Str` because a frame's bytes need not be UTF-8.
    Bytes(Vec<u8>),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Color([f32; 4]),
    /// A node, as `hecs::Entity::to_bits()`. Kept opaque so this crate
    /// depends on nothing.
    Node(u64),
    /// A script function, valid only for the duration of the binding call it
    /// was passed to. Immediate-mode UI callbacks never outlive their call, so
    /// the backend can register on entry and drop on exit -- no ownership
    /// question, no interaction with the collector.
    Callback(CallbackId),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
    /// Several return values, not a list of one.
    ///
    /// Lua and Rune both let a function return more than one thing, and
    /// `local text, changed = ui.text_field(...)` is how the widgets are meant
    /// to read. A tuple return becomes this; a `Vec` still becomes a `List`.
    Many(Vec<Value>),
}

impl Value {
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Nil => "nil",
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Num(_) => "number",
            Self::Str(_) => "string",
            Self::Bytes(_) => "bytes",
            Self::Vec2(_) => "vec2",
            Self::Vec3(_) => "vec3",
            Self::Color(_) => "color",
            Self::Node(_) => "node",
            Self::Callback(_) => "function",
            Self::List(_) => "list",
            Self::Map(_) => "map",
            Self::Many(_) => "several values",
        }
    }
}

/// One positional argument.
pub trait FromArg: Sized {
    fn from_arg(v: Option<&Value>) -> Result<Self>;
}

/// A whole argument list. Implemented for scalars (one argument) and for
/// tuples (one argument each, positionally).
pub trait FromArgs: Sized {
    fn from_args(args: &[Value]) -> Result<Self>;
}

/// A Rust value a script can hold.
pub trait IntoValue {
    fn into_value(self) -> Value;
}

fn want(v: Option<&Value>, expected: &str) -> anyhow::Error {
    v.map_or_else(
        || anyhow::anyhow!("expected {expected}, got no argument"),
        |v| anyhow::anyhow!("expected {expected}, got {}", v.type_name()),
    )
}

macro_rules! num_arg {
    ($($t:ty),*) => { $(
        impl FromArg for $t {
            fn from_arg(v: Option<&Value>) -> Result<Self> {
                match v {
                    Some(Value::Num(n)) => Ok(*n as Self),
                    Some(Value::Int(i)) => Ok(*i as Self),
                    other => Err(want(other, "number")),
                }
            }
        }
        impl IntoValue for $t {
            #[allow(clippy::cast_lossless, reason = "f32 to f64 widens; f64 is identity")]
            fn into_value(self) -> Value { Value::Num(self as f64) }
        }
    )* };
}
num_arg!(f32, f64);

macro_rules! int_arg {
    ($($t:ty),*) => { $(
        impl FromArg for $t {
            fn from_arg(v: Option<&Value>) -> Result<Self> {
                match v {
                    Some(Value::Int(i)) => Self::try_from(*i)
                        .map_err(|_| anyhow::anyhow!("{i} is out of range for {}", stringify!($t))),
                    Some(Value::Num(n)) => Ok(*n as Self),
                    other => Err(want(other, "integer")),
                }
            }
        }
        impl IntoValue for $t {
            // The neutral integer is i64; u64/usize past i64::MAX are not a
            // value a script can hold anyway.
            #[allow(clippy::cast_lossless, clippy::cast_possible_wrap)]
            fn into_value(self) -> Value { Value::Int(self as i64) }
        }
    )* };
}
int_arg!(i32, i64, u32, u64, usize);

impl FromArg for bool {
    fn from_arg(v: Option<&Value>) -> Result<Self> {
        match v {
            Some(Value::Bool(b)) => Ok(*b),
            other => Err(want(other, "bool")),
        }
    }
}

impl FromArg for String {
    fn from_arg(v: Option<&Value>) -> Result<Self> {
        match v {
            Some(Value::Str(s)) => Ok(s.clone()),
            other => Err(want(other, "string")),
        }
    }
}

/// A call-scoped script function, resolved by the backend that registered it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub struct CallbackId(pub u64);

impl FromArg for CallbackId {
    fn from_arg(v: Option<&Value>) -> Result<Self> {
        match v {
            Some(Value::Callback(id)) => Ok(*id),
            other => Err(want(other, "function")),
        }
    }
}

/// A node handle, opaque here and re-hydrated by the backend.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(pub u64);

impl FromArg for NodeId {
    fn from_arg(v: Option<&Value>) -> Result<Self> {
        match v {
            Some(Value::Node(bits)) => Ok(Self(*bits)),
            other => Err(want(other, "node")),
        }
    }
}

impl FromArg for Value {
    fn from_arg(v: Option<&Value>) -> Result<Self> {
        Ok(v.cloned().unwrap_or(Value::Nil))
    }
}

/// A missing or nil argument is `None`; anything else must convert.
impl<T: FromArg> FromArg for Option<T> {
    fn from_arg(v: Option<&Value>) -> Result<Self> {
        match v {
            None | Some(Value::Nil) => Ok(None),
            some => T::from_arg(some).map(Some),
        }
    }
}

impl IntoValue for bool {
    fn into_value(self) -> Value {
        Value::Bool(self)
    }
}
impl IntoValue for String {
    fn into_value(self) -> Value {
        Value::Str(self)
    }
}
impl IntoValue for &str {
    fn into_value(self) -> Value {
        Value::Str(self.to_string())
    }
}
impl IntoValue for NodeId {
    fn into_value(self) -> Value {
        Value::Node(self.0)
    }
}
impl IntoValue for Value {
    fn into_value(self) -> Value {
        self
    }
}
impl IntoValue for () {
    fn into_value(self) -> Value {
        Value::Nil
    }
}
impl<T: IntoValue> IntoValue for Option<T> {
    fn into_value(self) -> Value {
        self.map_or(Value::Nil, IntoValue::into_value)
    }
}
impl<T: IntoValue> IntoValue for Vec<T> {
    fn into_value(self) -> Value {
        Value::List(self.into_iter().map(IntoValue::into_value).collect())
    }
}

impl FromArgs for () {
    fn from_args(_: &[Value]) -> Result<Self> {
        Ok(())
    }
}

/// A single scalar consumes one argument.
macro_rules! scalar_args {
    ($($t:ty),*) => { $(
        impl FromArgs for $t {
            fn from_args(args: &[Value]) -> Result<Self> { Self::from_arg(args.first()) }
        }
    )* };
}
scalar_args!(
    bool, i32, i64, u32, u64, usize, f32, f64, String, NodeId, CallbackId, Value
);

impl<T: FromArg> FromArgs for Option<T> {
    fn from_args(args: &[Value]) -> Result<Self> {
        Self::from_arg(args.first())
    }
}

/// Tuples map positionally: element `n` reads argument `n`.
macro_rules! tuple_args {
    ($($n:tt $name:ident),+) => {
        impl<$($name: FromArg),+> FromArgs for ($($name,)+) {
            fn from_args(args: &[Value]) -> Result<Self> {
                Ok(($($name::from_arg(args.get($n))?,)+))
            }
        }
        impl<$($name: IntoValue),+> IntoValue for ($($name,)+) {
            #[allow(non_snake_case, reason = "the macro binds the type-parameter idents")]
            fn into_value(self) -> Value {
                let ($($name,)+) = self;
                Value::Many(vec![$($name.into_value()),+])
            }
        }
    };
}
tuple_args!(0 A);
tuple_args!(0 A, 1 B);
tuple_args!(0 A, 1 B, 2 C);
tuple_args!(0 A, 1 B, 2 C, 3 D);
tuple_args!(0 A, 1 B, 2 C, 3 D, 4 E);
tuple_args!(0 A, 1 B, 2 C, 3 D, 4 E, 5 F);
tuple_args!(0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G);
tuple_args!(0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G, 7 H);
tuple_args!(0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G, 7 H, 8 I);
tuple_args!(0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G, 7 H, 8 I, 9 J);
tuple_args!(0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G, 7 H, 8 I, 9 J, 10 K);
tuple_args!(0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G, 7 H, 8 I, 9 J, 10 K, 11 L);

/// A wrong argument count is a bug in the binding, not in the script.
pub fn expect_arity(args: &[Value], n: usize, name: &str) -> Result<()> {
    if args.len() == n {
        Ok(())
    } else {
        bail!("{name} takes {n} arguments, got {}", args.len())
    }
}

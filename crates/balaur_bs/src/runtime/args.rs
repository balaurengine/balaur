//! What an engine function is handed: the call's arguments where the module
//! left them. A number, a flag or a node is read from its slot when asked
//! for. A string or a value of another kind is read out of the module's memory
//! before the call, once: the engine function may call back into the module,
//! which changes that memory.

use super::engine::{ImportCtx, Raw};
use super::host::CtxReader;
use super::values::{Reader, Value, decode, float, rd32, width};
use crate::{Meta, Ty};
use smallvec::SmallVec;
use std::borrow::Cow;

/// A call's arguments, each read as the engine function asks for it: `None`
/// for one not of that kind, which [`CallArgs::value`] reads whatever it is.
pub trait CallArgs {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn int(&self, index: usize) -> Option<i64>;
    fn float(&self, index: usize) -> Option<f64>;
    fn bool(&self, index: usize) -> Option<bool>;
    fn node(&self, index: usize) -> Option<i64>;
    fn text(&self, index: usize) -> Option<&str>;
    /// Missing, or nil.
    fn is_nil(&self, index: usize) -> bool;
    /// The argument whatever its kind; `Value::Void` past the last. Lent
    /// where it was read already, so converting it copies nothing twice.
    fn value(&self, index: usize) -> Cow<'_, Value>;
    /// One of the engine's vectors, read from its slots: its numbers, and how
    /// many. `None` for anything else, which [`CallArgs::value`] reads.
    fn vector(&self, index: usize) -> Option<([f32; 4], usize)> {
        let _ = index;
        None
    }
}

/// Values already read, as an `extern fn` or a test has them.
pub struct Read<'a>(pub &'a [Value]);

impl CallArgs for Read<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn int(&self, index: usize) -> Option<i64> {
        match self.0.get(index) {
            Some(Value::Int(number)) => Some(*number),
            _ => None,
        }
    }
    fn float(&self, index: usize) -> Option<f64> {
        match self.0.get(index) {
            Some(Value::Float(number)) => Some(*number),
            _ => None,
        }
    }
    fn bool(&self, index: usize) -> Option<bool> {
        match self.0.get(index) {
            Some(Value::Bool(flag)) => Some(*flag),
            _ => None,
        }
    }
    fn node(&self, index: usize) -> Option<i64> {
        match self.0.get(index) {
            Some(Value::Node(id)) => Some(*id),
            _ => None,
        }
    }
    fn text(&self, index: usize) -> Option<&str> {
        match self.0.get(index) {
            Some(Value::Str(text)) => Some(text),
            _ => None,
        }
    }
    fn is_nil(&self, index: usize) -> bool {
        matches!(self.0.get(index), None | Some(Value::Void))
    }
    fn value(&self, index: usize) -> Cow<'_, Value> {
        self.0
            .get(index)
            .map_or(Cow::Owned(Value::Void), Cow::Borrowed)
    }
}

/// What an import takes, worked out once: each parameter's type, where it
/// starts in the slots, with one more for where the last ends, how many floats
/// it is when one of the engine's vectors, and which live in memory.
pub(crate) struct Params {
    pub(crate) tys: Vec<Ty>,
    at: Vec<usize>,
    vectors: Vec<u8>,
    in_memory: Vec<usize>,
}

impl Params {
    pub(crate) fn of(meta: &Meta, tys: &[Ty]) -> Self {
        Self {
            tys: tys.to_vec(),
            at: offsets(meta, tys),
            vectors: tys.iter().map(vector_width).collect(),
            in_memory: (0..tys.len()).filter(|at| in_memory(&tys[*at])).collect(),
        }
    }
}

/// An import's arguments in its slots.
pub(crate) struct Slots<'a> {
    meta: &'a Meta,
    params: &'a Params,
    raw: &'a [Raw],
    /// Each argument that lives in memory, read before the call, by its place.
    held: SmallVec<[(usize, Value); 2]>,
}

impl<'a> Slots<'a> {
    pub(crate) fn read(
        meta: &'a Meta,
        ctx: &dyn ImportCtx,
        params: &'a Params,
        raw: &'a [Raw],
    ) -> Self {
        let mut held = SmallVec::new();
        for index in &params.in_memory {
            let (index, ty) = (*index, &params.tys[*index]);
            let slots = &raw[params.at[index]..params.at[index + 1]];
            let value = match ctx.memory_bytes() {
                Some(memory) => read_held(meta, memory, ty, slots),
                None => read_held(meta, &CtxReader(ctx), ty, slots),
            };
            held.push((index, value));
        }
        Slots {
            meta,
            params,
            raw,
            held,
        }
    }

    fn held(&self, index: usize) -> Option<&Value> {
        self.held
            .iter()
            .find(|(place, _)| *place == index)
            .map(|(_, value)| value)
    }

    /// Every argument whole, what was read moved out rather than copied.
    pub(crate) fn into_values(mut self) -> Vec<Value> {
        (0..self.params.tys.len())
            .map(
                |index| match self.held.iter_mut().find(|(place, _)| *place == index) {
                    Some((_, value)) => std::mem::replace(value, Value::Void),
                    None => self.value(index).into_owned(),
                },
            )
            .collect()
    }

    fn slot(&self, index: usize) -> Option<Raw> {
        self.params
            .at
            .get(index)
            .and_then(|start| self.raw.get(*start))
            .copied()
    }
}

thread_local! {
    /// The buffers of strings calls were handed, for the next call's: a script
    /// names an action or a node by a string, frame after frame.
    static SPARE: std::cell::RefCell<Vec<Vec<u8>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The most buffers kept, and the longest: past those, a call's own are freed.
const SPARE_COUNT: usize = 16;
const SPARE_LENGTH: usize = 4096;

fn read_held(meta: &Meta, memory: &(impl Reader + ?Sized), ty: &Ty, slots: &[Raw]) -> Value {
    if matches!(ty, Ty::Str) {
        return spare_str(memory, slots[0].i32() as u32 as usize);
    }
    decode(meta, memory, ty, slots)
}

fn spare_str(memory: &(impl Reader + ?Sized), at: usize) -> Value {
    if at == 0 {
        return Value::Str(String::new());
    }
    let length = rd32(memory, at + 4) as usize;
    let mut bytes = SPARE
        .with(|spare| spare.borrow_mut().pop())
        .unwrap_or_default();
    bytes.clear();
    bytes.resize(length, 0);
    memory.fill(at + 8, &mut bytes);
    Value::Str(
        String::from_utf8(bytes)
            .unwrap_or_else(|bad| String::from_utf8_lossy(bad.as_bytes()).into_owned()),
    )
}

impl Drop for Slots<'_> {
    fn drop(&mut self) {
        for (_, value) in &mut self.held {
            if let Value::Str(text) = std::mem::replace(value, Value::Void)
                && (1..=SPARE_LENGTH).contains(&text.capacity())
            {
                SPARE.with(|spare| {
                    let mut spare = spare.borrow_mut();
                    if spare.len() < SPARE_COUNT {
                        spare.push(text.into_bytes());
                    }
                });
            }
        }
    }
}

/// Where each of `tys` starts in the slots, then where the last ends.
pub(crate) fn offsets(meta: &Meta, tys: &[Ty]) -> Vec<usize> {
    let mut at = Vec::with_capacity(tys.len() + 1);
    let mut start = 0;
    for ty in tys {
        at.push(start);
        start += width(meta, ty);
    }
    at.push(start);
    at
}

// Whether a value of `ty` is more than its slots say.
fn in_memory(ty: &Ty) -> bool {
    if vector_width(ty) > 0 {
        return false;
    }
    !matches!(
        ty,
        Ty::Int | Ty::Float | Ty::Bool | Ty::Node | Ty::Fn(..) | Ty::Void
    )
}

/// How many floats one of the engine's vectors is, 0 for any other type. All
/// its fields sit in the call's own slots, so a binding reads them with nothing
/// to allocate.
pub(crate) fn vector_width(ty: &Ty) -> u8 {
    let Ty::Struct(name) = ty else {
        return 0;
    };
    match name.as_str() {
        "Vec2" => 2,
        "Vec3" => 3,
        "Color" => 4,
        _ => 0,
    }
}

fn bits(raw: Raw) -> i64 {
    match raw {
        Raw::I32(number) => i64::from(number),
        Raw::I64(number) => number,
        Raw::F64(number) => number.to_bits().cast_signed(),
    }
}

impl CallArgs for Slots<'_> {
    fn len(&self) -> usize {
        self.params.tys.len()
    }
    fn int(&self, index: usize) -> Option<i64> {
        matches!(self.params.tys.get(index), Some(Ty::Int)).then(|| self.slot(index).map(bits))?
    }
    fn float(&self, index: usize) -> Option<f64> {
        if !matches!(self.params.tys.get(index), Some(Ty::Float)) {
            return None;
        }
        self.slot(index).map(|raw| match raw {
            Raw::F64(number) => number,
            other => float(bits(other).cast_unsigned()),
        })
    }
    fn bool(&self, index: usize) -> Option<bool> {
        matches!(self.params.tys.get(index), Some(Ty::Bool))
            .then(|| self.slot(index).map(|raw| raw.i32() != 0))?
    }
    fn node(&self, index: usize) -> Option<i64> {
        matches!(self.params.tys.get(index), Some(Ty::Node)).then(|| self.slot(index).map(bits))?
    }
    fn text(&self, index: usize) -> Option<&str> {
        match self.held(index) {
            Some(Value::Str(text)) => Some(text),
            _ => None,
        }
    }
    fn is_nil(&self, index: usize) -> bool {
        index >= self.params.tys.len() || matches!(self.held(index), Some(Value::Void))
    }
    fn value(&self, index: usize) -> Cow<'_, Value> {
        if let Some(value) = self.held(index) {
            return Cow::Borrowed(value);
        }
        let Some(raw) = self.slot(index) else {
            return Cow::Owned(Value::Void);
        };
        Cow::Owned(match &self.params.tys[index] {
            Ty::Int | Ty::Fn(..) => Value::Int(bits(raw)),
            Ty::Node => Value::Node(bits(raw)),
            Ty::Float => Value::Float(self.float(index).unwrap_or(0.0)),
            Ty::Bool => Value::Bool(raw.i32() != 0),
            ty @ Ty::Struct(_) => {
                let at = &self.params.at;
                decode(self.meta, &[][..], ty, &self.raw[at[index]..at[index + 1]])
            }
            _ => Value::Void,
        })
    }
    fn vector(&self, index: usize) -> Option<([f32; 4], usize)> {
        let width = usize::from(*self.params.vectors.get(index)?);
        if width == 0 {
            return None;
        }
        let start = *self.params.at.get(index)?;
        let mut out = [0.0; 4];
        for (slot, raw) in out.iter_mut().zip(self.raw.get(start..start + width)?) {
            *slot = match raw {
                Raw::F64(number) => *number,
                other => float(bits(*other).cast_unsigned()),
            } as f32;
        }
        Some((out, width))
    }
}

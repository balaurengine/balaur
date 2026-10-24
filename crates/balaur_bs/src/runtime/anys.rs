// The methods of an `any`, answered by walking its cell in the module's
// memory. A child handed back is shared, not copied: it gains a reference.

use super::engine::{ImportCtx, Raw};
use super::host::{CtxReader, alloc, encode_owned};
use super::values::{Value, float, rd32, rd64, read_any, read_str};
use crate::anys::{BOOL, FLOAT, INT, LIST, MAP, NODE, STR};
use crate::{Meta, Ty, words};

fn retain(ctx: &mut dyn ImportCtx, p: u32) {
    if p == 0 {
        return;
    }
    let rc = rd32(&CtxReader(&*ctx), p as usize) as i32;
    if rc > 0 {
        ctx.write(p as usize, &(rc + 1).to_le_bytes());
    }
}

/// An array's length and where its elements start.
fn array(ctx: &dyn ImportCtx, p: u32) -> (u32, u32) {
    if p == 0 {
        return (0, 0);
    }
    let m = CtxReader(ctx);
    (rd32(&m, p as usize + 4), rd32(&m, p as usize + 12))
}

fn kind_name(tag: i32) -> &'static str {
    match tag {
        BOOL => "bool",
        INT => "int",
        FLOAT => "float",
        STR => "str",
        LIST => "list",
        MAP => "map",
        NODE => "node",
        _ => "nil",
    }
}

/// The value in the map at `p` under `key`, as its cell's address.
fn lookup(ctx: &dyn ImportCtx, p: u32, key: &str) -> Option<u32> {
    let m = CtxReader(ctx);
    let (keys, values) = (rd32(&m, p as usize + 8), rd32(&m, p as usize + 12));
    let (len, kd) = array(ctx, keys);
    let (_, vd) = array(ctx, values);
    (0..len).find_map(|j| {
        let k = rd32(&m, (kd + 8 * j) as usize) as usize;
        (read_str(&m, k) == Value::Str(key.into())).then(|| rd32(&m, (vd + 8 * j) as usize))
    })
}

// `equals` and `sorted`: the `any` at `p` read whole, and compared.
fn compare(
    ctx: &mut dyn ImportCtx,
    meta: &Meta,
    name: &str,
    p: u32,
    args: &[Raw],
) -> Result<Vec<Raw>, String> {
    let m = CtxReader(&*ctx);
    let this = read_any(meta, &m, p as usize);
    if name == "equals" {
        let other = read_any(meta, &m, args[1].i32() as u32 as usize);
        return Ok(vec![Raw::I32(i32::from(same(&this, &other)))]);
    }
    let mut items = match this {
        Value::Array(items) => items,
        _ => vec![],
    };
    items.sort_by(order);
    encode_owned(ctx, meta, &Ty::Any, &Value::Array(items)).map(smallvec::SmallVec::into_vec)
}

// Whether two values are equal as values: numbers by number whatever their
// kind, maps by key whatever their order, and nil only to nil.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Float(y)) | (Value::Float(y), Value::Int(x)) => {
            (*x as f64).total_cmp(y).is_eq()
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b))
        }
        (Value::Map(x), Value::Map(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.iter().any(|(j, w)| k == j && same(v, w)))
        }
        _ => a == b,
    }
}

// How `sorted` orders two values: numbers by value, strings by text, and
// values of different kinds by kind.
fn order(a: &Value, b: &Value) -> std::cmp::Ordering {
    let number = |v: &Value| match v {
        Value::Int(x) => Some(*x as f64),
        Value::Float(x) => Some(*x),
        _ => None,
    };
    match (number(a), number(b), a, b) {
        (Some(x), Some(y), ..) => x.total_cmp(&y),
        (_, _, Value::Str(x), Value::Str(y)) => x.cmp(y),
        _ => quoted(a).cmp(&quoted(b)),
    }
}

/// What `.text()` says: a string as it is, anything else as `quoted` writes it.
pub(crate) fn text(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        v => quoted(v),
    }
}

// A value written out, the strings inside it in quotes.
fn quoted(v: &Value) -> String {
    match v {
        Value::Void => "nil".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(x) => x.to_string(),
        Value::Float(x) => format!("{x:?}"),
        Value::Node(x) => format!("node#{x}"),
        Value::Str(s) => format!("{s:?}"),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(quoted).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Map(entries) | Value::Struct(_, entries) => {
            let parts: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("{k:?}: {}", quoted(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

fn word_at(ctx: &dyn ImportCtx, at: u32) -> u32 {
    rd32(&CtxReader(ctx), at as usize)
}

fn put(ctx: &mut dyn ImportCtx, at: u32, v: u32) {
    ctx.write(at as usize, &v.to_le_bytes());
}

/// The array the cell at `cell` points to at `off`, made empty if it points
/// at none yet (a `map()` has neither of its arrays).
fn array_of(ctx: &mut dyn ImportCtx, cell: u32, off: u32) -> Result<u32, String> {
    let arr = word_at(ctx, cell + off);
    if arr != 0 {
        return Ok(arr);
    }
    let arr = alloc(ctx, 16)? as u32;
    put(ctx, arr, 1);
    put(ctx, cell + off, arr);
    Ok(arr)
}

/// One more 8-byte slot at the end of the array at `arr`, holding `v`. Full,
/// it doubles its capacity the way a script's own `push` does, the elements
/// moving to the new storage with their references.
fn append(ctx: &mut dyn ImportCtx, arr: u32, v: u32) -> Result<(), String> {
    let (len, mut data) = array(ctx, arr);
    let cap = word_at(ctx, arr + 8);
    if len == cap {
        let room = (cap * 2).max(4);
        let grown = alloc(ctx, 8 * room as usize)? as u32;
        if data != 0 {
            let old = ctx.read(data as usize, 8 * len as usize);
            ctx.write(grown as usize, &old);
            ctx.call(words::FREE, &[Raw::I32(data as i32)], 0)?;
        }
        put(ctx, arr + 8, room);
        put(ctx, arr + 12, grown);
        data = grown;
    }
    ctx.write((data + 8 * len) as usize, &u64::from(v).to_le_bytes());
    put(ctx, arr + 4, len + 1);
    Ok(())
}

// Drops one reference to the `any` at `p`, through the module's own release.
fn release_any(ctx: &mut dyn ImportCtx, p: u32) -> Result<(), String> {
    if p == 0 {
        return Ok(());
    }
    ctx.call(&format!("{}any", words::DEC), &[Raw::I32(p as i32)], 0)
        .map(|_| ())
}

// The element at `j` out of the array at `arr`, the rest moved down over it;
// the reference it held goes to the caller.
fn take_at(ctx: &mut dyn ImportCtx, arr: u32, j: u32) -> u32 {
    let (len, data) = array(ctx, arr);
    let out = word_at(ctx, data + 8 * j);
    for i in j..len - 1 {
        let next = word_at(ctx, data + 8 * (i + 1));
        put(ctx, data + 8 * i, next);
    }
    put(ctx, arr + 4, len - 1);
    out
}

// `any.remove(key)` in place: a map's entry or a list's element, handed back.
fn remove_in(ctx: &mut dyn ImportCtx, (p, tag): (u32, i32), key: &Value) -> Result<u32, String> {
    use crate::anys::{LIST, MAP};
    if tag == LIST {
        let list = word_at(ctx, p + 8);
        let len = array(ctx, list).0;
        let found = match key {
            Value::Int(i) => u32::try_from(*i).ok().filter(|i| *i < len),
            _ => None,
        };
        return Ok(found.map_or(0, |j| take_at(ctx, list, j)));
    }
    if tag != MAP {
        return Ok(0);
    }
    let Value::Str(key) = key else { return Ok(0) };
    let keys = word_at(ctx, p + 8);
    let (len, kd) = array(ctx, keys);
    let Some(j) = (0..len).find(|j| {
        read_str(&CtxReader(&*ctx), word_at(ctx, kd + 8 * j) as usize) == Value::Str(key.clone())
    }) else {
        return Ok(0);
    };
    let k = take_at(ctx, keys, j);
    ctx.call(&format!("{}str", words::DEC), &[Raw::I32(k as i32)], 0)?;
    let values = word_at(ctx, p + 12);
    Ok(take_at(ctx, values, j))
}

// `list.put(index, value)` or `list.insert(index, value)` in place: the
// value shared, not copied, and whatever `put` replaced released.
fn list_place(
    ctx: &mut dyn ImportCtx,
    name: &str,
    cell: u32,
    index: &Raw,
    v: u32,
) -> Result<Vec<Raw>, String> {
    let len = array(ctx, word_at(ctx, cell + 8)).0;
    let j = match index {
        Raw::I64(i) => u32::try_from(*i).ok(),
        r => u32::try_from(r.i32()).ok(),
    };
    let Some(j) = j.filter(|j| *j < len || (name == "insert" && *j == len)) else {
        return Ok(vec![Raw::I32(0)]);
    };
    retain(ctx, v);
    let list = array_of(ctx, cell, 8)?;
    if name == "put" {
        let data = array(ctx, list).1;
        let old = word_at(ctx, data + 8 * j);
        put(ctx, data + 8 * j, v);
        release_any(ctx, old)?;
    } else {
        append(ctx, list, v)?;
        let data = array(ctx, list).1;
        for i in (j + 1..=len).rev() {
            let prev = word_at(ctx, data + 8 * (i - 1));
            put(ctx, data + 8 * i, prev);
        }
        put(ctx, data + 8 * j, v);
    }
    Ok(vec![Raw::I32(1)])
}

// `map.set(key, value)` in place: the value shared, not copied.
fn set_in(
    ctx: &mut dyn ImportCtx,
    meta: &Meta,
    cell: u32,
    key: &str,
    v: u32,
) -> Result<(), String> {
    retain(ctx, v);
    let values = array_of(ctx, cell, 12)?;
    if let Some(j) = (0..array(ctx, word_at(ctx, cell + 8)).0).find(|j| {
        let (_, kd) = array(ctx, word_at(ctx, cell + 8));
        read_str(&CtxReader(ctx), word_at(ctx, kd + 8 * j) as usize) == Value::Str(key.into())
    }) {
        let (_, vd) = array(ctx, values);
        let old = word_at(ctx, vd + 8 * j);
        put(ctx, vd + 8 * j, v);
        return release_any(ctx, old);
    }
    let keys = array_of(ctx, cell, 8)?;
    let k = encode_owned(ctx, meta, &Ty::Str, &Value::Str(key.into()))?[0].i32() as u32;
    append(ctx, keys, k)?;
    append(ctx, values, v)
}

/// The methods that make or change a map or a list; `None` for the rest.
fn change(
    ctx: &mut dyn ImportCtx,
    meta: &Meta,
    name: &str,
    (p, tag): (u32, i32),
    args: &[Raw],
) -> Result<Option<Vec<Raw>>, String> {
    let key = |ctx: &dyn ImportCtx| match read_str(&CtxReader(ctx), args[1].i32() as u32 as usize) {
        Value::Str(s) => s,
        _ => String::new(),
    };
    let m = CtxReader(&*ctx);
    let owned = |ctx: &mut dyn ImportCtx, ty: Ty, v: Value| {
        encode_owned(ctx, meta, &ty, &v).map(smallvec::SmallVec::into_vec)
    };
    let out = match name {
        "set" if tag == MAP => {
            let k = key(ctx);
            set_in(ctx, meta, p, &k, args[2].i32() as u32)?;
            Ok(vec![Raw::I32(1)])
        }
        "push" if tag == LIST => {
            let v = args[1].i32() as u32;
            retain(ctx, v);
            let list = array_of(ctx, p, 8)?;
            append(ctx, list, v)?;
            Ok(vec![Raw::I32(1)])
        }
        "put" | "insert" if tag == LIST => {
            let v = args[2].i32() as u32;
            list_place(ctx, name, p, &args[1], v)
        }
        "set" | "push" | "put" | "insert" => Ok(vec![Raw::I32(0)]),
        "remove" => {
            let k = read_any(meta, &m, args[1].i32() as u32 as usize);
            let got = remove_in(ctx, (p, tag), &k)?;
            Ok(vec![Raw::I32(got as i32)])
        }
        "with" => {
            let mut entries = match read_any(meta, &m, p as usize) {
                Value::Map(entries) => entries,
                _ => vec![],
            };
            let k = key(ctx);
            let v = read_any(meta, &CtxReader(&*ctx), args[2].i32() as u32 as usize);
            match entries.iter_mut().find(|(e, _)| *e == k) {
                Some((_, slot)) => *slot = v,
                None => entries.push((k, v)),
            }
            owned(ctx, Ty::Any, Value::Map(entries))
        }
        _ => return Ok(None),
    };
    Ok(Some(out?))
}

/// `any.name(args)`, the `any` borrowed as `args[0]`.
pub(crate) fn run(
    ctx: &mut dyn ImportCtx,
    meta: &Meta,
    name: &str,
    args: &[Raw],
) -> Result<Vec<Raw>, String> {
    let p = args[0].i32() as u32;
    let tag = if p == 0 {
        0
    } else {
        rd32(&CtxReader(&*ctx), p as usize + 4) as i32
    };
    if let Some(out) = change(ctx, meta, name, (p, tag), args)? {
        return Ok(out);
    }
    let m = CtxReader(&*ctx);
    let at = p as usize + 8;
    let word = || rd32(&CtxReader(&*ctx), at);
    let int = |i: usize| match args.get(i) {
        Some(Raw::I64(x)) => *x,
        Some(r) => i64::from(r.i32()),
        None => 0,
    };
    let key = |ctx: &dyn ImportCtx| match read_str(&CtxReader(ctx), args[1].i32() as u32 as usize) {
        Value::Str(s) => s,
        _ => String::new(),
    };
    let child = |ctx: &mut dyn ImportCtx, c: Option<u32>| {
        let c = c.unwrap_or(0);
        retain(ctx, c);
        Ok(vec![Raw::I32(c as i32)])
    };
    let owned = |ctx: &mut dyn ImportCtx, ty: Ty, v: Value| {
        encode_owned(ctx, meta, &ty, &v).map(smallvec::SmallVec::into_vec)
    };
    match name {
        "kind" => owned(ctx, Ty::Str, Value::Str(kind_name(tag).into())),
        "equals" | "sorted" => compare(ctx, meta, name, p, args),
        "is_nil" => Ok(vec![Raw::I32(i32::from(p == 0))]),
        "as_int" => Ok(vec![Raw::I64(match tag {
            INT => rd64(&m, at) as i64,
            FLOAT => f64::from_bits(rd64(&m, at)) as i64,
            BOOL => i64::from(rd32(&m, at)),
            _ => 0,
        })]),
        "as_float" => Ok(vec![Raw::F64(match tag {
            FLOAT => float(rd64(&m, at)),
            INT => rd64(&m, at) as i64 as f64,
            _ => 0.0,
        })]),
        "as_bool" => Ok(vec![Raw::I32(i32::from(tag == BOOL && rd32(&m, at) != 0))]),
        "as_node" => Ok(vec![Raw::I64(if tag == NODE {
            rd64(&m, at) as i64
        } else {
            0
        })]),
        "as_str" if tag == STR => {
            let s = word();
            child(ctx, Some(s))
        }
        "as_str" => owned(ctx, Ty::Str, Value::Str(String::new())),
        "len" => Ok(vec![Raw::I64(i64::from(match tag {
            LIST | MAP => array(ctx, word()).0,
            STR => rd32(&m, word() as usize + 4),
            _ => 0,
        }))]),
        "get" if tag == MAP => {
            let found = lookup(ctx, p, &key(ctx));
            child(ctx, found)
        }
        "has" => Ok(vec![Raw::I32(i32::from(
            tag == MAP && lookup(ctx, p, &key(ctx)).is_some(),
        ))]),
        "at" if tag == LIST => {
            let (len, data) = array(ctx, word());
            let i = int(1);
            let found = u32::try_from(i)
                .ok()
                .filter(|i| *i < len)
                .map(|i| rd32(&CtxReader(&*ctx), (data + 8 * i) as usize));
            child(ctx, found)
        }
        "get" | "at" => child(ctx, None),
        "keys" => {
            let keys = match read_any(meta, &m, p as usize) {
                Value::Map(entries) => entries.into_iter().map(|(k, _)| Value::Str(k)).collect(),
                _ => vec![],
            };
            owned(ctx, Ty::Array(Box::new(Ty::Str)), Value::Array(keys))
        }
        "text" => {
            let v = read_any(meta, &m, p as usize);
            owned(ctx, Ty::Str, Value::Str(text(&v)))
        }
        _ => Err(format!("an `any` has no `{name}`")),
    }
}

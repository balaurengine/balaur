//! The batteries a script reaches for daily, as engine ops: assets, the
//! log, the random stream, what the platform is, the wall clock, hashes,
//! base64, an id, and a node query. Facts are recorded, so a replay answers
//! as the original run did.
#![allow(
    clippy::unnecessary_wraps,
    reason = "every op has the one signature the `ENGINE_OPS` table takes"
)]

use anyhow::{Result, anyhow};
use balaur_script::Value;

use crate::engine::Engine;
use crate::engine_api::{integer, number, text};
use crate::rng::Pcg32;

pub(crate) fn assets_load(eng: &Engine, args: &[Value]) -> Result<Value> {
    let definition = crate::assets::definition(eng, text(args, 0)?)?;
    crate::node_api::from_toml(&definition)
}

/// A private copy: read past the cache, so editing it cannot disturb what
/// every other holder of that reference sees.
pub(crate) fn assets_duplicate(eng: &Engine, args: &[Value]) -> Result<Value> {
    let definition = crate::assets::duplicate_definition(eng, text(args, 0)?)?;
    crate::node_api::from_toml(&definition)
}

pub(crate) fn assets_exists(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Bool(crate::assets::exists(eng, text(args, 0)?)))
}

/// Forget a reference, so the next load re-reads its source. What the editor
/// calls after writing an asset file.
pub(crate) fn assets_reload(eng: &Engine, args: &[Value]) -> Result<Value> {
    crate::assets::reload(eng, text(args, 0)?)?;
    Ok(Value::Nil)
}

pub(crate) fn assets_invalidate(eng: &Engine, _args: &[Value]) -> Result<Value> {
    crate::assets::invalidate(eng);
    Ok(Value::Nil)
}

/// Write a definition table back to the file a reference names, and forget the
/// cached copy so the next load reads what was written.
pub(crate) fn assets_save(eng: &Engine, args: &[Value]) -> Result<Value> {
    let definition = crate::node_api::to_toml(
        args.get(1)
            .ok_or_else(|| anyhow!("assets.save needs the table to write"))?,
    )?;
    crate::assets::save(eng, text(args, 0)?, &definition)?;
    Ok(Value::Nil)
}

/// Move a file or directory and rewrite every reference to it in the
/// project's `.toml` files; answers the files rewritten.
pub(crate) fn assets_rename(eng: &Engine, args: &[Value]) -> Result<Value> {
    let rewritten = crate::asset_index::rename(eng, text(args, 0)?, text(args, 1)?)?;
    Ok(Value::List(rewritten.into_iter().map(Value::Str).collect()))
}

/// The id `assets/index.toml` gives a path, or nil when it has none.
pub(crate) fn assets_id(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(crate::asset_index::id_of(eng, text(args, 0)?)?.map_or(Value::Nil, Value::Str))
}

/// The id a file has, giving it one and writing the index if it has none.
pub(crate) fn assets_assign_id(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Str(crate::asset_index::assign_id(
        eng,
        text(args, 0)?,
    )?))
}

/// The path an `id://` reference resolves to in the running project; a
/// path comes back as itself.
pub(crate) fn assets_path(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Str(crate::project::path_of(eng, text(args, 0)?)?))
}

/// Where files of an asset type belong, as its plugin declared it.
///
/// The editor promotes an inline definition to a file and has to put it
/// somewhere; only the type knows where. Empty when the type is unknown or
/// declared no directory, which a caller reads as "cannot promote".
pub(crate) fn assets_directory(eng: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Str(crate::assets::directory(eng, text(args, 0)?)))
}

/// The three writers a script has. They emit through `tracing`, so a scripted
/// line lands in the same stream, and the same `logbuf`, as an engine one --
/// which is what makes `log.recent` able to show both.
pub(crate) fn log_info(_: &Engine, args: &[Value]) -> Result<Value> {
    tracing::info!("[script] {}", text(args, 0)?);
    Ok(Value::Nil)
}

pub(crate) fn log_warn(_: &Engine, args: &[Value]) -> Result<Value> {
    tracing::warn!("[script] {}", text(args, 0)?);
    Ok(Value::Nil)
}

pub(crate) fn log_error(_: &Engine, args: &[Value]) -> Result<Value> {
    tracing::error!("[script] {}", text(args, 0)?);
    Ok(Value::Nil)
}

pub(crate) fn log_recent(_: &Engine, args: &[Value]) -> Result<Value> {
    let n = match args.first() {
        Some(Value::Int(n)) => usize::try_from(*n).unwrap_or(100),
        Some(Value::Num(n)) => *n as usize,
        _ => 100,
    };
    Ok(Value::List(
        crate::logbuf::recent(n)
            .into_iter()
            .map(|e| {
                // The structured fields ride along: a viewer that drops them
                // shows a message the event deliberately did not put there.
                let fields = e
                    .fields
                    .iter()
                    .map(|(name, value)| {
                        Value::Map(vec![
                            ("name".into(), Value::Str(name.clone())),
                            ("value".into(), Value::Str(value.clone())),
                        ])
                    })
                    .collect();
                Value::Map(vec![
                    ("time".into(), Value::Num(e.time)),
                    ("level".into(), Value::Str(e.level.clone())),
                    ("tag".into(), Value::Str(e.tag.clone())),
                    ("message".into(), Value::Str(e.message.clone())),
                    ("fields".into(), Value::List(fields)),
                ])
            })
            .collect(),
    ))
}

pub(crate) fn log_clear(_: &Engine, _: &[Value]) -> Result<Value> {
    crate::logbuf::clear();
    Ok(Value::Nil)
}

pub(crate) fn rng_seed(eng: &Engine, args: &[Value]) -> Result<Value> {
    let seed = match args.first() {
        Some(Value::Int(n)) => *n,
        Some(Value::Num(n)) => *n as i64,
        other => return Err(anyhow!("seed should be a number, got {other:?}")),
    };
    crate::rng::with_rng(eng, |rng| *rng = Pcg32::new(seed as u64));
    Ok(Value::Nil)
}

pub(crate) fn rng_random(eng: &Engine, _: &[Value]) -> Result<Value> {
    let v = crate::rng::with_rng(eng, Pcg32::next_f64);
    Ok(Value::Num(v))
}

pub(crate) fn rng_range(eng: &Engine, args: &[Value]) -> Result<Value> {
    let (lo, hi) = (number(args, 0)?, number(args, 1)?);
    let v = crate::rng::with_rng(eng, Pcg32::next_f64);
    Ok(Value::Num(v.mul_add(hi - lo, lo)))
}

pub(crate) fn rng_int(eng: &Engine, args: &[Value]) -> Result<Value> {
    let (lo, hi) = (integer(args, 0)?, integer(args, 1)?);
    let v = crate::rng::with_rng(eng, |rng| rng.next_range_i64(lo, hi));
    Ok(Value::Int(v))
}

pub(crate) fn platform(eng: &Engine, _: &[Value]) -> Result<Value> {
    let facts = crate::facts::platform(eng);
    Ok(Value::Map(vec![
        ("os".into(), Value::Str(facts.os)),
        ("web".into(), Value::Bool(facts.web)),
        ("mobile".into(), Value::Bool(facts.mobile)),
        ("editor".into(), Value::Bool(facts.editor)),
    ]))
}

pub(crate) fn device_id(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Str(crate::facts::platform(eng).device_id))
}

pub(crate) fn focused(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Bool(crate::facts::device(eng).focused))
}

pub(crate) fn dark_mode(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Bool(crate::facts::device(eng).dark_mode))
}

pub(crate) fn unix_time(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(Value::Num(
        eng.resource::<crate::facts::WallClock>().borrow().unix_time,
    ))
}

pub(crate) fn strings_system_locale(eng: &Engine, _: &[Value]) -> Result<Value> {
    Ok(crate::facts::platform(eng)
        .system_locale
        .map_or(Value::Nil, Value::Str))
}

pub(crate) fn scene_tagged(eng: &Engine, args: &[Value]) -> Result<Value> {
    let tag = text(args, 0)?;
    let world = eng.world();
    Ok(Value::List(
        crate::scene::tagged(&world, eng.root(), tag)
            .into_iter()
            .map(|e| Value::Node(crate::node_id_of(e).0))
            .collect(),
    ))
}

/// A version-4 UUID from the engine stream: four draws, the version and
/// variant nibbles set as the format says.
pub(crate) fn rng_uuid(eng: &Engine, _: &[Value]) -> Result<Value> {
    let words = crate::rng::with_rng(eng, |rng| {
        [
            rng.next_u32(),
            rng.next_u32(),
            rng.next_u32(),
            rng.next_u32(),
        ]
    });
    let mut bytes = [0u8; 16];
    for (i, word) in words.iter().enumerate() {
        bytes[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = hex_of(&bytes);
    Ok(Value::Str(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )))
}

pub(crate) fn hex_digest(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let digest = sha2::Sha256::digest(bytes);
    hex_of(&digest)
}

pub(crate) fn hash_sha256(eng: &Engine, args: &[Value]) -> Result<Value> {
    let path = crate::file_api::resolve(eng, text(args, 0)?)?;
    let bytes = crate::files::backend(eng).read(&path)?;
    Ok(Value::Str(hex_digest(&bytes)))
}

pub(crate) fn hash_sha256_text(_: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Str(hex_digest(text(args, 0)?.as_bytes())))
}

pub(crate) fn encoding_base64(_: &Engine, args: &[Value]) -> Result<Value> {
    use base64::Engine as _;
    let encoded = match args.first() {
        Some(Value::Bytes(bytes)) => base64::engine::general_purpose::STANDARD.encode(bytes),
        Some(Value::Str(text)) => base64::engine::general_purpose::STANDARD.encode(text.as_bytes()),
        other => return Err(anyhow!("base64 takes bytes or a string, got {other:?}")),
    };
    Ok(Value::Str(encoded))
}

pub(crate) fn encoding_from_base64(_: &Engine, args: &[Value]) -> Result<Value> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text(args, 0)?)
        .map_err(|err| anyhow!("not base64: {err}"))?;
    Ok(Value::Bytes(bytes))
}

/// Bytes as lowercase hex.
fn hex_of(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

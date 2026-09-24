//! The `regex` module: `regex-lite` over script strings, a pattern compiled
//! on every call and a match answered as a table.

use anyhow::{Result, anyhow};
use balaur_script::Value;

use crate::engine::Engine;
use crate::engine_api::text;

/// A byte offset as the integer a script reads; a text past 2^63 bytes is not one.
fn offset(at: usize) -> i64 {
    i64::try_from(at).unwrap_or(i64::MAX)
}

fn regex_of(args: &[Value]) -> Result<regex_lite::Regex> {
    let pattern = text(args, 0)?;
    regex_lite::Regex::new(pattern).map_err(|e| anyhow!("regex `{pattern}`: {e}"))
}

/// One match as a table: where it is, its text, and each group's text in
/// order, nil for a group that took no part.
fn regex_match(caps: &regex_lite::Captures<'_>) -> Value {
    let whole = caps.get(0).expect("group 0 is the match");
    let groups = caps
        .iter()
        .skip(1)
        .map(|g| g.map_or(Value::Nil, |m| Value::Str(m.as_str().to_string())))
        .collect();
    Value::Map(vec![
        ("start".into(), Value::Int(offset(whole.start()))),
        ("end".into(), Value::Int(offset(whole.end()))),
        ("text".into(), Value::Str(whole.as_str().to_string())),
        ("groups".into(), Value::List(groups)),
    ])
}

pub(crate) fn regex_matches(_: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Bool(regex_of(args)?.is_match(text(args, 1)?)))
}

pub(crate) fn regex_search(_: &Engine, args: &[Value]) -> Result<Value> {
    Ok(regex_of(args)?
        .captures(text(args, 1)?)
        .map_or(Value::Nil, |caps| regex_match(&caps)))
}

pub(crate) fn regex_search_all(_: &Engine, args: &[Value]) -> Result<Value> {
    let found = regex_of(args)?
        .captures_iter(text(args, 1)?)
        .map(|caps| regex_match(&caps))
        .collect();
    Ok(Value::List(found))
}

pub(crate) fn regex_replace(_: &Engine, args: &[Value]) -> Result<Value> {
    let with = text(args, 2)?;
    let all = matches!(args.get(3), Some(Value::Bool(true)));
    let re = regex_of(args)?;
    let out = if all {
        re.replace_all(text(args, 1)?, with)
    } else {
        re.replace(text(args, 1)?, with)
    };
    Ok(Value::Str(out.into_owned()))
}

pub(crate) fn regex_split(_: &Engine, args: &[Value]) -> Result<Value> {
    let parts = regex_of(args)?
        .split(text(args, 1)?)
        .map(|part| Value::Str(part.to_string()))
        .collect();
    Ok(Value::List(parts))
}

pub(crate) fn regex_escape(_: &Engine, args: &[Value]) -> Result<Value> {
    Ok(Value::Str(regex_lite::escape(text(args, 0)?)))
}
